/**
 * Dev-sink push-доставки: складає нотифікації в памʼять (як magic tokens
 * в auth). Реальний транспорт — `fcm-sink.mjs` за тим самим контрактом
 * `deliver(accountId, note) → {delivered, dropped}`; обидві реалізації
 * проходять спільний набір `push-sink-contract`.
 *
 * `deliver` async, хоча тут нічого не чекає: мережевий транспорт
 * синхронним не буває, а контракт спільний — та сама причина, що й для
 * store.
 */

/** Dev-реалізація sink-а push-доставки. */
export class DevPushSink {
  constructor() {
    /** @type {{account_id: string, type: number, root: string, reason: string, ref: string | null}[]} */
    this.deliveries = []
  }

  /**
   * Доставляє push усім пристроям акаунта.
   * @param {string} accountId акаунт-отримувач
   * @param {{ type: number, root: string, reason: string, ref?: string | null }} note зміст
   * @returns {Promise<{delivered: number, dropped: number}>} підсумок доставки
   */
  async deliver(accountId, note) {
    this.deliveries.push({
      account_id: accountId,
      type: note.type,
      root: note.root,
      reason: note.reason,
      ref: note.ref ?? null
    })
    return { delivered: 1, dropped: 0 }
  }
}
