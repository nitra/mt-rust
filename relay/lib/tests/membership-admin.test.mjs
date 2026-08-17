/**
 * Адміністрування учасників і пристроїв (access.md): `PATCH role` /
 * `DELETE` у Membership API та життєвий цикл ключів (rotation, revocation).
 */
import { Buffer } from 'node:buffer'

import { beforeEach, describe, expect, test } from 'vitest'

import { RelayCore } from '../relay.mjs'
import { InMemoryStore } from '../store.mjs'

/** Детермінований hex-pubkey (32 байти) з імені. */
function key(name) {
  return Buffer.from(name, 'utf8').toString('hex').padEnd(64, '0').slice(0, 64)
}

/** @type {InMemoryStore} */
let store
/** @type {RelayCore} */
let core
/** @type {Record<string, object>} */
let accounts
/** @type {object} */
let ownerDevice

beforeEach(async () => {
  store = new InMemoryStore()
  core = new RelayCore({ store })
  accounts = {
    owner: await store.createAccount({ email: 'owner@x' }),
    host: await store.createAccount({ email: 'host@x' }),
    outsider: await store.createAccount({ email: 'outsider@x' })
  }
  await store.createTask('root-1', accounts.owner.account_id)
  await store.setMemberRole('root-1', accounts.host.account_id, 'host')
  const registered = await store.registerDevice(accounts.owner.account_id, {
    name: 'mac',
    role: 'host',
    pubkey: key('mac')
  })
  ownerDevice = await store.deviceByToken(registered.device_token)
})

describe('PATCH role', () => {
  test('owner міняє роль; у кімнату йде MemberChanged', async () => {
    const inbox = []
    await core.subscribe(ownerDevice, 'root-1', frame => void inbox.push(frame))

    await core.changeMemberRole(accounts.owner.account_id, 'root-1', accounts.host.account_id, 'approver')
    expect(await store.memberRole('root-1', accounts.host.account_id)).toBe('approver')
    expect(inbox.at(-1)).toMatchObject({
      kind: 'event',
      event: { type: 'MemberChanged', account_id: accounts.host.account_id, role: 'approver' }
    })
  })

  test('не-owner не міняє ролей', async () => {
    await expect(
      core.changeMemberRole(accounts.host.account_id, 'root-1', accounts.host.account_id, 'owner')
    ).rejects.toThrow(/лише owner/)
  })

  test('невідома роль відхиляється', async () => {
    await expect(
      core.changeMemberRole(accounts.owner.account_id, 'root-1', accounts.host.account_id, 'бос')
    ).rejects.toThrow(/невідома роль/)
  })

  test('не-учасника не можна перевести в роль', async () => {
    await expect(
      core.changeMemberRole(accounts.owner.account_id, 'root-1', accounts.outsider.account_id, 'host')
    ).rejects.toThrow(/не учасник/)
  })

  test('останнього owner-а не понизити', async () => {
    // Задача без власника закривається адміністративною процедурою
    // оператора relay, а не звичайним API — дешевше не дати створити
    // цей стан, ніж потім із нього виходити.
    await expect(
      core.changeMemberRole(accounts.owner.account_id, 'root-1', accounts.owner.account_id, 'host')
    ).rejects.toThrow(/останній owner/)

    // З другим owner-ом пониження вже дозволене.
    await store.setMemberRole('root-1', accounts.host.account_id, 'owner')
    await core.changeMemberRole(accounts.owner.account_id, 'root-1', accounts.owner.account_id, 'host')
    expect(await store.memberRole('root-1', accounts.owner.account_id)).toBe('host')
  })
})

describe('DELETE учасника', () => {
  test('owner прибирає учасника; MemberChanged несе role: null', async () => {
    // `role: null` — саме те, що протокол задає для видалення.
    const inbox = []
    await core.subscribe(ownerDevice, 'root-1', frame => void inbox.push(frame))

    await core.removeMember(accounts.owner.account_id, 'root-1', accounts.host.account_id)
    expect(await store.memberRole('root-1', accounts.host.account_id)).toBeNull()
    expect(inbox.at(-1)).toMatchObject({
      kind: 'event',
      event: { type: 'MemberChanged', account_id: accounts.host.account_id, role: null }
    })
  })

  test('останнього owner-а не прибрати', async () => {
    await expect(core.removeMember(accounts.owner.account_id, 'root-1', accounts.owner.account_id)).rejects.toThrow(
      /останній owner/
    )
  })

  test('не-owner не прибирає', async () => {
    await expect(core.removeMember(accounts.host.account_id, 'root-1', accounts.owner.account_id)).rejects.toThrow(
      /лише owner/
    )
  })
})

describe('життєвий цикл ключів', () => {
  test('ротація: новий ключ у роздачі, старий — в історії', async () => {
    const rotated = await core.rotateDevice(ownerDevice, ownerDevice.device_id, {
      pubkey: key('mac-v2')
    })
    const pubkeys = await core.pubkeys(ownerDevice, 'root-1')
    expect(pubkeys.map(k => k.device_id)).toEqual([rotated.device_id])

    const history = await store.devicesOf(accounts.owner.account_id)
    expect(history).toHaveLength(2)
    expect(history.find(d => d.device_id === ownerDevice.device_id).retired_at).toBeTruthy()
  })

  test('ротація зберігає імʼя і роль, якщо їх не задали', async () => {
    const rotated = await core.rotateDevice(ownerDevice, ownerDevice.device_id, {
      pubkey: key('mac-v2')
    })
    const fresh = (await store.devicesOf(accounts.owner.account_id)).find(d => d.device_id === rotated.device_id)
    expect(fresh).toMatchObject({ name: 'mac', role: 'host' })
  })

  test('revocation прибирає пристрій із роздачі', async () => {
    await core.revokeDevice(ownerDevice, ownerDevice.device_id)
    expect(await store.devicesOf(accounts.owner.account_id)).toEqual([])
  })

  test('чужий пристрій не ротується і не відкликається', async () => {
    // Чужий акаунт не має підстав вирішувати за власника ключа.
    const other = await store.registerDevice(accounts.host.account_id, {
      name: 'чужий',
      role: 'client',
      pubkey: key('other')
    })
    await expect(core.rotateDevice(ownerDevice, other.device_id, { pubkey: key('x') })).rejects.toThrow(
      /не цього акаунта/
    )
    await expect(core.revokeDevice(ownerDevice, other.device_id)).rejects.toThrow(/не цього акаунта/)
  })

  test('невалідний pubkey відхиляється до будь-яких змін', async () => {
    // Інакше старий ключ уже був би retired, а нового не зʼявилось —
    // акаунт лишився б без жодного дійсного ключа.
    await expect(core.rotateDevice(ownerDevice, ownerDevice.device_id, { pubkey: 'не-ключ' })).rejects.toThrow(/hex/)
    const history = await store.devicesOf(accounts.owner.account_id)
    expect(history).toHaveLength(1)
    expect(history[0].retired_at).toBeNull()
  })
})
