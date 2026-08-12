/**
 * Контракт sink-а push: `deliver(accountId, note) → {delivered, dropped}`.
 *
 * Той самий підхід, що в store- і auth-контрактах: реалізацій дві —
 * `DevPushSink` (памʼять) і `FcmPushSink` (FCM HTTP v1), і взаємозамінність
 * доводиться однаковою поведінкою.
 *
 * FCM-реалізація працює через інʼєктований `fetch`: набір доводить, що
 * relay формує документовані запити і правильно реагує на документовані
 * відповіді, і НЕ доводить доставки живим FCM.
 */
import { Buffer } from 'node:buffer'
import { generateKeyPairSync } from 'node:crypto'

import { describe, expect, test } from 'vitest'

import { FcmPushSink, GoogleAccessToken } from '../fcm-sink.mjs'
import { DevPushSink } from '../push-sink.mjs'
import { InMemoryStore } from '../store.mjs'

const { privateKey } = generateKeyPairSync('rsa', { modulusLength: 2048 })
const PEM = privateKey.export({ format: 'pem', type: 'pkcs8' }).toString()

/**
 * Акаунт із зареєстрованим пристроєм і push-токеном.
 * @param {InMemoryStore} store store
 * @param {string} email email акаунта
 * @param {string} pushToken токен транспорту
 * @returns {Promise<string>} account_id
 */
async function accountWithPush(store, email, pushToken) {
  const account = await store.createAccount({ email })
  const device = await store.registerDevice(account.account_id, {
    name: 'phone',
    role: 'client',
    pubkey: 'ab'.repeat(32)
  })
  await store.setPushToken(device.device_id, pushToken)
  return account.account_id
}

/**
 * Stub Google/FCM: рахує виклики і віддає задані статуси.
 * @param {(url: string) => {ok: boolean, status: number, body?: object}} route маршрутизатор відповідей
 * @returns {{fetch: Function, calls: object[]}} клієнт і журнал викликів
 */
function googleStub(route) {
  const calls = []
  return {
    calls,
    fetch: async (url, options) => {
      calls.push({ url, options })
      const reply = route(url)
      return { ok: reply.ok, status: reply.status, json: async () => reply.body ?? {} }
    }
  }
}

/** Стандартний маршрут: token endpoint і messages:send віддають успіх. */
const OK_ROUTE = url =>
  url.includes('oauth2')
    ? { ok: true, status: 200, body: { access_token: 'bearer-1', expires_in: 3600 } }
    : { ok: true, status: 200, body: { name: 'projects/p/messages/1' } }

/**
 * Набір перевірок контракту.
 * @param {string} label назва реалізації у виводі
 * @param {() => Promise<{sink: object, store: InMemoryStore}>} makeSink фабрика
 * @returns {void}
 */
function sinkContract(label, makeSink) {
  describe(`push-sink-контракт: ${label}`, () => {
    test('доставка акаунту з пристроєм повертає delivered', async () => {
      const { sink, store } = await makeSink()
      const accountId = await accountWithPush(store, `s-${Date.now()}@x`, 'tok-1')
      const result = await sink.deliver(accountId, { type: 3, root: 'root-1', reason: 'AuditPending', ref: 'f_1' })
      expect(result.delivered).toBeGreaterThan(0)
      expect(result.dropped).toBe(0)
    })

    test('deliver повертає обʼєкт підсумку, а не кидає, на акаунті без пристроїв', async () => {
      const { sink, store } = await makeSink()
      const account = await store.createAccount({ email: `empty-${Date.now()}@x` })
      const result = await sink.deliver(account.account_id, { type: 1, root: 'root-1', reason: 'new-events' })
      expect(result).toMatchObject({ dropped: 0 })
    })
  })
}

sinkContract('dev', async () => ({ sink: new DevPushSink(), store: new InMemoryStore() }))

sinkContract('fcm', async () => {
  const store = new InMemoryStore()
  const stub = googleStub(OK_ROUTE)
  return {
    store,
    sink: new FcmPushSink({
      store,
      projectId: 'proj',
      accessToken: new GoogleAccessToken({ clientEmail: 'svc@proj.iam', privateKey: PEM, fetch: stub.fetch }),
      fetch: stub.fetch
    })
  }
})

