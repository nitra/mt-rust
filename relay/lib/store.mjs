/**
 * In-memory сховище relay за схемою access.md: accounts, devices, tasks,
 * task_members, invitations.
 *
 * Це dev/тестова реалізація store-інтерфейсу; PostgreSQL-реалізація — окрема
 * задача за тим самим інтерфейсом (stack.md, «Relay-інфраструктура»).
 * Персистентне у relay — ЛИШЕ акаунти/membership/запрошення; журнали сесій,
 * git і lease relay не тримає (межі — access.md, «Relay: обов'язки і межі»).
 */
import { Buffer } from 'node:buffer'
import { randomUUID, timingSafeEqual } from 'node:crypto'

import { PUBKEY_RE } from './signing.mjs'

/**
 * Порівняння секретних токенів за сталий час (захист від timing-атак).
 * @param {string} a перший токен
 * @param {string} b другий токен
 * @returns {boolean} true якщо токени збігаються
 */
function tokenEquals(a, b) {
  const bufferA = Buffer.from(a)
  const bufferB = Buffer.from(b)
  if (bufferA.length !== bufferB.length) return false
  return timingSafeEqual(bufferA, bufferB)
}

/** Ролі учасників задачі (access.md): owner ⊃ host ⊃ approver ⊃ viewer. */
const ROLES = ['owner', 'host', 'approver', 'viewer']

/**
 * Чи достатня роль `actual` для мінімально потрібної `required`.
 * @param {string | null} actual фактична роль учасника (або null — не учасник)
 * @param {string} required мінімально потрібна роль
 * @returns {boolean} true якщо роль достатня
 */
export function roleAtLeast(actual, required) {
  if (!actual) return false
  return ROLES.indexOf(actual) <= ROLES.indexOf(required) && ROLES.includes(actual)
}

/** In-memory реалізація store-інтерфейсу relay. */
export class InMemoryStore {
  constructor() {
    /** @type {Map<string, {account_id: string, email: string, display_name: string}>} */
    this.accounts = new Map()
    /** @type {Map<string, {device_id: string, account_id: string, role: string, pubkey: string, name: string, device_token: string, last_seen: string | null}>} */
    this.devices = new Map()
    /** @type {Map<string, {root_node_hash: string, owner_account: string, project_name: string, remote_url: string, created_at: string}>} */
    this.tasks = new Map()
    /** @type {Map<string, Map<string, string>>} root_node_hash → (account_id → role) */
    this.members = new Map()
    /** @type {Map<string, {invitation_id: string, root_node_hash: string, from_account: string, to_email: string, role: string, status: string, created_at: string}>} */
    this.invitations = new Map()
  }

  /**
   * Створює акаунт (relay-логін; auth-провайдер — поза store).
   * @param {{ email: string, displayName?: string }} params дані акаунта
   * @returns {{account_id: string, email: string, display_name: string}} акаунт
   */
  createAccount({ email, displayName = '' }) {
    const account = { account_id: randomUUID(), email, display_name: displayName }
    this.accounts.set(account.account_id, account)
    return account
  }

  /**
   * Акаунт за email (для доставки запрошень).
   * @param {string} email email акаунта
   * @returns {{account_id: string, email: string, display_name: string} | null} акаунт або null
   */
  accountByEmail(email) {
    for (const account of this.accounts.values()) if (account.email === email) return account
    return null
  }

  /**
   * Реєструє пристрій акаунта: `{name, role, pubkey} → device_token` (access.md).
   * Pubkey — hex 32-байтовий Ed25519 (формат, який очікує pubkey-кеш хоста
   * в agent-server): невалідний формат відхиляється одразу, а не на першій
   * невдалій перевірці підпису.
   * @param {string} accountId акаунт-власник
   * @param {{ name: string, role: 'host'|'client', pubkey: string }} params дані пристрою
   * @returns {{device_id: string, device_token: string}} ідентифікатор і токен пристрою
   * @throws {Error} pubkey не hex-32
   */
  registerDevice(accountId, { name, role, pubkey }) {
    if (!PUBKEY_RE.test(pubkey ?? '')) {
      throw new Error('registerDevice відхилено: pubkey має бути hex Ed25519 (32 байти)')
    }
    const device = {
      device_id: randomUUID(),
      account_id: accountId,
      role,
      pubkey,
      name,
      device_token: randomUUID(),
      last_seen: null
    }
    this.devices.set(device.device_id, device)
    return { device_id: device.device_id, device_token: device.device_token }
  }

