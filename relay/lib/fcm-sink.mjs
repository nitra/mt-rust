/**
 * FCM-транспорт push (stack.md, «Push: FCM (data-повідомлення трьох типів)»).
 *
 * Той самий контракт sink-а, що й `DevPushSink` — `deliver(accountId, note)`;
 * обидві реалізації проходять спільний набір `push-sink-contract`.
 *
 * Дві частини, бо це дві різні відповідальності: [`GoogleAccessToken`] —
 * OAuth2 service-account flow (підписаний JWT → access token із кешем), і
 * [`FcmPushSink`] — власне доставка `messages:send` на всі push-токени
 * акаунта.
 *
 * `fetch` інʼєктується. Межа перевірки чесна: контрактний набір доводить,
 * що relay формує документовані запити й правильно реагує на документовані
 * відповіді (успіх, протухлий токен, 5xx), і НЕ доводить доставки живим
 * FCM — це видно лише на реальному проєкті Firebase.
 */
import { Buffer } from 'node:buffer'
import { createSign } from 'node:crypto'
import { URLSearchParams } from 'node:url'

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

/** Скоуп, якого вимагає FCM HTTP v1. */
const FCM_SCOPE = 'https://www.googleapis.com/auth/firebase.messaging'

/** Запас перед закінченням access token — щоб не ловити 401 на межі. */
const TOKEN_SKEW_SEC = 60

/** Час життя підписаного JWT (максимум, який приймає Google, — година). */
const ASSERTION_TTL_SEC = 3600

/**
 * base64url без padding — те, що вимагає JWT.
 * @param {Buffer|string} value дані
 * @returns {string} base64url
 */
function base64url(value) {
  return Buffer.from(value).toBase64({ alphabet: 'base64url', omitPadding: true })
}

/**
 * Access token сервісного акаунта Google з кешем до закінчення строку.
 *
 * Кеш тут не оптимізація: без нього кожен push тягнув би окремий обмін на
 * token endpoint, і сплеск подій у задачі перетворився б на сплеск логінів.
 */
export class GoogleAccessToken {
  /**
   * @param {{ clientEmail: string, privateKey: string, tokenUri?: string, fetch?: typeof globalThis.fetch, now?: () => number }} deps креденшели сервісного акаунта і клієнти
   */
  constructor({
    clientEmail,
    privateKey,
    tokenUri = 'https://oauth2.googleapis.com/token',
    fetch = globalThis.fetch,
    now = () => Date.now()
  }) {
    this.clientEmail = clientEmail
    this.privateKey = privateKey
    this.tokenUri = tokenUri
    this.fetch = fetch
    this.now = now
    /** @type {{token: string, expires_at: number} | null} */
    this.cached = null
  }

  /**
   * Підписаний RS256 JWT-assertion для grant-type `jwt-bearer`.
   * @returns {string} assertion
   */
  assertion() {
    const issuedAt = Math.floor(this.now() / 1000)
    const header = base64url(JSON.stringify({ alg: 'RS256', typ: 'JWT' }))
    const claims = base64url(
      JSON.stringify({
        iss: this.clientEmail,
        scope: FCM_SCOPE,
        aud: this.tokenUri,
        iat: issuedAt,
        exp: issuedAt + ASSERTION_TTL_SEC
      })
    )
    const signer = createSign('RSA-SHA256')
    signer.update(`${header}.${claims}`)
    return `${header}.${claims}.${signer.sign(this.privateKey, 'base64url')}`
  }

  /**
   * Дійсний access token (з кешу або новий).
   * @returns {Promise<string>} bearer-токен
   * @throws {Error} token endpoint відмовив
   */
  async token() {
    if (this.cached && this.cached.expires_at > this.now()) return this.cached.token
    const response = await this.fetch(this.tokenUri, {
      method: 'POST',
      headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
      body: new URLSearchParams({
        grant_type: 'urn:ietf:params:oauth:grant-type:jwt-bearer',
        assertion: this.assertion()
      }).toString()
    })
    if (!response.ok) throw new Error(`push: token endpoint відмовив (${response.status})`)
    const body = await response.json()
    if (!body?.access_token) throw new Error('push: token endpoint не повернув access_token')
    this.cached = {
      token: body.access_token,
      expires_at: this.now() + Math.max(0, Number(body.expires_in ?? 0) - TOKEN_SKEW_SEC) * 1000
    }
    return this.cached.token
  }
}

/**
 * Sink push поверх FCM HTTP v1.
 *
 * Доставка йде на **всі** push-токени акаунта: пристроїв у людини кілька,
 * і relay не знає, який із них зараз у руках.
 */
export class FcmPushSink {
  /**
   * @param {{ store: object, projectId: string, accessToken: GoogleAccessToken, fetch?: typeof globalThis.fetch, endpoint?: string }} deps store, проєкт Firebase, джерело токена
   */
  constructor({ store, projectId, accessToken, fetch = globalThis.fetch, endpoint = 'https://fcm.googleapis.com' }) {
    this.store = store
    this.projectId = projectId
    this.accessToken = accessToken
    this.fetch = fetch
    this.endpoint = stripTrailingSlashes(String(endpoint))
  }

  /**
   * Доставляє data-повідомлення на всі push-токени акаунта.
   *
   * Помилка доставки на один пристрій НЕ зриває решту: push — сигнал
   * «подивись у задачу», а не транспорт даних, тож втрата на одному
   * пристрої не має ставати втратою на всіх. Протухлий токен
   * (`404`/`UNREGISTERED`) знімається зі store — інакше видалений застосунок
   * тягнув би за собою помилку на кожній події назавжди.
   * @param {string} accountId акаунт-отримувач
   * @param {{ type: number, root: string, reason: string, ref?: string | null }} note зміст
   * @returns {Promise<{delivered: number, dropped: number}>} підсумок доставки
   */
  async deliver(accountId, note) {
    const targets = await this.store.pushTokensFor(accountId)
    if (targets.length === 0) return { delivered: 0, dropped: 0 }

    const bearer = await this.accessToken.token()
    // FCM приймає в `data` лише рядки — числа й null довелося б серіалізувати
    // на приймальній стороні, тому приводимо тут.
    const data = {
      type: String(note.type),
      root: String(note.root),
      reason: String(note.reason),
      ref: note.ref ? String(note.ref) : ''
    }

    let delivered = 0
    let dropped = 0
    for (const target of targets) {
      const response = await this.fetch(`${this.endpoint}/v1/projects/${this.projectId}/messages:send`, {
        method: 'POST',
        headers: { Authorization: `Bearer ${bearer}`, 'Content-Type': 'application/json' },
        body: JSON.stringify({ message: { token: target.push_token, data } })
      })
      if (response.ok) {
        delivered += 1
        continue
      }
      if (response.status === 404 || response.status === 400) {
        await this.store.setPushToken(target.device_id, '')
        dropped += 1
      }
    }
    return { delivered, dropped }
  }
}
