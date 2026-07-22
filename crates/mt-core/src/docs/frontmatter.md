---
type: Rust Module
title: frontmatter.rs
resource: crates/mt-core/src/frontmatter.rs
docgen:
  crc: 0cc51d2c
  model: omlx/gemma-4-e2b-it-4bit
  tier: local-min
  score: 100
---

## Огляд

Файл відповідає за парсинг та серіалізацію YAML front-matter для task-файлів. Мета — гарантувати 1:1 ідентичність вихідного байта між JS-версією та функцією `serialize_yaml`, зберігаючи порядок вставки ключів.

## Поведінка

parse_front_matter: парсить YAML front-matter з markdown-тексту.
parse_yaml: парсить чистий YAML-блок (без `---`-маркерів).
get_body: повертає тіло документа (без front-matter, з обрізаним лівим whitespace).
build_markdown: будує markdown-файл із front-matter і тілом: `---\n<yaml>\n---\n\n<body>`.

## Публічний API

**parse_front_matter** — YAML front-matter з markdown-тексту.
**parse_yaml** — чистий YAML-блок без `---`-маркерів.
**get_body** — тіло документа без front-matter.
**build_markdown** — складання markdown-файлу з front-matter і тілом.

## Гарантії поведінки

- Read-only: не виконує операцій запису (ФС/БД).
- Перехоплює помилки і не пропускає винятків назовні (fail-safe).
- За певних помилок повертає порожнє значення (напр. `null`) замість винятку.
