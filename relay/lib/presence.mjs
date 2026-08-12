/**
 * Presence relay (access.md: «presence (хости: hostname, проєкти, активні
 * вузли)»).
 *
 * **Ефемерне за побудовою** — і це не оптимізація, а межа зі спеки:
 * «Relay — ефемерний координатор, не сховище» (overview.md), персистентні в
 * ньому лише акаунти/membership/запрошення. Тому presence живе в памʼяті
 * процесу й зникає разом із ним: після рестарту relay його не «відновлює»,
 * а збирає заново з того, що оголосять живі пристрої. Персистентним
 * лишається тільки `last_seen` пристрою — це факт про пристрій, а не про
 * поточне зʼєднання.
 *
 * Presence НЕ є джерелом істини про «хто пише» — ним лишається git claim
 * (overview.md, принцип 1). Тут — лише «хто зараз на звʼязку і над чим»,
 * тобто підказка для людини й для маршрутизації, а не право на запис.
 */

/**
 * Скільки запис живе без оновлення. Закриття сокета прибирає presence
 * одразу; TTL — страховка на випадок, коли close не прийшов (half-open
 * зʼєднання, обрив мережі), інакше «привиди» висіли б у кімнаті вічно.
 */
const PRESENCE_TTL_MS = 90_000

/** Реєстр присутності за кімнатами. */
export class Presence {
  /**
   * @param {{ ttlMs?: number, now?: () => number }} [options] TTL і годинник (інʼєкція для тестів)
   */
  constructor({ ttlMs = PRESENCE_TTL_MS, now = () => Date.now() } = {}) {
    this.ttlMs = ttlMs
    this.now = now
    /** @type {Map<string, Map<string, object>>} кімната → пристрій → запис */
    this.rooms = new Map()
  }

  /**
   * Оголошує/оновлює присутність пристрою в кімнаті.
   *
   * `hostname`/`projects`/`nodes` — рівно те, що перелічує access.md. Relay
   * їх не інтерпретує: це рядки для показу людині, а не роутінгові поля.
   * @param {string} root кореневий вузол задачі
   * @param {{ deviceId: string, accountId: string, role?: string, hostname?: string, projects?: string[], nodes?: string[] }} entry запис присутності
   * @returns {object} нормалізований запис
   */
  announce(root, entry) {
    let room = this.rooms.get(root)
    if (!room) {
      room = new Map()
      this.rooms.set(root, room)
    }
    const at = this.now()
    const record = {
      device_id: entry.deviceId,
      account_id: entry.accountId,
      role: entry.role ?? '',
      hostname: entry.hostname ?? '',
      projects: [...(entry.projects ?? [])],
      nodes: [...(entry.nodes ?? [])],
      // `since` переживає повторні оголошення: людині важливо «з якого часу
      // тут», а не «коли востаннє надіслав heartbeat».
      since: room.get(entry.deviceId)?.since ?? at,
      updated_at: at
    }
    room.set(entry.deviceId, record)
    return record
  }

  /**
   * Прибирає пристрій із кімнати (закриття сокета, відписка).
   * @param {string} root кореневий вузол задачі
   * @param {string} deviceId пристрій
   * @returns {boolean} чи був запис
   */
  forget(root, deviceId) {
    const room = this.rooms.get(root)
    if (!room?.delete(deviceId)) return false
    if (room.size === 0) this.rooms.delete(root)
    return true
  }

  /**
   * Присутні в кімнаті; протухлі записи прибираються при читанні —
   * окремого таймера не заводимо, бо він тримав би процес живим заради
   * прибирання памʼяті.
   * @param {string} root кореневий вузол задачі
   * @returns {object[]} записи присутності
   */
  of(root) {
    const room = this.rooms.get(root)
    if (!room) return []
    const deadline = this.now() - this.ttlMs
    for (const [deviceId, record] of room) {
      if (record.updated_at <= deadline) room.delete(deviceId)
    }
    if (room.size === 0) {
      this.rooms.delete(root)
      return []
    }
    return [...room.values()]
  }
}
