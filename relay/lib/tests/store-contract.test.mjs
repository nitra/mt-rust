/**
 * Контракт store-інтерфейсу relay: один набір перевірок, який мусять
 * проходити ВСІ реалізації (access.md, «Схема даних»).
 *
 * Навіщо саме так: реалізацій дві — in-memory (dev, без персистентності) і
 * SQLite (персистентна). Взаємозамінність не доводиться тим, що обидві
 * «мають ті самі методи» — вона доводиться однаковою ПОВЕДІНКОЮ, тому
 * набір параметризований фабрикою.
 *
 * Обидві прогоняються завжди: SQLite бере `:memory:`-базу, тож перевірка
 * не потребує ні інфраструктури, ні змінних середовища.
 */
import { Buffer } from 'node:buffer'
import { randomUUID } from 'node:crypto'
import { rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

import { afterAll, describe, expect, test } from 'vitest'

import { createSqliteStore } from '../sqlite-store.mjs'
import { InMemoryStore } from '../store.mjs'

/** Детермінований hex-pubkey (32 байти) з імені. */
function key(name) {
  return Buffer.from(name, 'utf8').toString('hex').padEnd(64, '0').slice(0, 64)
}

/**
 * Набір перевірок контракту.
 * @param {string} label назва реалізації у виводі
 * @param {() => Promise<object>} makeStore фабрика чистого store
 * @returns {void}
 */
function storeContract(label, makeStore) {
  describe(`store-контракт: ${label}`, () => {
    test('акаунт створюється і знаходиться за email та id', async () => {
      const store = await makeStore()
      const created = await store.createAccount({ email: `a-${Date.now()}@x`, displayName: 'A' })
      expect(created.account_id).toBeTruthy()
      expect(await store.accountByEmail(created.email)).toMatchObject({
        account_id: created.account_id
      })
      expect(await store.accountById(created.account_id)).toMatchObject({ email: created.email })
      expect(await store.accountByEmail('nobody@x')).toBeNull()
    })

    test('createAccount ідемпотентний за email', async () => {
      // Дірка, яку відкрив auth-контракт: у схемі `email` UNIQUE, але
      // in-memory реалізація плодила другий акаунт — і `account_id` «плив»
      // між викликами `verifySession`, тобто учасник задачі після
      // перелогіну ставав іншою людиною.
      const store = await makeStore()
      const email = `dup-${Date.now()}@x`
      const first = await store.createAccount({ email, displayName: 'A' })
      const second = await store.createAccount({ email, displayName: 'B' })
      expect(second.account_id).toBe(first.account_id)
    })

    test('пристрій: токен авторизує, невалідний pubkey відхиляється одразу', async () => {
      const store = await makeStore()
      const account = await store.createAccount({ email: `d-${Date.now()}@x` })
      const { device_token, device_id } = await store.registerDevice(account.account_id, {
        name: 'mac',
        role: 'host',
        pubkey: key('mac')
      })
      const device = await store.deviceByToken(device_token)
      expect(device).toMatchObject({ device_id, account_id: account.account_id })
      expect(await store.deviceByToken('не-токен')).toBeNull()

      await expect(
        store.registerDevice(account.account_id, { name: 'bad', role: 'client', pubkey: 'pk' })
      ).rejects.toThrow(/hex/)
    })

    test('push-токен пристрою: записується, знімається, не тече між акаунтами', async () => {
      const store = await makeStore()
      const one = await store.createAccount({ email: `p1-${Date.now()}@x` })
      const two = await store.createAccount({ email: `p2-${Date.now()}@x` })
      const device = await store.registerDevice(one.account_id, { name: 'phone', role: 'client', pubkey: key('p') })
      await store.registerDevice(two.account_id, { name: 'other', role: 'client', pubkey: key('o') })

      // Пристрій без токена не потрапляє у вибірку — доставляти нема куди.
      expect(await store.pushTokensFor(one.account_id)).toEqual([])

      await store.setPushToken(device.device_id, 'fcm-token')
      expect(await store.pushTokensFor(one.account_id)).toEqual([
        { device_id: device.device_id, push_token: 'fcm-token' }
      ])
      expect(await store.pushTokensFor(two.account_id)).toEqual([])

      // Порожній рядок — зняти (протухлий токен після 404 від FCM).
      await store.setPushToken(device.device_id, '')
      expect(await store.pushTokensFor(one.account_id)).toEqual([])
    })

    test('touchDevice переживає перечитування пристрою', async () => {
      // Розходження, знайдене на presence: `last_seen` писався присвоєнням
      // у обʼєкт, повернутий з `deviceByToken`. В in-memory це той самий
      // запис, у SQLite — відірваний рядок, тож запис губився мовчки.
      const store = await makeStore()
      const account = await store.createAccount({ email: `t-${Date.now()}@x` })
      const { device_id, device_token } = await store.registerDevice(account.account_id, {
        name: 'mac',
        role: 'host',
        pubkey: key('touch')
      })
      expect((await store.deviceByToken(device_token)).last_seen).toBeFalsy()

      await store.touchDevice(device_id, '2026-08-12T10:00:00Z')
      expect((await store.deviceByToken(device_token)).last_seen).toBe('2026-08-12T10:00:00Z')
    })

    test('ротація: retired лишається в історії, але зникає з pubkeys', async () => {
      // Історію ключів тримаємо, бо історичні підписи в git лишаються
      // валідним фактом; з обігу ключ має зникнути негайно.
      const store = await makeStore()
      const account = await store.createAccount({ email: `rot-${Date.now()}@x` })
      const root = `root-${Date.now()}-rot`
      await store.createTask(root, account.account_id)
      const old = await store.registerDevice(account.account_id, {
        name: 'mac',
        role: 'host',
        pubkey: key('old')
      })
      const fresh = await store.registerDevice(account.account_id, {
        name: 'mac',
        role: 'host',
        pubkey: key('new')
      })
      expect((await store.pubkeysFor(root)).map(k => k.device_id).toSorted()).toEqual(
        [old.device_id, fresh.device_id].toSorted()
      )

      await store.retireDevice(old.device_id, '2026-08-14T00:00:00Z')
      expect((await store.pubkeysFor(root)).map(k => k.device_id)).toEqual([fresh.device_id])
      // …але в історії акаунта він лишається, з міткою часу.
      const history = await store.devicesOf(account.account_id)
      expect(history.find(d => d.device_id === old.device_id).retired_at).toBe('2026-08-14T00:00:00Z')
    })

    test('revocation: пристрій зникає і з pubkeys, і з історії', async () => {
      const store = await makeStore()
      const account = await store.createAccount({ email: `rev-${Date.now()}@x` })
      const root = `root-${Date.now()}-rev`
      await store.createTask(root, account.account_id)
      const device = await store.registerDevice(account.account_id, {
        name: 'втрачений',
        role: 'client',
        pubkey: key('lost')
      })

      await store.deleteDevice(device.device_id)
      expect(await store.pubkeysFor(root)).toEqual([])
      expect(await store.devicesOf(account.account_id)).toEqual([])
      expect(await store.deviceByToken(device.device_token)).toBeNull()
    })

    test('createTask робить власника owner автоматично', async () => {
      const store = await makeStore()
      const owner = await store.createAccount({ email: `o-${Date.now()}@x` })
      const root = `root-${Date.now()}-a`
      await store.createTask(root, owner.account_id)
      expect(await store.memberRole(root, owner.account_id)).toBe('owner')
      expect(await store.membersOf(root)).toEqual([{ account_id: owner.account_id, role: 'owner' }])
    })

    test('setMemberRole — upsert: додає нового і змінює наявного', async () => {
      // Саме ця семантика тримає accept-запрошення: update-only тихо
      // ламав би додавання учасника.
      const store = await makeStore()
      const owner = await store.createAccount({ email: `o2-${Date.now()}@x` })
      const guest = await store.createAccount({ email: `g-${Date.now()}@x` })
      const root = `root-${Date.now()}-b`
      await store.createTask(root, owner.account_id)

      await store.setMemberRole(root, guest.account_id, 'viewer')
      expect(await store.memberRole(root, guest.account_id)).toBe('viewer')
      await store.setMemberRole(root, guest.account_id, 'approver')
      expect(await store.memberRole(root, guest.account_id)).toBe('approver')

      await store.removeMember(root, guest.account_id)
      expect(await store.memberRole(root, guest.account_id)).toBeNull()
    })

    test('запрошення: pending знаходиться, статус змінюється', async () => {
      const store = await makeStore()
      const owner = await store.createAccount({ email: `o3-${Date.now()}@x` })
      const root = `root-${Date.now()}-c`
      await store.createTask(root, owner.account_id)

      const invitation = await store.createInvitation(root, owner.account_id, 'new@x', 'host')
      expect(invitation.status).toBe('pending')
      expect(await store.pendingInvitationFor(root, 'new@x')).toMatchObject({
        invitation_id: invitation.invitation_id
      })

      await store.setInvitationStatus(invitation.invitation_id, 'accepted')
      expect((await store.invitationById(invitation.invitation_id)).status).toBe('accepted')
      // Після accept відкритого запрошення для цього email більше немає —
      // на цьому тримається ідемпотентність bootstrap-у.
      expect(await store.pendingInvitationFor(root, 'new@x')).toBeNull()
    })

    test('pubkeysFor віддає лише пристрої approver+', async () => {
      const store = await makeStore()
      const owner = await store.createAccount({ email: `o4-${Date.now()}@x` })
      const viewer = await store.createAccount({ email: `v-${Date.now()}@x` })
      const root = `root-${Date.now()}-d`
      await store.createTask(root, owner.account_id)
      await store.setMemberRole(root, viewer.account_id, 'viewer')

      await store.registerDevice(owner.account_id, {
        name: 'owner-mac',
        role: 'host',
        pubkey: key('owner-mac')
      })
      await store.registerDevice(viewer.account_id, {
        name: 'viewer-tab',
        role: 'client',
        pubkey: key('viewer-tab')
      })

      const pubkeys = await store.pubkeysFor(root)
      expect(pubkeys).toHaveLength(1)
      expect(pubkeys[0]).toMatchObject({ account_id: owner.account_id })
    })
  })
}

storeContract('in-memory', async () => new InMemoryStore())

const sqliteStores = []
storeContract('sqlite', async () => {
  // Окрема :memory:-база на кожен тест — ізоляція без файлів на диску.
  const store = await createSqliteStore(':memory:')
  sqliteStores.push(store)
  return store
})

afterAll(() => {
  for (const store of sqliteStores) store.close()
})

describe('sqlite: персистентність', () => {
  test('дані переживають перевідкриття бази', async () => {
    // Те, заради чого все й робилось: рестарт relay не має зносити
    // акаунти й membership.
    const file = join(tmpdir(), `relay-persist-${randomUUID()}.sqlite`)
    try {
      const first = await createSqliteStore(file)
      const owner = await first.createAccount({ email: 'owner@persist' })
      await first.createTask('root-persist', owner.account_id)
      await first.setMemberRole('root-persist', owner.account_id, 'owner')
      const { device_token } = await first.registerDevice(owner.account_id, {
        name: 'mac',
        role: 'host',
        pubkey: key('mac')
      })
      first.close()

      const reopened = await createSqliteStore(file)
      expect(await reopened.accountByEmail('owner@persist')).toMatchObject({
        account_id: owner.account_id
      })
      expect(await reopened.memberRole('root-persist', owner.account_id)).toBe('owner')
      expect(await reopened.deviceByToken(device_token)).toMatchObject({
        account_id: owner.account_id
      })
      reopened.close()
    } finally {
      rmSync(file, { force: true })
      rmSync(`${file}-wal`, { force: true })
      rmSync(`${file}-shm`, { force: true })
    }
  })
})

describe('sqlite: міграція існуючої бази', () => {
  test('колонка, додана після створення бази, добудовується', async () => {
    // `CREATE TABLE IF NOT EXISTS` наздоганяє лише нові таблиці. Відколи
    // store персистентний, база на диску переживає релізи — і без явного
    // ALTER запити падали б на «no such column».
    const { Database } = await import('bun:sqlite')
    const file = join(tmpdir(), `relay-migrate-${randomUUID()}.sqlite`)
    try {
      // База «попередньої версії»: devices без push_token.
      const legacy = new Database(file, { create: true })
      legacy.exec(`
        CREATE TABLE accounts (account_id TEXT PRIMARY KEY, email TEXT NOT NULL UNIQUE, display_name TEXT NOT NULL DEFAULT '');
        CREATE TABLE devices (
          device_id TEXT PRIMARY KEY,
          account_id TEXT NOT NULL REFERENCES accounts(account_id) ON DELETE CASCADE,
          name TEXT NOT NULL DEFAULT '', role TEXT NOT NULL, pubkey TEXT NOT NULL,
          device_token TEXT NOT NULL UNIQUE, last_seen TEXT
        );
      `)
      legacy.query('INSERT INTO accounts (account_id, email) VALUES (?, ?)').run('acc-1', 'legacy@x')
      legacy
        .query('INSERT INTO devices (device_id, account_id, role, pubkey, device_token) VALUES (?, ?, ?, ?, ?)')
        .run('dev-1', 'acc-1', 'client', key('legacy'), 'tok-1')
      legacy.close()

      const store = await createSqliteStore(file)
      // Наявні дані на місці, нова колонка працює.
      expect(await store.accountByEmail('legacy@x')).toMatchObject({ account_id: 'acc-1' })
      expect(await store.pushTokensFor('acc-1')).toEqual([])
      await store.setPushToken('dev-1', 'fcm-1')
      expect(await store.pushTokensFor('acc-1')).toEqual([{ device_id: 'dev-1', push_token: 'fcm-1' }])
      store.close()
    } finally {
      rmSync(file, { force: true })
      rmSync(`${file}-wal`, { force: true })
      rmSync(`${file}-shm`, { force: true })
    }
  })
})
