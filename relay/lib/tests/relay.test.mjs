import { Buffer } from 'node:buffer'
import { generateKeyPairSync, sign } from 'node:crypto'

import { beforeEach, describe, expect, test } from 'vitest'

import { DevPushSink } from '../push-sink.mjs'
import { PushRouter } from '../push.mjs'
import { RelayCore } from '../relay.mjs'
import { Rooms } from '../rooms.mjs'
import { transferMessage } from '../signing.mjs'
import { InMemoryStore, roleAtLeast } from '../store.mjs'

const RE_NOT_MEMBER = /не учасник/
const RE_VIEWER = /viewer/
const RE_OWNER_ONLY = /owner/
const RE_FOREIGN_ACCOUNT = /іншому акаунту/
const RE_ALREADY_PROCESSED = /оброблене/
const RE_HEX = /hex/
const RE_SIGNATURE = /підпис/

/** @type {InMemoryStore} */
let store
/** @type {RelayCore} */
let core
/** @type {Record<string, object>} акаунти фікстури */
let accounts
/** @type {Record<string, object>} пристрої фікстури (повні записи) */
let devices

/**
 * Детермінований hex-pubkey (32 байти) з імені — задовольняє валідацію
 * формату там, де криптографія тесту не потрібна.
 * @param {string} name імʼя пристрою
 * @returns {string} hex-рядок 64 символи
 */
function fakeKey(name) {
  return Buffer.from(name, 'utf8').toString('hex').padEnd(64, '0').slice(0, 64)
}

/**
 * Реєструє пристрій і повертає повний запис (для викликів ядра).
 * @param {string} accountId акаунт-власник
 * @param {string} name імʼя пристрою
 * @returns {object} запис пристрою
 */
async function device(accountId, name) {
  const { device_token } = await store.registerDevice(accountId, {
    name,
    role: 'client',
    pubkey: fakeKey(name)
  })
  return await store.deviceByToken(device_token)
}

/**
 * Підписка з накопиченням кадрів у масив.
 * @param {object[]} inbox приймач кадрів
 * @returns {(frame: object) => void} колбек доставки
 */
function collectInto(inbox) {
  return frame => {
    inbox.push(frame)
  }
}

beforeEach(async () => {
  store = new InMemoryStore()
  core = new RelayCore({ store })
  accounts = {
    owner: await store.createAccount({ email: 'owner@x' }),
    approver: await store.createAccount({ email: 'approver@x' }),
    viewer: await store.createAccount({ email: 'viewer@x' }),
    outsider: await store.createAccount({ email: 'outsider@x' })
  }
  await store.createTask('root-1', accounts.owner.account_id)
  await store.setMemberRole('root-1', accounts.approver.account_id, 'approver')
  await store.setMemberRole('root-1', accounts.viewer.account_id, 'viewer')
  await store.createTask('root-2', accounts.outsider.account_id)
  devices = {
    owner: await device(accounts.owner.account_id, 'mac-owner'),
    approver: await device(accounts.approver.account_id, 'phone-approver'),
    viewer: await device(accounts.viewer.account_id, 'tab-viewer'),
    outsider: await device(accounts.outsider.account_id, 'pc-outsider')
  }
})

describe('membership-роутінг кімнат', () => {
  test('підписка лише учасникам кореня; конверт доходить лише у свою кімнату', async () => {
    const inbox1 = []
    const inbox2 = []
    await core.subscribe(devices.viewer, 'root-1', collectInto(inbox1))
    await core.subscribe(devices.outsider, 'root-2', collectInto(inbox2))

    await core.clientEnvelope(devices.owner, 'root-1', { seq: 0, node_hash: 'root-1' })

    expect(inbox1).toHaveLength(1)
    expect(inbox2).toHaveLength(0)
    await expect(core.subscribe(devices.outsider, 'root-1', collectInto([]))).rejects.toThrow(RE_NOT_MEMBER)
  })

  test('viewer не шле клієнтські події; approver шле (ApprovalResponse)', async () => {
    await expect(core.clientEnvelope(devices.viewer, 'root-1', { seq: 0 })).rejects.toThrow(RE_VIEWER)
    await expect(core.clientEnvelope(devices.approver, 'root-1', { seq: 0 })).resolves.not.toThrow()
    await expect(core.clientEnvelope(devices.outsider, 'root-1', { seq: 0 })).rejects.toThrow(RE_NOT_MEMBER)
  })
})

