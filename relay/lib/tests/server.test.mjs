import { Buffer } from 'node:buffer'
import { generateKeyPairSync, sign } from 'node:crypto'
import { once } from 'node:events'

import { WebSocket } from 'ws'
import { afterAll, beforeAll, expect, test } from 'vitest'

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

/** @type {object} */
let owner
/** @type {object} */
let approver
/** @type {import('node:crypto').KeyObject} */
let ownerPrivateKey

beforeAll(async () => {
  owner = store.createAccount({ email: 'owner@x' })
  const viewer = store.createAccount({ email: 'viewer@x' })
  approver = store.createAccount({ email: 'approver@x' })
  store.createTask('root-1', owner.account_id)
  store.setMemberRole('root-1', viewer.account_id, 'viewer')
  store.setMemberRole('root-1', approver.account_id, 'approver')
  const pair = generateKeyPairSync('ed25519')
  ownerPrivateKey = pair.privateKey
  hostToken = store.registerDevice(owner.account_id, {
    name: 'mac',
    role: 'host',
    pubkey: pair.publicKey.export({ format: 'der', type: 'spki' }).subarray(-32).toString('hex')
  }).device_token
  viewerToken = store.registerDevice(viewer.account_id, {
    name: 'tab',
    role: 'client',
    pubkey: fakeKey('tab')
  }).device_token
  server = await startRelayServer(new RelayCore({ store }))
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

  const newcomer = store.createAccount({ email: 'new@x' })
  const token = store.registerDevice(newcomer.account_id, {
    name: 'new-phone',
    role: 'client',
    pubkey: fakeKey('new-phone')
  }).device_token
  const other = await connect()
  await roundtrip(other, { kind: 'hello', device_token: token })
  const accepted = await roundtrip(other, { kind: 'accept', invitation_id: invited.invitation_id })
  expect(accepted).toEqual({ kind: 'ok', root: 'root-1', role: 'host' })
  expect(store.memberRole('root-1', newcomer.account_id)).toBe('host')
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
  expect(store.memberRole('root-1', approver.account_id)).toBe('owner')
  expect(store.memberRole('root-1', owner.account_id)).toBe('host')
  socket.close()
})

test('bootstrap_owners через WS: сідинг з owner:-розмітки (новим owner-ом)', async () => {
  // Після transfer вище owner кореня — approver; реєструємо його пристрій.
  const token = store.registerDevice(approver.account_id, {
    name: 'approver-mac',
    role: 'client',
    pubkey: fakeKey('approver-mac')
  }).device_token
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
