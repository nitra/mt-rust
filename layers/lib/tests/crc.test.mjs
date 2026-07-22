import { describe, expect, test } from 'vitest'

import { crc32hex, essenceCrc, fileCrc, normalizeEssence } from '../crc.mjs'

const HEX8_RE = /^[0-9a-f]{8}$/

describe('crc32hex', () => {
  test('порожній рядок → 00000000', () => {
    expect(crc32hex('')).toBe('00000000')
  })

  test('класичний check-вектор "123456789" → cbf43926', () => {
    expect(crc32hex('123456789')).toBe('cbf43926')
  })

  test('UTF-8: кирилиця хешується за байтами, звірка з node:zlib', async () => {
    const { crc32 } = await import('node:zlib')
    for (const sample of ['граф задач', 'a', 'Суть\nдеталі', '123456789']) {
      expect(crc32hex(sample)).toBe(crc32(sample).toString(16).padStart(8, '0'))
    }
  })

  test('завжди 8 hex lowercase', () => {
    expect(crc32hex('a')).toMatch(HEX8_RE)
    expect(crc32hex(' ')).toMatch(HEX8_RE)
  })
})

describe('normalizeEssence / essenceCrc', () => {
  test('rewrap абзаців не змінює CRC', () => {
    const one = 'Граф задач живе у git.\nКоординація — через CAS claims.'
    const two = 'Граф задач живе у git. Координація — через CAS claims.'
    expect(essenceCrc(one)).toBe(essenceCrc(two))
  })

  test('CRLF, хвостові пробіли, порожні рядки не змінюють CRC', () => {
    const base = 'Один канон.\nПереклади derived.'
    expect(essenceCrc('Один канон.\r\n\r\n  Переклади derived.  \n')).toBe(essenceCrc(base))
  })

  test('NFC-нормалізація: композитна і декомпонована "й" еквівалентні', () => {
    expect(essenceCrc('його')).toBe(essenceCrc('його'))
  })

  test('зміна слова змінює CRC', () => {
    expect(essenceCrc('Граф задач живе у git.')).not.toBe(essenceCrc('Граф задач живе у svn.'))
  })

  test('normalizeEssence колапсить будь-який whitespace до одного пробілу', () => {
    expect(normalizeEssence('  a\t\tb\n\nc  ')).toBe('a b c')
  })
})

describe('fileCrc', () => {
  test('CRLF→LF нормалізується, решта байтів значуща', () => {
    expect(fileCrc('a\r\nb')).toBe(fileCrc('a\nb'))
    expect(fileCrc('a\nb')).not.toBe(fileCrc('a\nb '))
  })
})
