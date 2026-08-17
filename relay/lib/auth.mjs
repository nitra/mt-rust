/**
 * Auth relay: перевірка сесії користувача перед реєстрацією пристрою.
 *
 * Контракт (stack.md, «Relay-інфраструктура») — один метод:
 * `verifySession(token) → {account_id}`. Реалізацій дві, обидві проходять
 * спільний набір `auth-contract`: [`DevMagicAuth`] (dev, magic tokens) і
 * [`KratosAuth`] (продакшн, Ory Kratos за тим самим інтерфейсом).
 *
 * Навіщо окремий шар: до цього єдиним credential-ом relay був
 * `device_token`, а видати його не було чим — акаунти й пристрої
 * створювались лише прямим викликом store з тестів. Тобто реального шляху
 * «людина → пристрій у кімнаті» не існувало. Сесія закриває саме цю ланку:
 * доводить контроль над email, після чого пристрій реєструє свій pubkey і
 * далі живе за `device_token` (access.md, «Акаунти, пристрої, ключі»).
 *
 * Спільна для реалізацій точка стикування — **email**: identity провайдера
 * мапиться на локальний акаунт через `createAccount` (upsert за email), тож
 * `account_id` лишається стабільним при зміні провайдера.
 */
import { randomUUID } from 'node:crypto'

/**
 * Прибирає кінцеві слеші без регулярного виразу.
 *
 * Наївний `/\/+$/` дає суперлінійний бектрекінг на рядку з довгим хвостом
 * слешів — дешевий вектор ReDoS, тому це цикл, а не regex.
 * @param {string} value адреса
 * @returns {string} адреса без кінцевих слешів
 */
function stripTrailingSlashes(value) {
  let end = value.length
  while (end > 0 && value[end - 1] === '/') end -= 1
  return value.slice(0, end)
}

/** Дефолтний час життя сесії — доба (реалізація, не контракт). */
export const DEFAULT_SESSION_TTL_MS = 24 * 60 * 60 * 1000

/**
 * Дістає email з traits identity Kratos.
 * @param {object} identity identity з відповіді whoami
 * @returns {string} email або порожній рядок
 */
function emailOf(identity) {
  const email = identity?.traits?.email
  return typeof email === 'string' ? email : ''
}

/**
 * Dev-реалізація: magic tokens без доставки листа.
 *
 * У продакшні magic link надсилають на email, і саме перехід за посиланням
 * доводить контроль над скринькою. У dev доставки немає — тому токен
 * **повертається викликачу напряму**: це той самий секрет, лише без
 * поштового шляху. Наслідок свідомий і обмежувальний: провайдер, який уміє
 * видати сесію на будь-який email без доказу, придатний ЛИШЕ для dev, і
 * саме тому видача живе в ньому, а не в ядрі relay ([`KratosAuth`] методу
 * `issueSession` не має — прод-шлях відмовляє за побудовою, а не за
 * прапорцем).
 */
export class DevMagicAuth {
  /**
   * @param {{ store: object, ttlMs?: number, now?: () => number }} deps store, TTL і годинник (інʼєкція для тестів)
   */
  constructor({ store, ttlMs = DEFAULT_SESSION_TTL_MS, now = () => Date.now() }) {
    this.store = store
    this.ttlMs = ttlMs
    this.now = now
    /** @type {Map<string, {account_id: string, expires_at: number}>} */
    this.sessions = new Map()
  }

  /**
   * Видає сесію на email; акаунт створюється при першому логіні.
   * @param {string} email email користувача
   * @returns {Promise<{token: string, account_id: string, expires_at: string}>} сесія
   * @throws {Error} порожній email
   */
  async issueSession(email) {
    if (!email) throw new Error('login відхилено: потрібен email')
    const account = await this.store.createAccount({ email })
    const token = randomUUID()
    const expiresAt = this.now() + this.ttlMs
    this.sessions.set(token, { account_id: account.account_id, expires_at: expiresAt })
    return {
      token,
      account_id: account.account_id,
      expires_at: new Date(expiresAt).toISOString()
    }
  }

  /**
   * Перевіряє сесію. Прострочена видаляється одразу — інакше мапа росла б
   * назавжди, а «прострочено» і «невідомо» мають бути нерозрізнюваними для
   * викликача.
   * @param {string} token токен сесії
   * @returns {Promise<{account_id: string}>} акаунт сесії
   * @throws {Error} невідомий або прострочений токен
   */
  async verifySession(token) {
    const session = this.sessions.get(token ?? '')
    if (!session) throw new Error('сесія відхилена: невідомий токен')
    if (session.expires_at <= this.now()) {
      this.sessions.delete(token)
      throw new Error('сесія відхилена: невідомий токен')
    }
    return { account_id: session.account_id }
  }

  /**
   * Завершує сесію (logout).
   * @param {string} token токен сесії
   * @returns {Promise<void>} завершення
   */
  async revokeSession(token) {
    this.sessions.delete(token ?? '')
  }
}

/**
 * Продакшн-реалізація поверх Ory Kratos: сесія перевіряється зверненням до
 * `GET {baseUrl}/sessions/whoami` із заголовком `X-Session-Token`.
 *
 * `fetch` інʼєктується — це не тестова зручність, а межа: relay не має
 * власного знання про Kratos поза цим одним запитом і формою відповіді
 * (`{active, identity: {traits: {email}}}`). **Межа перевірки чесна:**
 * контрактний набір доводить, що relay правильно обробляє документовану
 * форму відповіді (активна, неактивна, 401, без email), і НЕ доводить
 * сумісності з конкретною версією живого Kratos — це перевіряється лише
 * інтеграційно на розгорнутому інстансі.
 */
export class KratosAuth {
  /**
   * @param {{ store: object, baseUrl: string, fetch?: typeof globalThis.fetch }} deps store, адреса public API Kratos, HTTP-клієнт
   */
  constructor({ store, baseUrl, fetch = globalThis.fetch }) {
    this.store = store
    this.baseUrl = stripTrailingSlashes(String(baseUrl ?? ''))
    this.fetch = fetch
  }

  /**
   * Перевіряє сесію в Kratos і мапить identity на локальний акаунт за email.
   * @param {string} token session token Kratos
   * @returns {Promise<{account_id: string}>} акаунт сесії
   * @throws {Error} неактивна/невідома сесія або identity без email
   */
  async verifySession(token) {
    const response = await this.fetch(`${this.baseUrl}/sessions/whoami`, {
      headers: { 'X-Session-Token': String(token ?? '') }
    })
    if (!response.ok) throw new Error('сесія відхилена: невідомий токен')
    const session = await response.json()
    if (!session?.active) throw new Error('сесія відхилена: сесія неактивна')
    const email = emailOf(session.identity)
    // Локальний акаунт тримається на email: без нього немає чим стикувати
    // identity з membership, тож це відмова, а не акаунт-без-адреси.
    if (!email) throw new Error('сесія відхилена: identity без email')
    const name = session.identity?.traits?.name
    const account = await this.store.createAccount({
      email,
      displayName: typeof name === 'string' ? name : ''
    })
    return { account_id: account.account_id }
  }
}
