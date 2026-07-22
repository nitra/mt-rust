/**
 * Кімнати relay: підписки за кореневим вузлом задачі, broadcast Envelope,
 * буфер останніх N Envelope (live-хвіст для реплею при підписці).
 *
 * Relay НЕ парсить payload далі роутінгових полів і НЕ зберігає журнали
 * сесій (access.md, «Relay: обов'язки і межі») — буфер ефемерний, глибший
 * реплей клієнт добирає з `session.jsonl` run ref-а через хост.
 */

/** Ліміт буфера кімнати (stack.md: «буфер ≤ 200 Envelope/run»). */
const BUFFER_LIMIT = 200

/** Кімнати з ефемерним буфером і підписниками. */
export class Rooms {
  /**
   * @param {number} [bufferLimit] ліміт буфера кімнати
   */
  constructor(bufferLimit = BUFFER_LIMIT) {
    this.bufferLimit = bufferLimit
    /** @type {Map<string, {buffer: object[], subscribers: Set<{deviceId: string, send: (frame: object) => void}>}>} */
    this.rooms = new Map()
  }

  /**
   * Кімната за ключем (створюється ліниво).
   * @param {string} root кореневий вузол задачі
   * @returns {{buffer: object[], subscribers: Set<object>}} кімната
   */
  room(root) {
    let room = this.rooms.get(root)
    if (!room) {
      room = { buffer: [], subscribers: new Set() }
      this.rooms.set(root, room)
    }
    return room
  }

  /**
   * Підписує пристрій: спершу реплей буфера, далі — live-стрічка.
   * @param {string} root кореневий вузол задачі
   * @param {{deviceId: string, send: (frame: object) => void}} subscriber підписник
   * @returns {() => void} відписка
   */
  subscribe(root, subscriber) {
    const room = this.room(root)
    for (const frame of room.buffer) subscriber.send(frame)
    room.subscribers.add(subscriber)
    return () => room.subscribers.delete(subscriber)
  }

  /**
   * Broadcast кадру всім підписникам кімнати; буферизує з обрізанням до ліміту.
   * @param {string} root кореневий вузол задачі
   * @param {object} frame кадр (envelope чи service-подія) — opaque
   * @returns {void}
   */
  publish(root, frame) {
    const room = this.room(root)
    room.buffer.push(frame)
    if (room.buffer.length > this.bufferLimit) {
      room.buffer.splice(0, room.buffer.length - this.bufferLimit)
    }
    for (const subscriber of room.subscribers) subscriber.send(frame)
  }
}
