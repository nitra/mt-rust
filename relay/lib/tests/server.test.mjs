import { Buffer } from 'node:buffer'
import { generateKeyPairSync, sign } from 'node:crypto'
import { once } from 'node:events'

import { WebSocket } from 'ws'
import { afterAll, beforeAll, expect, test } from 'vitest'

import { DevMagicAuth, KratosAuth } from '../auth.mjs'
import { RelayCore } from '../relay.mjs'
import { startRelayServer } from '../server.mjs'
import { transferMessage } from '../signing.mjs'
import { InMemoryStore } from '../store.mjs'

/**
 * Детермінований hex-pubkey (32 байти) з імені.
 * @param {string} name імʼя пристрою
 * @returns {string} hex-рядок 64 символи
 */
function fakeKey(name) {
  return Buffer.from(name, 'utf8').toString('hex').padEnd(64, '0').slice(0, 64)
}

const RE_HELLO = /hello/
const RE_VIEWER = /viewer/
const RE_HEX_KEY = /^[0-9a-f]{64}$/
const RE_SIGNATURE = /підпис/
const RE_MEMBER = /не учасник/
const RE_SESSION = /сесія відхилена/
const RE_HEX_KEY_MSG = /hex Ed25519/
const RE_NO_ISSUE = /не видає сесій/
// Тести ходять на локальний loopback без TLS; sdl-правило про insecure-URL
// націлене на продакшн-адреси, тому схему складаємо окремо від хоста.
const WS_SCHEME = 'ws:'

/** @type {InMemoryStore} */
const store = new InMemoryStore()
/** @type {{ port: number, close: () => Promise<void> }} */
let server
/** @type {string} */
let hostToken
/** @type {string} */
let viewerToken

/**
 * Відкриває WS-клієнт і чекає open.
 * @returns {Promise<WebSocket>} відкритий сокет
 */
async function connect() {
  const socket = new WebSocket(`${WS_SCHEME}//127.0.0.1:${server.port}`)
  await once(socket, 'open')
  return socket
}

/**
 * Шле кадр і чекає наступний вхідний JSON-кадр.
 * @param {WebSocket} socket сокет
 * @param {object} frame кадр для відправки
 * @returns {Promise<object>} відповідь relay
 */
async function roundtrip(socket, frame) {
  socket.send(JSON.stringify(frame))
  const [raw] = await once(socket, 'message')
  return JSON.parse(String(raw))
}

/**
 * Сокет із буфером усіх отриманих кадрів.
 *
 * Навіщо не `once(socket, 'message')`: одноразовий слухач ловить лише той
 * кадр, що прийшов, поки на ньому чекають, — а relay шле пачками (реплей
 * буфера кімнати одразу після `subscribe`), тож кадри між очікуваннями
 * просто губляться. Тут слухач стоїть постійно, а чекання йде по буферу.
 * @param {number} port порт relay
 * @returns {Promise<{socket: WebSocket, waitFor: (matches: (frame: object) => boolean) => Promise<object>}>} сокет і очікувач
 */
async function collectingSocket(port) {
  const socket = new WebSocket(`${WS_SCHEME}//127.0.0.1:${port}`)
  const inbox = []
  let notify = null
  socket.on('message', raw => {
    inbox.push(JSON.parse(String(raw)))
    notify?.()
  })
  await once(socket, 'open')
  const waitFor = async matches => {
    for (;;) {
      const index = inbox.findIndex(matches)
      if (index >= 0) return inbox.splice(index, 1)[0]
      await new Promise(resolve => {
        notify = resolve
      })
    }
  }
  return { socket, waitFor }
}

/** @type {object} */
let owner
/** @type {object} */
let approver
/** @type {import('node:crypto').KeyObject} */
let ownerPrivateKey