describe('membership API', () => {
  test('invite (лише owner) → accept → запис у members + broadcast MemberChanged', async () => {
    const inbox = []
    await core.subscribe(devices.owner, 'root-1', collectInto(inbox))
    const invited = await store.createAccount({ email: 'new@x' })

    await expect(core.invite(accounts.viewer.account_id, 'root-1', { email: 'new@x', role: 'host' })).rejects.toThrow(
      RE_OWNER_ONLY
    )

    const invitation = await core.invite(accounts.owner.account_id, 'root-1', {
      email: 'new@x',
      role: 'host'
    })
    // Чужий акаунт не може прийняти.
    await expect(core.accept(invitation.invitation_id, accounts.viewer.account_id)).rejects.toThrow(RE_FOREIGN_ACCOUNT)
    const membership = await core.accept(invitation.invitation_id, invited.account_id)

    expect(membership).toEqual({ root_node_hash: 'root-1', role: 'host' })
    expect(await store.memberRole('root-1', invited.account_id)).toBe('host')
    expect(inbox.at(-1)).toEqual({
      kind: 'event',
      event: { type: 'MemberChanged', account_id: invited.account_id, role: 'host' }
    })
    // Повторний accept — відмова (не pending).
    await expect(core.accept(invitation.invitation_id, invited.account_id)).rejects.toThrow(RE_ALREADY_PROCESSED)
  })

  test('transfer ownership: новий owner, попередній стає host', async () => {
    await core.transferOwnership('root-1', accounts.owner.account_id, accounts.approver.account_id)
    expect(await store.memberRole('root-1', accounts.approver.account_id)).toBe('owner')
    expect(await store.memberRole('root-1', accounts.owner.account_id)).toBe('host')
    // Колишній owner більше не передає.
    await expect(
      core.transferOwnership('root-1', accounts.owner.account_id, accounts.viewer.account_id)
    ).rejects.toThrow(RE_OWNER_ONLY)
  })
})

describe('буфер кімнати', () => {
  test('обрізається до ліміту; підписка реплеїть хвіст', async () => {
    const rooms = new Rooms(3)
    const smallCore = new RelayCore({ store, rooms })
    for (let i = 0; i < 5; i++) {
      await smallCore.clientEnvelope(devices.owner, 'root-1', { seq: i })
    }
    const inbox = []
    await smallCore.subscribe(devices.owner, 'root-1', collectInto(inbox))
    expect(inbox.map(f => f.envelope.seq)).toEqual([2, 3, 4])
  })
})

describe('from_host', () => {
  test('ставиться relay-єм за роллю пристрою, не з кадру клієнта', async () => {
    const hostDevice = await store.deviceByToken(
      (
        await store.registerDevice(accounts.owner.account_id, {
          name: 'host-mac',
          role: 'host',
          pubkey: fakeKey('host-mac')
        })
      ).device_token
    )
    const inbox = []
    await core.subscribe(devices.viewer, 'root-1', collectInto(inbox))

    await core.clientEnvelope(hostDevice, 'root-1', { seq: 1 })
    await core.clientEnvelope(devices.approver, 'root-1', { seq: 0 })

    expect(inbox.map(f => f.from_host)).toEqual([true, false])
  })
})

describe('pubkeys', () => {
  test('лише пристрої approver+; доступ лише учасникам', async () => {
    const pubkeys = await core.pubkeys(devices.viewer, 'root-1')
    expect(pubkeys.map(k => k.pubkey).toSorted()).toEqual([fakeKey('mac-owner'), fakeKey('phone-approver')].toSorted())
    await expect(core.pubkeys(devices.outsider, 'root-1')).rejects.toThrow(RE_NOT_MEMBER)
  })

  test('registerDevice відхиляє pubkey не у hex-32 форматі', async () => {
    await expect(
      store.registerDevice(accounts.owner.account_id, { name: 'bad', role: 'client', pubkey: 'pk-bad' })
    ).rejects.toThrow(RE_HEX)
  })
})

