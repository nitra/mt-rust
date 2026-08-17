/**
 * Вибір sink-а push за середовищем — той самий патерн, що й `create-store`
 * та `create-auth`.
 *
 * Усі три змінні FCM задані → реальна доставка; інакше — dev-sink у памʼять.
 * Перемикач на повноті трійки, а не на окремому прапорці: сервісний акаунт
 * без будь-якої з частин непрацездатний, і краще лишитись у dev-режимі
 * явно, ніж падати на першій нотифікації.
 */
import process from 'node:process'
import { FcmPushSink, GoogleAccessToken } from './fcm-sink.mjs'
import { DevPushSink } from './push-sink.mjs'

/**
 * @param {object} store store relay (push-токени пристроїв)
 * @param {Record<string, string|undefined>} [env] середовище; дефолт — `process.env`
 * @returns {object} sink за контрактом `deliver(accountId, note)`
 */
export function createPushSink(store, env = process.env) {
  const { FCM_PROJECT_ID, FCM_CLIENT_EMAIL, FCM_PRIVATE_KEY } = env
  if (!FCM_PROJECT_ID || !FCM_CLIENT_EMAIL || !FCM_PRIVATE_KEY) return new DevPushSink()
  return new FcmPushSink({
    store,
    projectId: FCM_PROJECT_ID,
    accessToken: new GoogleAccessToken({
      clientEmail: FCM_CLIENT_EMAIL,
      // У змінних середовища PEM переносить рядки як \n — інакше ключ
      // не розбереться.
      privateKey: FCM_PRIVATE_KEY.replaceAll('\\n', '\n')
    })
  })
}
