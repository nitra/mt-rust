/**
 * Presence relay (access.md: «presence (хости: hostname, проєкти, активні
 * вузли)») — реєстр, гейти ядра і зняття при розриві зʼєднання.
 */
import { Buffer } from 'node:buffer'

import { beforeEach, describe, expect, test } from 'vitest'

import { Presence } from '../presence.mjs'
import { RelayCore } from '../relay.mjs'
import { createSqliteStore } from '../sqlite-store.mjs'
import { InMemoryStore } from '../store.mjs'

/** Детермінований hex-pubkey (32 байти) з імені. */
function key(name) {
  return Buffer.from(name, 'utf8').toString('hex').padEnd(64, '0').slice(0, 64)
}

describe('реєстр Presence', () => {
  test('announce додає запис, forget прибирає', () => {
    const presence = new Presence()
    presence.announce('root-1', {
      deviceId: 'dev-1',
      accountId: 'acc-1',
      hostname: 'mac-vitalii',
      projects: ['mt'],
      nodes: ['mt/demo']
    })
    expect(presence.of('root-1')).toEqual([
      expect.objectContaining({
        device_id: 'dev-1',
        account_id: 'acc-1',
        hostname: 'mac-vitalii',
        projects: ['mt'],
        nodes: ['mt/demo']
      })
    ])

    expect(presence.forget('root-1', 'dev-1')).toBe(true)
    expect(presence.of('root-1')).toEqual([])
    // Повторний forget — не помилка: close приходить і для тих, хто
    // нічого не оголошував.
    expect(presence.forget('root-1', 'dev-1')).toBe(false)
  })

  test('повторне announce оновлює вузли, але зберігає since', () => {
    // Людині важливо «з якого часу тут», а не «коли востаннє надіслав
    // heartbeat» — інакше кожен heartbeat скидав би вік присутності.
    let clock = 1000
    const presence = new Presence({ now: () => clock })
    presence.announce('root-1', { deviceId: 'dev-1', accountId: 'acc-1', nodes: ['a'] })
    clock += 5000
    presence.announce('root-1', { deviceId: 'dev-1', accountId: 'acc-1', nodes: ['b'] })

    const [record] = presence.of('root-1')
    expect(record.since).toBe(1000)
    expect(record.updated_at).toBe(6000)
    expect(record.nodes).toEqual(['b'])
  })

  test('протухлий запис зникає при читанні', () => {
    // Страховка на випадок, коли close не прийшов (half-open зʼєднання):
    // без неї «привиди» висіли б у кімнаті вічно.
    let clock = 0
    const presence = new Presence({ ttlMs: 100, now: () => clock })
    presence.announce('root-1', { deviceId: 'dev-1', accountId: 'acc-1' })
    clock = 100
    expect(presence.of('root-1')).toEqual([])
    expect(presence.rooms.has('root-1')).toBe(false)
  })

  test('кімнати ізольовані', () => {
    const presence = new Presence()
    presence.announce('root-1', { deviceId: 'dev-1', accountId: 'acc-1' })
    presence.announce('root-2', { deviceId: 'dev-2', accountId: 'acc-2' })
    expect(presence.of('root-1').map(r => r.device_id)).toEqual(['dev-1'])
    expect(presence.of('root-2').map(r => r.device_id)).toEqual(['dev-2'])
  })
})

describe('presence у ядрі relay', () => {
  /** @type {InMemoryStore} */
  let store
  /** @type {RelayCore} */
  let core
  /** @type {object} */
  let owner
  /** @type {object} */
  let outsider
  /** @type {object} */
  let ownerDevice
  /** @type {object} */
  let outsiderDevice

  beforeEach(async () => {
    store = new InMemoryStore()
    core = new RelayCore({ store })
    owner = await store.createAccount({ email: 'owner@x' })
    outsider = await store.createAccount({ email: 'outsider@x' })
    await store.createTask('root-1', owner.account_id)
    const registered = await store.registerDevice(owner.account_id, {
      name: 'mac',
      role: 'host',
      pubkey: key('mac')
    })
    ownerDevice = await store.deviceByToken(registered.device_token)
    const other = await store.registerDevice(outsider.account_id, {
      name: 'pc',
      role: 'client',
      pubkey: key('pc')
    })
    outsiderDevice = await store.deviceByToken(other.device_token)
  })

  test('announce транслює PresenceChanged учасникам кімнати', async () => {
    const inbox = []
    await core.subscribe(ownerDevice, 'root-1', frame => inbox.push(frame))
    await core.announcePresence(ownerDevice, 'root-1', {
      hostname: 'mac-vitalii',
      projects: ['mt'],
      nodes: ['mt/demo']
    })

    expect(inbox.at(-1)).toMatchObject({
      kind: 'event',
      event: { type: 'PresenceChanged', device_id: ownerDevice.device_id, hostname: 'mac-vitalii' }
    })
  })

  test('не учасник не оголошує присутності й не читає її', async () => {
    await expect(core.announcePresence(outsiderDevice, 'root-1', {})).rejects.toThrow(/не учасник/)
    await expect(core.presenceOf(outsiderDevice, 'root-1')).rejects.toThrow(/не учасник/)
  })

  test('dropPresence транслює gone і прибирає із переліку', async () => {
    const inbox = []
    await core.announcePresence(ownerDevice, 'root-1', { hostname: 'mac' })
    await core.subscribe(ownerDevice, 'root-1', frame => inbox.push(frame))

    core.dropPresence(ownerDevice, 'root-1')
    expect(inbox.at(-1)).toMatchObject({
      kind: 'event',
      event: { type: 'PresenceChanged', device_id: ownerDevice.device_id, gone: true }
    })
    expect(await core.presenceOf(ownerDevice, 'root-1')).toEqual([])
  })

  test('presence не переживає рестарт relay', async () => {
    // Межа зі спеки: relay — ефемерний координатор. Persist у нього мають
    // акаунти й membership, presence — ні.
    await core.announcePresence(ownerDevice, 'root-1', { hostname: 'mac' })
    expect(await core.presenceOf(ownerDevice, 'root-1')).toHaveLength(1)

    const restarted = new RelayCore({ store })
    expect(await restarted.presenceOf(ownerDevice, 'root-1')).toEqual([])
  })
})

describe('last_seen переживає підключення на персистентному store', () => {
  test('connectDevice пише last_seen у базу, а не в памʼять обʼєкта', async () => {
    // Саме тут ховалось розходження: присвоєння в повернутий обʼєкт
    // працювало лише в in-memory реалізації.
    const store = await createSqliteStore(':memory:')
    try {
      const account = await store.createAccount({ email: 'seen@x' })
      const { device_token, device_id } = await store.registerDevice(account.account_id, {
        name: 'mac',
        role: 'host',
        pubkey: key('seen')
      })
      const core = new RelayCore({ store })
      await core.connectDevice(device_token)

      const reread = await store.deviceByToken(device_token)
      expect(reread.device_id).toBe(device_id)
      expect(reread.last_seen).toBeTruthy()
    } finally {
      store.close()
    }
  })
})