beforeAll(async () => {
  owner = await store.createAccount({ email: 'owner@x' })
  const viewer = await store.createAccount({ email: 'viewer@x' })
  approver = await store.createAccount({ email: 'approver@x' })
  await store.createTask('root-1', owner.account_id)
  await store.setMemberRole('root-1', viewer.account_id, 'viewer')
  await store.setMemberRole('root-1', approver.account_id, 'approver')
  const pair = generateKeyPairSync('ed25519')
  ownerPrivateKey = pair.privateKey
  hostToken = (
    await store.registerDevice(owner.account_id, {
      name: 'mac',
      role: 'host',
      pubkey: pair.publicKey.export({ format: 'der', type: 'spki' }).subarray(-32).toString('hex')
    })
  ).device_token
  viewerToken = (
    await store.registerDevice(viewer.account_id, {
      name: 'tab',
      role: 'client',
      pubkey: fakeKey('tab')
    })
  ).device_token
  server = await startRelayServer(new RelayCore({ store, auth: new DevMagicAuth({ store }) }))
})

afterAll(async () => {
  await server.close()
})

test('невірний device_token → error; кадри до hello відхиляються', async () => {
  const socket = await connect()
  const denied = await roundtrip(socket, { kind: 'subscribe', root: 'root-1' })
  expect(denied.kind).toBe('error')
  expect(denied.message).toMatch(RE_HELLO)
  const bad = await roundtrip(socket, { kind: 'hello', device_token: 'чужий' })
  expect(bad.kind).toBe('error')
  socket.close()
})

test('hello → subscribe → envelope доходить підписнику; реплей після реконекту', async () => {
  const publisher = await connect()
  const helloReply = await roundtrip(publisher, { kind: 'hello', device_token: hostToken })
  expect(helloReply.kind).toBe('ok')

  const subscriber = await connect()
  await roundtrip(subscriber, { kind: 'hello', device_token: viewerToken })
  await roundtrip(subscriber, { kind: 'subscribe', root: 'root-1' })

  publisher.send(JSON.stringify({ kind: 'envelope', root: 'root-1', envelope: { seq: 0, node_hash: 'demo' } }))
  const [raw] = await once(subscriber, 'message')
  const delivered = JSON.parse(String(raw))
  expect(delivered).toEqual({ kind: 'envelope', envelope: { seq: 0, node_hash: 'demo' }, from_host: true })
  subscriber.close()

  // Реконект: буфер кімнати реплеїться одразу після subscribe.
  const reconnected = await connect()
  await roundtrip(reconnected, { kind: 'hello', device_token: viewerToken })
  reconnected.send(JSON.stringify({ kind: 'subscribe', root: 'root-1' }))
  const [replayRaw] = await once(reconnected, 'message')
  expect(JSON.parse(String(replayRaw))).toEqual({
    kind: 'envelope',
    envelope: { seq: 0, node_hash: 'demo' },
    from_host: true
  })
  reconnected.close()
  publisher.close()
})

test('pubkeys-кадр: pubkey-и approver+ пристроїв для перевірки підписів', async () => {
  const socket = await connect()
  await roundtrip(socket, { kind: 'hello', device_token: viewerToken })
  const reply = await roundtrip(socket, { kind: 'pubkeys', root: 'root-1' })
  expect(reply.kind).toBe('pubkeys')
  expect(reply.root).toBe('root-1')
  // Owner (approver+) — так; viewer — ні.
  expect(reply.pubkeys.map(k => k.account_id)).toEqual([owner.account_id])
  expect(reply.pubkeys[0].pubkey).toMatch(RE_HEX_KEY)
  socket.close()
})

test('membership через WS: invite → accept новим акаунтом', async () => {
  const socket = await connect()
  await roundtrip(socket, { kind: 'hello', device_token: hostToken })
  const invited = await roundtrip(socket, { kind: 'invite', root: 'root-1', email: 'new@x', role: 'host' })
  expect(invited).toMatchObject({ kind: 'ok', status: 'pending' })

  const newcomer = await store.createAccount({ email: 'new@x' })
  const token = (
    await store.registerDevice(newcomer.account_id, {
      name: 'new-phone',
      role: 'client',
      pubkey: fakeKey('new-phone')
    })
  ).device_token
  const other = await connect()
  await roundtrip(other, { kind: 'hello', device_token: token })
  const accepted = await roundtrip(other, { kind: 'accept', invitation_id: invited.invitation_id })
  expect(accepted).toEqual({ kind: 'ok', root: 'root-1', role: 'host' })
  expect(await store.memberRole('root-1', newcomer.account_id)).toBe('host')
  socket.close()
  other.close()
})