describe('підписаний transfer ownership', () => {
  /**
   * Реальна Ed25519-пара: пристрій із цим pubkey і функція підпису.
   * @param {string} accountId акаунт-власник пристрою
   * @returns {{ device: object, signTransfer: (payload: object) => string }} пристрій і підписувач
   */
  async function signingDevice(accountId) {
    const { publicKey, privateKey } = generateKeyPairSync('ed25519')
    const raw = publicKey.export({ format: 'der', type: 'spki' }).subarray(-32)
    const { device_token } = await store.registerDevice(accountId, {
      name: 'signer',
      role: 'client',
      pubkey: raw.toString('hex')
    })
    return {
      device: await store.deviceByToken(device_token),
      signTransfer: payload => sign(null, transferMessage(payload), privateKey).toBase64()
    }
  }

  test('валідний підпис акта проходить, зіпсований — відмова без зміни ролей', async () => {
    const { device: signer, signTransfer } = await signingDevice(accounts.owner.account_id)
    const payload = {
      root: 'root-1',
      fromAccount: accounts.owner.account_id,
      toAccount: accounts.approver.account_id
    }
    const good = signTransfer(payload)
    const bad = signTransfer({ ...payload, toAccount: accounts.viewer.account_id })

    await expect(
      core.transferOwnership('root-1', accounts.owner.account_id, accounts.approver.account_id, {
        device: signer,
        signature: bad
      })
    ).rejects.toThrow(RE_SIGNATURE)
    expect(await store.memberRole('root-1', accounts.owner.account_id)).toBe('owner')

    await core.transferOwnership('root-1', accounts.owner.account_id, accounts.approver.account_id, {
      device: signer,
      signature: good
    })
    expect(await store.memberRole('root-1', accounts.approver.account_id)).toBe('owner')
    expect(await store.memberRole('root-1', accounts.owner.account_id)).toBe('host')
  })
})

describe('push', () => {
  /** @type {DevPushSink} */
  let sink
  /** @type {RelayCore} */
  let pushCore

  beforeEach(async () => {
    sink = new DevPushSink()
    pushCore = new RelayCore({ store })
    pushCore.push = new PushRouter({ store, sink, rooms: pushCore.rooms })
  })

  test('invite: тип 2 зареєстрованому акаунту; незареєстрований email — тихо', async () => {
    await pushCore.invite(accounts.owner.account_id, 'root-1', { email: 'viewer@x', role: 'host' })
    await pushCore.invite(accounts.owner.account_id, 'root-1', { email: 'ghost@x', role: 'host' })
    expect(sink.deliveries).toEqual([
      { account_id: accounts.viewer.account_id, type: 2, root: 'root-1', reason: 'invited', ref: null }
    ])
  })

  test('attention-подія — тип 3 учасникам, крім автора; звичайна подія — тип 1', async () => {
    await pushCore.clientEnvelope(devices.owner, 'root-1', {
      seq: 0,
      event: { type: 'PlanReview', plan_ref: 'plan_001' }
    })
    const attention = sink.deliveries.filter(d => d.type === 3)
    expect(attention.map(d => d.account_id).toSorted()).toEqual(
      [accounts.approver.account_id, accounts.viewer.account_id].toSorted()
    )
    expect(attention.every(d => d.reason === 'PlanReview' && d.ref === 'plan_001')).toBe(true)

    sink.deliveries.length = 0
    await pushCore.clientEnvelope(devices.owner, 'root-1', { seq: 1, event: { type: 'NodeState', state: 'running' } })
    expect(sink.deliveries.every(d => d.type === 1 && d.reason === 'new-events')).toBe(true)
    expect(sink.deliveries.map(d => d.account_id).toSorted()).toEqual(
      [accounts.approver.account_id, accounts.viewer.account_id].toSorted()
    )
  })

  test('тип 1 не будить того, хто підписаний на кімнату', async () => {
    // Push існує, щоб розбудити НЕпідключене: пристрою з живою підпискою
    // подія вже прийшла кадром, і push поверх неї — чистий шум.
    await pushCore.subscribe(devices.viewer, 'root-1', () => {})
    await pushCore.clientEnvelope(devices.owner, 'root-1', { seq: 4, event: { type: 'NodeState', state: 'running' } })
    expect(sink.deliveries.map(d => d.account_id)).toEqual([accounts.approver.account_id])
  })

  test('тип 1 дедуплікується вікном: серія подій — один push', async () => {
    let clock = 0
    const throttled = new RelayCore({
      store,
      push: new PushRouter({ store, sink, wakeCooldownMs: 1000, now: () => clock })
    })
    for (let seq = 0; seq < 5; seq += 1) {
      await throttled.clientEnvelope(devices.owner, 'root-1', { seq, event: { type: 'NodeState', state: 'running' } })
      clock += 100
    }
    expect(sink.deliveries.filter(d => d.account_id === accounts.viewer.account_id)).toHaveLength(1)

    // Поза вікном — знову можна.
    clock += 1000
    await throttled.clientEnvelope(devices.owner, 'root-1', { seq: 9, event: { type: 'NodeState', state: 'running' } })
    expect(sink.deliveries.filter(d => d.account_id === accounts.viewer.account_id)).toHaveLength(2)
  })

  test('pending: тип 3 лише для h.md-assignee з notify, інакше нікого не будить', async () => {
    // Без notify кожен вузол, що став до черги, будив би всю кімнату.
    await pushCore.clientEnvelope(devices.owner, 'root-1', {
      seq: 5,
      event: {
        type: 'NodeState',
        state: 'pending',
        notify: true,
        to_account_id: accounts.approver.account_id,
        fact_ref: 'node-7'
      }
    })
    expect(sink.deliveries).toEqual([
      {
        account_id: accounts.approver.account_id,
        type: 3,
        root: 'root-1',
        reason: 'NodeState',
        ref: 'node-7'
      }
    ])

    sink.deliveries.length = 0
    await pushCore.clientEnvelope(devices.owner, 'root-1', {
      seq: 6,
      event: { type: 'NodeState', state: 'pending', to_account_id: accounts.approver.account_id }
    })
    expect(sink.deliveries.filter(d => d.type === 3)).toEqual([])
  })

  test('unresolvable — тип 3 усім, крім автора', async () => {
    await pushCore.clientEnvelope(devices.owner, 'root-1', {
      seq: 7,
      event: { type: 'NodeState', state: 'unresolvable', reason_ref: 'unresolvable.md' }
    })
    expect(sink.deliveries.map(d => d.type)).toEqual([3, 3])
  })

  test('Escalation: адресний push лише to_account_id; без резолву — нікому', async () => {
    await pushCore.clientEnvelope(devices.owner, 'root-1', {
      seq: 2,
      event: {
        type: 'Escalation',
        from: 'olena',
        to: 'vkozlov',
        to_account_id: accounts.approver.account_id,
        reason_ref: 'escalation_001.md'
      }
    })
    await pushCore.clientEnvelope(devices.owner, 'root-1', {
      seq: 3,
      event: { type: 'Escalation', from: 'olena', to: 'petro', reason_ref: 'escalation_002.md' }
    })
    expect(sink.deliveries).toEqual([
      {
        account_id: accounts.approver.account_id,
        type: 3,
        root: 'root-1',
        reason: 'Escalation',
        ref: 'escalation_001.md'
      }
    ])
  })
})

