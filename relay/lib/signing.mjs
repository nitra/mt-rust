/**
 * Перевірка Ed25519-підписів на relay (node:crypto, без залежностей).
 *
 * Дзеркалить canonical-формат crates/agent-protocol: домен-префікс і
 * NUL-розділені поля — підпис, зроблений Rust-клієнтом (`sign_transfer`),
 * перевіряється тут байт-у-байт. Pubkey пристрою — hex 32 байти (як у
 * agent-server relay_client), загортається у SPKI DER для node:crypto.
 */
import { Buffer } from 'node:buffer'
import { createPublicKey, verify } from 'node:crypto'

/** ASN.1 SPKI-префікс для raw Ed25519 pubkey (RFC 8410). */
const SPKI_PREFIX = Buffer.from('302a300506032b6570032100', 'hex')

/** Домен підпису transfer ownership (дзеркало agent-protocol). */
const TRANSFER_DOMAIN = 'mt-transfer-v4'

/** Hex-формат Ed25519 pubkey пристрою: рівно 32 байти. */
export const PUBKEY_RE = /^[0-9a-f]{64}$/i

/**
 * Canonical-повідомлення transfer ownership: домен і поля через NUL —
 * межі полів однозначні, підпис не переноситься між контекстами.
 * @param {{ root: string, fromAccount: string, toAccount: string }} payload акт передачі
 * @returns {Buffer} байти для підпису/перевірки
 */
export function transferMessage({ root, fromAccount, toAccount }) {
  return Buffer.from([TRANSFER_DOMAIN, root, fromAccount, toAccount].join('\0'), 'utf8')
}

/**
 * Перевіряє Ed25519-підпис повідомлення проти hex-pubkey пристрою.
 * @param {string} pubkeyHex pubkey пристрою (hex, 32 байти)
 * @param {Buffer} message canonical-повідомлення
 * @param {string} signatureBase64 підпис (base64, як в Envelope)
 * @returns {boolean} true якщо підпис валідний
 */
export function verifySignature(pubkeyHex, message, signatureBase64) {
  if (!PUBKEY_RE.test(pubkeyHex)) return false
  let signature
  try {
    signature = Uint8Array.fromBase64(signatureBase64)
  } catch {
    return false
  }
  if (signature.length !== 64) return false
  const key = createPublicKey({
    key: Buffer.concat([SPKI_PREFIX, Buffer.from(pubkeyHex, 'hex')]),
    format: 'der',
    type: 'spki'
  })
  return verify(null, message, key, signature)
}
