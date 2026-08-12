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
