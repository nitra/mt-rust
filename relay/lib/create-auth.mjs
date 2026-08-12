/**
 * Вибір реалізації auth за середовищем — той самий патерн, що й
 * `create-store.mjs`.
 *
 * `KRATOS_PUBLIC_URL` заданий → Ory Kratos; інакше — dev magic tokens.
 * Перемикач саме такий (наявність адреси, а не окремий прапорець
 * `NODE_ENV`), щоб продакшн-режим не можна було ввімкнути наполовину:
 * без адреси Kratos перевіряти сесію нічим.
 */
import { DevMagicAuth, KratosAuth } from './auth.mjs'

/**
 * @param {object} store store relay (мапінг identity → акаунт)
 * @param {Record<string, string|undefined>} [env] середовище; дефолт — `process.env`
 * @returns {object} провайдер auth за контрактом `verifySession`
 */
export function createAuth(store, env = process.env) {
  const baseUrl = env.KRATOS_PUBLIC_URL
  if (baseUrl) return new KratosAuth({ store, baseUrl })
  // Dev — свідомий дефолт: relay піднімається без зовнішнього провайдера.
  // Ціна відома й обмежена конструкцією: DevMagicAuth видає сесію на
  // будь-який email без доказу володіння ним, тож будь-який не-ефемерний
  // інстанс задає KRATOS_PUBLIC_URL.
  return new DevMagicAuth({ store })
}
