/**
 * Вибір реалізації store за середовищем.
 *
 * `RELAY_DB_PATH` заданий → SQLite (дані переживають рестарт); інакше —
 * in-memory. Обидві реалізації проходять один набір `store-contract`, тож
 * перемикання не змінює поведінки relay — лише те, чи переживають дані
 * рестарт процесу.
 */
import process from 'node:process'
import { InMemoryStore } from './store.mjs'

/**
 * @param {string} [dbPath] шлях до SQLite-файлу; дефолт — `RELAY_DB_PATH`
 * @returns {Promise<object>} store за контрактом relay
 */
export async function createStore(dbPath = process.env.RELAY_DB_PATH) {
  if (!dbPath) {
    // In-memory — свідомий дефолт для dev і тестів: relay піднімається без
    // жодної підготовки. Ціна відома й задокументована: рестарт зносить
    // акаунти й membership, тому для будь-якого не-ефемерного інстансу
    // задається RELAY_DB_PATH.
    return new InMemoryStore()
  }
  const { createSqliteStore } = await import('./sqlite-store.mjs')
  return await createSqliteStore(dbPath)
}
