/**
 * Dev-sink push-доставки: складає нотифікації в памʼять (як magic tokens
 * в auth) — реальний FCM-транспорт підключається за тим самим інтерфейсом
 * `deliver(accountId, note)` окремою задачею (stack.md, «Push»).
 */

/** Dev-реалізація sink-а push-доставки. */
export class DevPushSink {
  constructor() {
    /** @type {{account_id: string, root: string, reason: string, ref: string | null}[]} */
    this.deliveries = []
  }

  /**
   * Доставляє push усім пристроям акаунта.
   * @param {string} accountId акаунт-отримувач
   * @param {{ root: string, reason: string, ref?: string | null }} note зміст
   * @returns {void}
   */
  deliver(accountId, note) {
    this.deliveries.push({ account_id: accountId, root: note.root, reason: note.reason, ref: note.ref ?? null })
  }
}