  /**
   * Пристрій за device_token (авторизація WS-підключення).
   * @param {string} token device_token
   * @returns {object | null} запис пристрою або null
   */
  deviceByToken(token) {
    for (const device of this.devices.values()) {
      if (tokenEquals(device.device_token, token)) return device
    }
    return null
  }

  /**
   * Реєструє задачу (кореневий вузол); власник стає owner автоматично.
   * @param {string} rootNodeHash node-hash кореневого вузла
   * @param {string} ownerAccount акаунт-власник
   * @param {{ projectName?: string, remoteUrl?: string }} [meta] метадані
   * @returns {object} запис задачі
   */
  createTask(rootNodeHash, ownerAccount, meta = {}) {
    const task = {
      root_node_hash: rootNodeHash,
      owner_account: ownerAccount,
      project_name: meta.projectName ?? '',
      remote_url: meta.remoteUrl ?? '',
      created_at: new Date().toISOString()
    }
    this.tasks.set(rootNodeHash, task)
    this.members.set(rootNodeHash, new Map([[ownerAccount, 'owner']]))
    return task
  }

  /**
   * Роль акаунта у задачі.
   * @param {string} rootNodeHash кореневий вузол
   * @param {string} accountId акаунт
   * @returns {string | null} роль або null (не учасник)
   */
  memberRole(rootNodeHash, accountId) {
    return this.members.get(rootNodeHash)?.get(accountId) ?? null
  }

  /**
   * Встановлює/змінює роль учасника.
   * @param {string} rootNodeHash кореневий вузол
   * @param {string} accountId акаунт
   * @param {string} role нова роль
   * @returns {void}
   */
  setMemberRole(rootNodeHash, accountId, role) {
    this.members.get(rootNodeHash)?.set(accountId, role)
  }

  /**
   * Прибирає учасника.
   * @param {string} rootNodeHash кореневий вузол
   * @param {string} accountId акаунт
   * @returns {void}
   */
  removeMember(rootNodeHash, accountId) {
    this.members.get(rootNodeHash)?.delete(accountId)
  }

  /**
   * Учасники задачі.
   * @param {string} rootNodeHash кореневий вузол
   * @returns {{account_id: string, role: string}[]} перелік учасників
   */
  membersOf(rootNodeHash) {
    const members = this.members.get(rootNodeHash)
    if (!members) return []
    return Array.from(members.entries(), ([account_id, role]) => ({ account_id, role }))
  }

  /**
   * Створює запрошення (status: pending).
   * @param {string} rootNodeHash кореневий вузол
   * @param {string} fromAccount хто запрошує
   * @param {string} toEmail кого
   * @param {string} role роль після accept
   * @returns {object} запис запрошення
   */
  createInvitation(rootNodeHash, fromAccount, toEmail, role) {
    const invitation = {
      invitation_id: randomUUID(),
      root_node_hash: rootNodeHash,
      from_account: fromAccount,
      to_email: toEmail,
      role,
      status: 'pending',
      created_at: new Date().toISOString()
    }
    this.invitations.set(invitation.invitation_id, invitation)
    return invitation
  }

  /**
   * Запрошення за id.
   * @param {string} invitationId id запрошення
   * @returns {object | null} запис або null
   */
  invitationById(invitationId) {
    return this.invitations.get(invitationId) ?? null
  }

  /**
   * Відкрите (pending) запрошення email-а у задачу — для ідемпотентного
   * bootstrap: повторний прогін не плодить дублікати.
   * @param {string} rootNodeHash кореневий вузол
   * @param {string} toEmail email запрошеного
   * @returns {object | null} pending-запрошення або null
   */
  pendingInvitationFor(rootNodeHash, toEmail) {
    for (const invitation of this.invitations.values()) {
      if (
        invitation.root_node_hash === rootNodeHash &&
        invitation.to_email === toEmail &&
        invitation.status === 'pending'
      ) {
        return invitation
      }
    }
    return null
  }

  /**
   * Pubkey-и пристроїв учасників із роллю approver+ (для перевірки підписів
   * approvals хостом; access.md «GET pubkeys»).
   * @param {string} rootNodeHash кореневий вузол
   * @returns {{device_id: string, account_id: string, pubkey: string}[]} pubkey-и
   */
  pubkeysFor(rootNodeHash) {
    const approvers = new Set(
      this.membersOf(rootNodeHash)
        .filter(m => roleAtLeast(m.role, 'approver'))
        .map(m => m.account_id)
    )
    return this.devices
      .values()
      .filter(device => approvers.has(device.account_id))
      .map(({ device_id, account_id, pubkey }) => ({ device_id, account_id, pubkey }))
      .toArray()
  }
}