describe('bootstrapMembers', () => {
  test('owner-gated; зареєстровані стають учасниками, решта — pending; ідемпотентно', async () => {
    const registered = await store.createAccount({ email: 'olena@x' })
    const entries = [
      { email: 'olena@x', role: 'owner' },
      { email: 'viewer@x', role: 'owner' }, // вже учасник — роль не чіпаємо
      { email: 'ghost@x' }
    ]

    await expect(core.bootstrapMembers(accounts.viewer.account_id, 'root-1', entries)).rejects.toThrow(RE_OWNER_ONLY)

    const first = await core.bootstrapMembers(accounts.owner.account_id, 'root-1', entries)
    expect(first).toEqual({ added: ['olena@x'], invited: ['ghost@x'], kept: ['viewer@x'] })
    expect(await store.memberRole('root-1', registered.account_id)).toBe('owner')
    expect(await store.memberRole('root-1', accounts.viewer.account_id)).toBe('viewer')
    expect(await store.pendingInvitationFor('root-1', 'ghost@x')).not.toBeNull()

    // Повторний прогін: без нових запрошень і без зміни ролей.
    const again = await core.bootstrapMembers(accounts.owner.account_id, 'root-1', entries)
    expect(again).toEqual({ added: [], invited: ['ghost@x'], kept: ['olena@x', 'viewer@x'] })
    const pending = store.invitations
      .values()
      .filter(i => i.to_email === 'ghost@x')
      .toArray()
    expect(pending).toHaveLength(1)
  })
})

describe('roleAtLeast', () => {
  test('ієрархія owner ⊃ host ⊃ approver ⊃ viewer', async () => {
    expect(roleAtLeast('owner', 'viewer')).toBe(true)
    expect(roleAtLeast('viewer', 'approver')).toBe(false)
    expect(roleAtLeast(null, 'viewer')).toBe(false)
  })
})
