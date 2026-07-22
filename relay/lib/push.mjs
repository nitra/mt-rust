/**
 * Push-нотифікації relay (access.md, «Push-нотифікації»): «вас запрошено»
 * (тип 2) і «задача потребує уваги» (тип 3). FCM-доставка — окрема задача;
 * тут інтерфейс sink-а (dev-реалізація — `push-sink.mjs`, як magic tokens
 * в auth). Relay не парсить payload далі роутінгових полів — для push
 * роутінговими є `event.type` і адресний `event.to_account_id`.
 */

/** Типи подій Envelope, що означають «задача потребує уваги» (тип 3). */
const ATTENTION_TYPES = new Set(['PlanReview', 'AuditPending', 'Escalation'])

/** Маршрутизатор push поверх store і sink-а. */
export class PushRouter {
  /**
   * @param {{ store: import('./store.mjs').InMemoryStore, sink: import('./push-sink.mjs').DevPushSink }} deps залежності
   */
  constructor({ store, sink }) {
    this.store = store
    this.sink = sink
  }

  /**
   * Тип 2: «вас запрошено у задачу X». Незареєстрований email — тихо
   * (запрошення pending до реєстрації, push наздожене при onboarding).
   * @param {string} email email запрошеного
   * @param {string} root кореневий вузол задачі
   * @returns {boolean} true якщо доставлено (акаунт існує)
   */
  invited(email, root) {
    const account = this.store.accountByEmail(email)
    if (!account) return false
    this.sink.deliver(account.account_id, { root, reason: 'invited' })
    return true
  }

  /**
   * Тип 3: «задача X потребує уваги» з події Envelope. Адресна подія
   * (`Escalation` з `to_account_id`) будить лише адресата; безадресні
   * attention-події — всіх учасників, крім автора (він і так знає).
   * @param {string} root кореневий вузол задачі
   * @param {object} envelope конверт (роутінгові поля: event.type, event.to_account_id)
   * @param {string} senderAccount акаунт-автор конверта
   * @returns {void}
   */
  onEnvelope(root, envelope, senderAccount) {
    const event = envelope?.event
    if (!event) return
    const attention = ATTENTION_TYPES.has(event.type) || (event.type === 'NodeState' && event.state === 'unresolvable')
    if (!attention) return
    const ref = event.reason_ref ?? event.plan_ref ?? event.fact_ref ?? null

    if (event.type === 'Escalation') {
      // Адресат резолвиться емітером (handle → account через .mt/directory.json);
      // без резолву адресний push неможливий — не спамимо всю кімнату.
      if (event.to_account_id) {
        this.sink.deliver(event.to_account_id, { root, reason: 'escalation', ref })
      }
      return
    }

    for (const member of this.store.membersOf(root)) {
      if (member.account_id === senderAccount) continue
      this.sink.deliver(member.account_id, { root, reason: event.type, ref })
    }
  }
}
