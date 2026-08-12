/**
 * Push-нотифікації relay (access.md, «Push-нотифікації») — три типи
 * data-повідомлень:
 *
 * 1. «нові події у задачі X» — розбудити клієнт/хост;
 * 2. «вас запрошено у задачу X»;
 * 3. «задача X потребує уваги» — `unresolvable`, `plan-review`, `pending`
 *    для `h.md`-assignee (`notify: true`), `AuditPending`.
 *
 * Тут — маршрутизація; доставка — sink (`push-sink.mjs` для dev,
 * `fcm-sink.mjs` для FCM). Relay не парсить payload далі роутінгових полів:
 * для push роутінговими є `event.type`, `event.state`, `event.notify` і
 * адресний `event.to_account_id`.
 */

/** Типи подій Envelope, що самі по собі означають «потребує уваги». */
const ATTENTION_TYPES = new Set(['PlanReview', 'AuditPending', 'Escalation'])

/** Стани вузла, що потребують уваги (тип 3). */
const ATTENTION_STATES = new Set(['unresolvable', 'plan-review'])

/**
 * Мінімальний інтервал між розсилками push типу 1 у межах однієї задачі.
 * Тип 1 не несе змісту — він каже «є нові події», тож повторення протягом
 * вікна не додає нічого, крім вібрації в кишені.
 */
const WAKE_COOLDOWN_MS = 30_000

/** Маршрутизатор push поверх store, кімнат і sink-а. */
export class PushRouter {
  /**
   * @param {{ store: object, sink: object, rooms?: object, wakeCooldownMs?: number, now?: () => number }} deps залежності
   */
  constructor({ store, sink, rooms = null, wakeCooldownMs = WAKE_COOLDOWN_MS, now = () => Date.now() }) {
    this.store = store
    this.sink = sink
    this.rooms = rooms
    this.wakeCooldownMs = wakeCooldownMs
    this.now = now
    /** @type {Map<string, number>} час останньої розсилки типу 1 на задачу */
    this.lastWake = new Map()
  }

  /**
   * Тип 2: «вас запрошено у задачу X». Незареєстрований email — тихо
   * (запрошення pending до реєстрації, push наздожене при onboarding).
   * @param {string} email email запрошеного
   * @param {string} root кореневий вузол задачі
   * @returns {Promise<boolean>} true якщо акаунт існує і push відправлено
   */
  async invited(email, root) {
    const account = await this.store.accountByEmail(email)
    if (!account) return false
    await this.sink.deliver(account.account_id, { type: 2, root, reason: 'invited' })
    return true
  }

  /**
   * Класифікує подію: тип 3 (потребує уваги) чи ні.
   * @param {object} event подія Envelope
   * @returns {boolean} чи потребує уваги
   */
  static needsAttention(event) {
    if (ATTENTION_TYPES.has(event.type)) return true
    if (event.type !== 'NodeState') return false
    // `pending` — увага лише для явно призначеної людини з `notify: true`
    // (h.md): інакше кожен вузол, що став до черги, будив би всю кімнату.
    if (event.state === 'pending') return event.notify === true
    return ATTENTION_STATES.has(event.state)
  }

  /**
   * Акаунти, що зараз мають живу підписку на кімнату.
   * @param {string} root кореневий вузол задачі
   * @returns {Set<string>} акаунти онлайн
   */
  onlineAccounts(root) {
    const online = new Set()
    for (const subscriber of this.rooms?.room(root).subscribers ?? []) {
      if (subscriber.accountId) online.add(subscriber.accountId)
    }
    return online
  }

  /**
   * Push із події Envelope.
   *
   * Тип 3 адресний або широкий (за винятком автора — він і так знає).
   * Тип 1 — усе інше: будить лише тих, хто **не** підписаний на кімнату.
   * Push існує, щоб розбудити те, що не підключене; пристрою з живою
   * підпискою подія вже прийшла кадром, і push поверх неї — чистий шум.
   * @param {string} root кореневий вузол задачі
   * @param {object} envelope конверт (парситься лише до роутінгових полів)
   * @param {string} senderAccount акаунт-автор конверта
   * @returns {Promise<void>} завершення розсилки
   */
  async onEnvelope(root, envelope, senderAccount) {
    const event = envelope?.event
    if (!event) return
    const ref = event.reason_ref ?? event.plan_ref ?? event.fact_ref ?? null

    if (!PushRouter.needsAttention(event)) {
      await this.wake(root, senderAccount)
      return
    }

    if (event.type === 'Escalation' || (event.type === 'NodeState' && event.state === 'pending')) {
      // Адресат резолвиться емітером (handle → account через
      // `.mt/directory.json`); без резолву адресний push неможливий —
      // не спамимо всю кімнату.
      if (event.to_account_id) {
        await this.sink.deliver(event.to_account_id, { type: 3, root, reason: event.type, ref })
      }
      return
    }

    for (const member of await this.store.membersOf(root)) {
      if (member.account_id === senderAccount) continue
      await this.sink.deliver(member.account_id, { type: 3, root, reason: event.type, ref })
    }
  }

  /**
   * Тип 1: «нові події у задачі X» — офлайновим учасникам, не частіше
   * одного разу на вікно в межах задачі.
   *
   * Вікно саме задачне, а не на пару (задача, акаунт): по-перше, воно
   * відсікає звернення до store ще до запиту — конверти течуть потоком під
   * час ходу агента, і `membersOf` на кожен фрагмент був би запитом до бази на
   * кожен рядок виводу; по-друге, стан вікна лишається одним записом на
   * задачу замість запису на кожного учасника.
   *
   * Ціна відома й обмежена: учасник, який щойно відпав або сам був автором
   * попередньої події, дочекається наступного вікна. Для сигналу «є нові
   * події» (без змісту) затримка до вікна нічого не втрачає.
   * @param {string} root кореневий вузол задачі
   * @param {string} senderAccount акаунт-автор події
   * @returns {Promise<void>} завершення розсилки
   */
  async wake(root, senderAccount) {
    const at = this.now()
    if (at - (this.lastWake.get(root) ?? -Infinity) < this.wakeCooldownMs) return
    this.lastWake.set(root, at)

    const online = this.onlineAccounts(root)
    for (const member of await this.store.membersOf(root)) {
      if (member.account_id === senderAccount || online.has(member.account_id)) continue
      await this.sink.deliver(member.account_id, { type: 1, root, reason: 'new-events' })
    }
  }
}
