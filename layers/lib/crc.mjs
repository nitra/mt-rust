/** @see ./docs/crc.md */

import { crc32 } from 'node:zlib'

/**
 * CRC32 тексту (UTF-8) як 8 hex lowercase — формат `docgen.crc`.
 * `node:zlib` кодує рядок як UTF-8 сам; підтримується і в Node ≥20.15, і в Bun.
 * @param {string} text текст для хешування
 * @returns {string} 8 hex lowercase символів CRC32
 */
export function crc32hex(text) {
  return crc32(text).toString(16).padStart(8, '0')
}

/**
 * Нормалізація тексту суті перед CRC: косметика (rewrap, CRLF, хвостові
 * пробіли, кількість порожніх рядків) не змінює результат — CRC чутливий
 * лише до зміни слів.
 * @param {string} text текст суті
 * @returns {string} нормалізований текст (одна лінія, згорнутий whitespace)
 */
export function normalizeEssence(text) {
  return text.normalize('NFC').replaceAll(/\s+/g, ' ').trim()
}

/**
 * essence-CRC: crc32hex від нормалізованої суті.
 * @param {string} text текст суті
 * @returns {string} 8 hex lowercase символів CRC32
 */
export function essenceCrc(text) {
  return crc32hex(normalizeEssence(text))
}

/**
 * file-CRC: crc32hex тіла документа (без frontmatter) з нормалізацією CRLF→LF.
 * @param {string} body тіло документа без frontmatter
 * @returns {string} 8 hex lowercase символів CRC32
 */
export function fileCrc(body) {
  return crc32hex(body.replaceAll('\r\n', '\n'))
}
