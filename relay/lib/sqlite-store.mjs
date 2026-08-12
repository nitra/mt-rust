/**
 * SQLite-реалізація store-інтерфейсу relay: той самий контракт, що й
 * `InMemoryStore` — обидві проходять спільний набір `store-contract`.
 *
 * Навіщо: in-memory сховище втрачає акаунти й membership на рестарті relay,
 * тобто учасники «зникають» разом із процесом. Persist стосується ЛИШЕ
 * акаунтів/пристроїв/membership/запрошень — журнали сесій, git і lease
 * relay не тримає за визначенням (access.md, «Relay: обов'язки і межі»).
 *
 * Чому SQLite: обсяг даних relay — це
 * акаунти й membership, тобто одиниці мегабайтів і одиниці записів на
 * хвилину; єдиний файл дає персистентність без інфраструктури, і — що
 * важливіше — його видно в тестах на кожній машині, а не лише там, де
 * піднято БД. Контракт store параметризований, тож перехід на PostgreSQL
 * лишається заміною однієї реалізації, а не переписуванням relay.
 *
 * Драйвер — вбудований `bun:sqlite`, без зовнішньої залежності.
 */
import { randomUUID } from 'node:crypto'
import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

import { PUBKEY_RE } from './signing.mjs'
import { roleAtLeast } from './store.mjs'

const SCHEMA_PATH = join(dirname(fileURLToPath(import.meta.url)), 'schema.sql')

/** ISO8601 — той самий формат часу, що і в in-memory реалізації. */
function now() {
  return new Date().toISOString()
}

/**
 * Store поверх SQLite. Створюється через [`createSqliteStore`], щоб схема
 * гарантовано була застосована до першого запиту.
 *
 * Методи async, хоча `bun:sqlite` синхронний: контракт store спільний для
 * всіх реалізацій, а мережеве сховище синхронним не буває.
 */
export class SqliteStore {
  /**
   * @param {import('bun:sqlite').Database} db відкрита база
   */
  constructor(db) {
    this.db = db
  }

  /**
   * Застосовує схему (ідемпотентно).
   * @returns {void}
   */
  migrate() {
    this.db.exec(readFileSync(SCHEMA_PATH, 'utf8'))
  }

  /**
   * Створює акаунт; email унікальний — повторний логін повертає наявний,
   * а не плодить дублікати.
   * @param {{ email: string, displayName?: string }} params дані акаунта
   * @returns {Promise<{account_id: string, email: string, display_name: string}>} акаунт
   */
  async createAccount({ email, displayName = '' }) {
    const existing = await this.accountByEmail(email)
    if (existing) return existing
    const account = { account_id: randomUUID(), email, display_name: displayName }
    this.db
      .query('INSERT INTO accounts (account_id, email, display_name) VALUES (?, ?, ?)')
      .run(account.account_id, account.email, account.display_name)
    return account
  }

  /**
   * Акаунт за email (для доставки запрошень).
   * @param {string} email email акаунта
   * @returns {Promise<object | null>} акаунт або null
   */
  async accountByEmail(email) {
    return this.db.query('SELECT * FROM accounts WHERE email = ?').get(email) ?? null
  }

  /**
   * Акаунт за id (перевірка адресата запрошення).
   * @param {string} accountId акаунт
   * @returns {Promise<object | null>} акаунт або null
   */
  async accountById(accountId) {
    return this.db.query('SELECT * FROM accounts WHERE account_id = ?').get(accountId) ?? null
  }

  /**
   * Реєструє пристрій акаунта. Валідація pubkey — до запису: невалідний
   * формат має відхилятись одразу, а не на першій невдалій перевірці підпису.
   * @param {string} accountId акаунт-власник
   * @param {{ name: string, role: string, pubkey: string }} params дані пристрою
   * @returns {Promise<{device_id: string, device_token: string}>} ідентифікатор і токен
   * @throws {Error} pubkey не hex-32
   */
  async registerDevice(accountId, { name, role, pubkey }) {
    if (!PUBKEY_RE.test(pubkey ?? '')) {
      throw new Error('registerDevice відхилено: pubkey має бути hex Ed25519 (32 байти)')
    }
    const device = { device_id: randomUUID(), device_token: randomUUID() }
    this.db
      .query(
        `INSERT INTO devices (device_id, account_id, name, role, pubkey, device_token)
         VALUES (?, ?, ?, ?, ?, ?)`
      )
      .run(device.device_id, accountId, name ?? '', role, pubkey, device.device_token)
    return device
  }

  /**
   * Пристрій за device_token (авторизація WS-підключення).
   * @param {string} token device_token
   * @returns {Promise<object | null>} запис пристрою або null
   */
  async deviceByToken(token) {
    return this.db.query('SELECT * FROM devices WHERE device_token = ?').get(token ?? '') ?? null
  }

  /**
   * Реєструє задачу; власник стає owner автоматично (access.md).
   * @param {string} rootNodeHash node-hash кореневого вузла
   * @param {string} ownerAccount акаунт-власник
   * @param {{ projectName?: string, remoteUrl?: string }} [meta] метадані
   * @returns {Promise<object>} запис задачі
   */
  async createTask(rootNodeHash, ownerAccount, meta = {}) {
    const task = {
      root_node_hash: rootNodeHash,
      owner_account: ownerAccount,
      project_name: meta.projectName ?? '',
      remote_url: meta.remoteUrl ?? '',
      created_at: now()
    }
    this.db
      .query(
        `INSERT INTO tasks (root_node_hash, owner_account, project_name, remote_url, created_at)
         VALUES (?, ?, ?, ?, ?)
         ON CONFLICT(root_node_hash) DO UPDATE SET project_name = excluded.project_name`
      )
      .run(task.root_node_hash, task.owner_account, task.project_name, task.remote_url, task.created_at)
    await this.setMemberRole(rootNodeHash, ownerAccount, 'owner')
    return task
  }

