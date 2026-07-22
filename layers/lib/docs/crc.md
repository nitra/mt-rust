---
type: JS Module
title: crc.mjs
resource: layers/lib/crc.mjs
docgen:
  crc: c91aead7
  model: openai-codex/gpt-5.4-mini
  score: 100
  issues: judge:inaccurate:0.98
  judgeModel: openai-codex/gpt-5.4-mini
---

## Огляд

Файл формує стабільне CRC для тексту й документа: `normalizeEssence` зводить суть і тіло файлу до однакового представлення, `essenceCrc` і `fileCrc` дають значення для перевірки змін, а `crc32hex` повертає CRC у hex-форматі. Це дає змогу відрізняти змістові зміни від косметичних і порівнювати результат без прив’язки до форматування.

## Поведінка

- `crc32hex` — обчислює CRC32 для тексту в UTF-8 і повертає 8-символьний lowercase hex, сумісний із `docgen.crc`.
- `normalizeEssence` — нормалізує текст суті так, щоб косметичні зміни на кшталт переносу рядків, CRLF, хвостових пробілів і зайвих порожніх рядків не впливали на результат.
- `essenceCrc` — рахує CRC32 від нормалізованої суті тексту.
- `fileCrc` — рахує CRC32 тіла документа без frontmatter із нормалізацією CRLF до LF.

## Публічний API

- crc32hex — рахує CRC32 від UTF-8 тексту й повертає його як 8-символьний lowercase hex для `docgen.crc`.
- normalizeEssence — приводить текст суті до стабільного вигляду перед CRC: зміни в обгортанні, CRLF, хвостових пробілах і кількості порожніх рядків не змінюють результат; CRC реагує лише на зміну слів.
- essenceCrc — обчислює essence-CRC як `crc32hex` від нормалізованої суті.
- fileCrc — обчислює file-CRC як `crc32hex` від тіла документа без frontmatter після заміни CRLF на LF.

## Гарантії поведінки

- Read-only: не виконує операцій запису (ФС/БД).