test('transfer_ownership через WS: без підпису — error, з підписом — передано', async () => {
  const socket = await connect()
  await roundtrip(socket, { kind: 'hello', device_token: hostToken })

  const unsigned = await roundtrip(socket, {
    kind: 'transfer_ownership',
    root: 'root-1',
    to_account: approver.account_id
  })
  expect(unsigned.kind).toBe('error')
  expect(unsigned.message).toMatch(RE_SIGNATURE)

  const signature = sign(
    null,
    transferMessage({ root: 'root-1', fromAccount: owner.account_id, toAccount: approver.account_id }),
    ownerPrivateKey
  ).toBase64()
  const transferred = await roundtrip(socket, {
    kind: 'transfer_ownership',
    root: 'root-1',
    to_account: approver.account_id,
    signature
  })
  expect(transferred).toEqual({ kind: 'ok', transferred: 'root-1', to_account: approver.account_id })
  expect(await store.memberRole('root-1', approver.account_id)).toBe('owner')
  expect(await store.memberRole('root-1', owner.account_id)).toBe('host')
  socket.close()
})

test('bootstrap_owners через WS: сідинг з owner:-розмітки (новим owner-ом)', async () => {
  // Після transfer вище owner кореня — approver; реєструємо його пристрій.
  const token = (
    await store.registerDevice(approver.account_id, {
      name: 'approver-mac',
      role: 'client',
      pubkey: fakeKey('approver-mac')
    })
  ).device_token
  const socket = await connect()
  await roundtrip(socket, { kind: 'hello', device_token: token })
  const reply = await roundtrip(socket, {
    kind: 'bootstrap_owners',
    root: 'root-1',
    entries: [{ email: 'viewer@x', role: 'owner' }, { email: 'ghost@x' }]
  })
  expect(reply.kind).toBe('ok')
  expect(reply.bootstrap).toEqual({ added: [], invited: ['ghost@x'], kept: ['viewer@x'] })
  socket.close()
})

test('viewer не шле клієнтські події через WS', async () => {
  const socket = await connect()
  await roundtrip(socket, { kind: 'hello', device_token: viewerToken })
  const rejected = await roundtrip(socket, {
    kind: 'envelope',
    root: 'root-1',
    envelope: { seq: 1 }
  })
  expect(rejected.kind).toBe('error')
  expect(rejected.message).toMatch(RE_VIEWER)
  socket.close()
})

test('login → register_device → hello: повний шлях від email до кімнати', async () => {
  // Ланка, якої не було: до цього device_token можна було здобути лише
  // прямим викликом store, тобто мережевого шляху «людина → пристрій»
  // не існувало взагалі.
  const socket = await connect()
  const session = await roundtrip(socket, { kind: 'login', email: 'newcomer@x' })
  expect(session.kind).toBe('session')
  expect(session.token).toBeTruthy()

  const device = await roundtrip(socket, {
    kind: 'register_device',
    session_token: session.token,
    name: 'phone',
    role: 'client',
    pubkey: fakeKey('phone')
  })
  expect(device.kind).toBe('device')

  const hello = await roundtrip(socket, { kind: 'hello', device_token: device.device_token })
  expect(hello.kind).toBe('ok')
  expect(hello.device_id).toBe(device.device_id)

  // Пристрій справжній, але membership лишається окремим гейтом.
  const denied = await roundtrip(socket, { kind: 'subscribe', root: 'root-1' })
  expect(denied.kind).toBe('error')
  expect(denied.message).toMatch(RE_MEMBER)
  socket.close()
})