describe('fcm: форма запиту', () => {
  /**
   * Готує sink із заданим маршрутом відповідей.
   * @param {Function} route маршрутизатор
   * @returns {{sink: FcmPushSink, store: InMemoryStore, stub: object}} набір
   */
  function setup(route) {
    const store = new InMemoryStore()
    const stub = googleStub(route)
    const sink = new FcmPushSink({
      store,
      projectId: 'proj',
      accessToken: new GoogleAccessToken({ clientEmail: 'svc@proj.iam', privateKey: PEM, fetch: stub.fetch }),
      fetch: stub.fetch
    })
    return { sink, store, stub }
  }

  test('data-повідомлення на messages:send: усі поля рядкові', async () => {
    // FCM приймає в `data` лише рядки — число чи null тут означали б
    // помилку 400 на кожній події.
    const { sink, store, stub } = setup(OK_ROUTE)
    const accountId = await accountWithPush(store, 'shape@x', 'tok-shape')
    await sink.deliver(accountId, { type: 1, root: 'root-1', reason: 'new-events', ref: null })

    const send = stub.calls.find(call => call.url.includes('messages:send'))
    expect(send.url).toBe('https://fcm.googleapis.com/v1/projects/proj/messages:send')
    expect(send.options.headers.Authorization).toBe('Bearer bearer-1')
    expect(JSON.parse(send.options.body)).toEqual({
      message: { token: 'tok-shape', data: { type: '1', root: 'root-1', reason: 'new-events', ref: '' } }
    })
  })

  test('access token кешується між доставками', async () => {
    // Без кешу сплеск подій у задачі став би сплеском логінів на token
    // endpoint.
    const { sink, store, stub } = setup(OK_ROUTE)
    const accountId = await accountWithPush(store, 'cache@x', 'tok-cache')
    await sink.deliver(accountId, { type: 1, root: 'r', reason: 'new-events' })
    await sink.deliver(accountId, { type: 1, root: 'r', reason: 'new-events' })
    expect(stub.calls.filter(call => call.url.includes('oauth2'))).toHaveLength(1)
  })

  test('протухлий токен знімається зі store, решта пристроїв не страждає', async () => {
    // Інакше видалений застосунок тягнув би помилку на кожній події назавжди.
    const store = new InMemoryStore()
    const stub = googleStub(url => {
      if (url.includes('oauth2')) return { ok: true, status: 200, body: { access_token: 'b', expires_in: 3600 } }
      return { ok: false, status: 404 }
    })
    const sink = new FcmPushSink({
      store,
      projectId: 'proj',
      accessToken: new GoogleAccessToken({ clientEmail: 'svc@proj.iam', privateKey: PEM, fetch: stub.fetch }),
      fetch: stub.fetch
    })
    const account = await store.createAccount({ email: 'stale@x' })
    const device = await store.registerDevice(account.account_id, {
      name: 'old',
      role: 'client',
      pubkey: 'cd'.repeat(32)
    })
    await store.setPushToken(device.device_id, 'tok-stale')

    const result = await sink.deliver(account.account_id, { type: 1, root: 'r', reason: 'new-events' })
    expect(result).toEqual({ delivered: 0, dropped: 1 })
    expect(await store.pushTokensFor(account.account_id)).toEqual([])
  })

  test('5xx не знімає токен — це збій сервера, а не мертвий пристрій', async () => {
    const store = new InMemoryStore()
    const stub = googleStub(url => {
      if (url.includes('oauth2')) return { ok: true, status: 200, body: { access_token: 'b', expires_in: 3600 } }
      return { ok: false, status: 503 }
    })
    const sink = new FcmPushSink({
      store,
      projectId: 'proj',
      accessToken: new GoogleAccessToken({ clientEmail: 'svc@proj.iam', privateKey: PEM, fetch: stub.fetch }),
      fetch: stub.fetch
    })
    const accountId = await accountWithPush(store, 'flaky@x', 'tok-flaky')
    expect(await sink.deliver(accountId, { type: 1, root: 'r', reason: 'new-events' })).toEqual({
      delivered: 0,
      dropped: 0
    })
    expect(await store.pushTokensFor(accountId)).toHaveLength(1)
  })

  test('відмова token endpoint — явна помилка, не тиха втрата push', async () => {
    const { sink, store } = setup(() => ({ ok: false, status: 401 }))
    const accountId = await accountWithPush(store, 'noauth@x', 'tok-noauth')
    await expect(sink.deliver(accountId, { type: 1, root: 'r', reason: 'new-events' })).rejects.toThrow(
      /token endpoint/
    )
  })

  test('JWT-assertion підписаний і несе скоуп FCM', async () => {
    const { sink } = setup(OK_ROUTE)
    const [header, claims, signature] = sink.accessToken.assertion().split('.')
    expect(JSON.parse(Buffer.from(header, 'base64url').toString())).toEqual({ alg: 'RS256', typ: 'JWT' })
    expect(JSON.parse(Buffer.from(claims, 'base64url').toString())).toMatchObject({
      iss: 'svc@proj.iam',
      scope: 'https://www.googleapis.com/auth/firebase.messaging',
      aud: 'https://oauth2.googleapis.com/token'
    })
    expect(signature.length).toBeGreaterThan(100)
  })
})