  /**
   * Роль акаунта у задачі.
   * @param {string} rootNodeHash кореневий вузол
   * @param {string} accountId акаунт
   * @returns {Promise<string | null>} роль або null
   */
  async memberRole(rootNodeHash, accountId) {
    const row = this.db
      .query('SELECT role FROM task_members WHERE root_node_hash = ? AND account_id = ?')
      .get(rootNodeHash, accountId)
    return row?.role ?? null
  }

  /**
   * Upsert ролі: accept-запрошення додає учасника, PATCH role змінює
   * наявного — одна операція на обидва шляхи (як в in-memory).
   * @param {string} rootNodeHash кореневий вузол
   * @param {string} accountId акаунт
   * @param {string} role роль
   * @returns {Promise<void>} завершення запису
   */
  async setMemberRole(rootNodeHash, accountId, role) {
    this.db
      .query(
        `INSERT INTO task_members (root_node_hash, account_id, role, joined_at)
         VALUES (?, ?, ?, ?)
         ON CONFLICT(root_node_hash, account_id) DO UPDATE SET role = excluded.role`
      )
      .run(rootNodeHash, accountId, role, now())
  }

  /**
   * Прибирає учасника.
   * @param {string} rootNodeHash кореневий вузол
   * @param {string} accountId акаунт
   * @returns {Promise<void>} завершення запису
   */
  async removeMember(rootNodeHash, accountId) {
    this.db.query('DELETE FROM task_members WHERE root_node_hash = ? AND account_id = ?').run(rootNodeHash, accountId)
  }

  /**
   * Учасники задачі.
   * @param {string} rootNodeHash кореневий вузол
   * @returns {Promise<{account_id: string, role: string}[]>} перелік учасників
   */
  async membersOf(rootNodeHash) {
    return this.db.query('SELECT account_id, role FROM task_members WHERE root_node_hash = ?').all(rootNodeHash)
  }

  /**
   * Створює запрошення (status: pending).
   * @param {string} rootNodeHash кореневий вузол
   * @param {string} fromAccount хто запрошує
   * @param {string} toEmail кого
   * @param {string} role роль після accept
   * @returns {Promise<object>} запис запрошення
   */
  async createInvitation(rootNodeHash, fromAccount, toEmail, role) {
    const invitation = {
      invitation_id: randomUUID(),
      root_node_hash: rootNodeHash,
      from_account: fromAccount,
      to_email: toEmail,
      role,
      status: 'pending',
      created_at: now()
    }
    this.db
      .query(
        `INSERT INTO invitations
           (invitation_id, root_node_hash, from_account, to_email, role, status, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?)`
      )
      .run(
        invitation.invitation_id,
        invitation.root_node_hash,
        invitation.from_account,
        invitation.to_email,
        invitation.role,
        invitation.status,
        invitation.created_at
      )
    return invitation
  }

  /**
   * Запрошення за id.
   * @param {string} invitationId id запрошення
   * @returns {Promise<object | null>} запис або null
   */
  async invitationById(invitationId) {
    return this.db.query('SELECT * FROM invitations WHERE invitation_id = ?').get(invitationId) ?? null
  }

  /**
   * Відкрите (pending) запрошення email-а у задачу — ідемпотентний bootstrap.
   * @param {string} rootNodeHash кореневий вузол
   * @param {string} toEmail email запрошеного
   * @returns {Promise<object | null>} pending-запрошення або null
   */
  async pendingInvitationFor(rootNodeHash, toEmail) {
    return (
      this.db
        .query(
          `SELECT * FROM invitations
           WHERE root_node_hash = ? AND to_email = ? AND status = 'pending'`
        )
        .get(rootNodeHash, toEmail) ?? null
    )
  }

  /**
   * Статус запрошення: accepted | declined | revoked.
   * @param {string} invitationId id запрошення
   * @param {string} status новий статус
   * @returns {Promise<void>} завершення запису
   */
  async setInvitationStatus(invitationId, status) {
    this.db.query('UPDATE invitations SET status = ? WHERE invitation_id = ?').run(status, invitationId)
  }

  /**
   * Pubkey-и пристроїв учасників `approver+` (access.md «GET pubkeys»).
   * Фільтр ролей — через [`roleAtLeast`], щоб ієрархія ролей мала рівно
   * одне визначення на всі реалізації store.
   * @param {string} rootNodeHash кореневий вузол
   * @returns {Promise<{device_id: string, account_id: string, pubkey: string}[]>} pubkey-и
   */
  async pubkeysFor(rootNodeHash) {
    const approvers = (await this.membersOf(rootNodeHash))
      .filter(member => roleAtLeast(member.role, 'approver'))
      .map(member => member.account_id)
    if (approvers.length === 0) return []
    const placeholders = approvers.map(() => '?').join(', ')
    return this.db
      .query(
        `SELECT device_id, account_id, pubkey FROM devices
         WHERE account_id IN (${placeholders})`
      )
      .all(...approvers)
  }

  /**
   * Закриває базу (тести, зупинка relay).
   * @returns {void}
   */
  close() {
    this.db.close()
  }
}

/**
 * Створює SQLite-store і застосовує схему.
 * @param {string} path шлях до файлу бази (`:memory:` — тимчасова)
 * @returns {Promise<SqliteStore>} готовий store
 */
export async function createSqliteStore(path) {
  const { Database } = await import('bun:sqlite')
  const store = new SqliteStore(new Database(path, { create: true }))
  store.migrate()
  return store
}