test('register_device: чужа сесія і невалідний pubkey відхиляються', async () => {
  const socket = await connect()
  const badSession = await roundtrip(socket, {
    kind: 'register_device',
    session_token: 'вигаданий',
    name: 'x',
    role: 'client',
    pubkey: fakeKey('x')
  })
  expect(badSession.kind).toBe('error')
  expect(badSession.message).toMatch(RE_SESSION)

  const session = await roundtrip(socket, { kind: 'login', email: 'badkey@x' })
  const badKey = await roundtrip(socket, {
    kind: 'register_device',
    session_token: session.token,
    name: 'x',
    role: 'client',
    pubkey: 'коротко'
  })
  expect(badKey.kind).toBe('error')
  expect(badKey.message).toMatch(RE_HEX_KEY_MSG)
  socket.close()
})

test('у продакшн-режимі кадр login відмовляє за побудовою', async () => {
  // KratosAuth не має issueSession — шлях закритий відсутністю методу,
  // а не прапорцем конфігурації, який можна забути виставити.
  const prodStore = new InMemoryStore()
  const prod = await startRelayServer(
    new RelayCore({
      store: prodStore,
      auth: new KratosAuth({ store: prodStore, baseUrl: 'https://kratos.test', fetch: async () => ({ ok: false }) })
    })
  )
  const socket = new WebSocket(`${WS_SCHEME}//127.0.0.1:${prod.port}`)
  await once(socket, 'open')
  const rejected = await roundtrip(socket, { kind: 'login', email: 'prod@x' })
  expect(rejected.kind).toBe('error')
  expect(rejected.message).toMatch(RE_NO_ISSUE)
  socket.close()
  await prod.close()
})

test('set_push_token: пристрій реєструє і знімає токен транспорту', async () => {
  const socket = await connect()
  const hello = await roundtrip(socket, { kind: 'hello', device_token: hostToken })
  const set = await roundtrip(socket, { kind: 'set_push_token', push_token: 'fcm-abc' })
  expect(set).toEqual({ kind: 'ok', push_token: true })
  expect(await store.pushTokensFor(owner.account_id)).toEqual([{ device_id: hello.device_id, push_token: 'fcm-abc' }])

  await roundtrip(socket, { kind: 'set_push_token', push_token: '' })
  expect(await store.pushTokensFor(owner.account_id)).toEqual([])
  socket.close()
})

test('presence через WS: оголошення, who, зняття при розриві', async () => {
  const watcher = await collectingSocket(server.port)
  watcher.socket.send(JSON.stringify({ kind: 'hello', device_token: viewerToken }))
  await watcher.waitFor(frame => Boolean(frame.device_id))
  watcher.socket.send(JSON.stringify({ kind: 'subscribe', root: 'root-1' }))
  await watcher.waitFor(frame => frame.subscribed === 'root-1')

  const announcer = await collectingSocket(server.port)
  announcer.socket.send(JSON.stringify({ kind: 'hello', device_token: hostToken }))
  await announcer.waitFor(frame => Boolean(frame.device_id))
  announcer.socket.send(
    JSON.stringify({
      kind: 'presence',
      root: 'root-1',
      hostname: 'mac-vitalii',
      projects: ['mt'],
      nodes: ['mt/demo']
    })
  )
  const announced = await announcer.waitFor(frame => Boolean(frame.presence))
  expect(announced.presence).toMatchObject({ hostname: 'mac-vitalii' })

  // Учасник кімнати бачить зміну присутності live.
  const changed = await watcher.waitFor(frame => frame.event?.type === 'PresenceChanged')
  expect(changed.event).toMatchObject({ hostname: 'mac-vitalii', nodes: ['mt/demo'] })

  watcher.socket.send(JSON.stringify({ kind: 'who', root: 'root-1' }))
  const who = await watcher.waitFor(frame => frame.kind === 'presence')
  expect(who.devices).toHaveLength(1)
  expect(who.devices[0]).toMatchObject({ hostname: 'mac-vitalii', projects: ['mt'] })

  // Розрив зʼєднання знімає presence негайно, не чекаючи TTL.
  announcer.socket.close()
  const gone = await watcher.waitFor(frame => frame.event?.gone === true)
  expect(gone.event.type).toBe('PresenceChanged')

  watcher.socket.send(JSON.stringify({ kind: 'who', root: 'root-1' }))
  const empty = await watcher.waitFor(frame => frame.kind === 'presence')
  expect(empty.devices).toEqual([])
  watcher.socket.close()
})
