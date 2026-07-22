---
type: Rust Module
title: directory.rs
resource: crates/mt-core/src/directory.rs
docgen:
  crc: 5ce11be5
  model: openai-codex/gpt-5.4-mini
  tier: cloud-min
  score: 100
  issues: judge:inaccurate:0.98
  judgeModel: openai-codex/gpt-5.4-mini
---

## Огляд

Канонічний парсер вмісту `.mt/directory.json`: перетворює flat-мапінг `handle → PII` у структуру для подальшого використання в коді, щоб у git-файлах лишалися лише handles, а email та імʼя людини зберігалися поза історією. Для рішень, що спираються на цей формат, орієнтиром є конфіг `directory.json`. Публічний `resolve_email` повертає email для заданого handle або `null`, якщо запису немає чи вхідні дані не дають однозначного результату. Для частини некоректних або неповних даних модуль теж повертає порожнє значення замість винятку.

## Поведінка

- DirectoryEntry — зберігає PII для одного handle: email як основний ідентифікатор для relay та optional display name.
- parse_directory — перетворює вміст `.mt/directory.json` на мапінг handle → PII; відсутній або невалідний вміст дає порожній мапінг.
- resolve_email — повертає email для заданого handle або `null`, якщо handle відсутній у directory.

## Публічний API

- DirectoryEntry — запис directory для одного handle: email як ключ relay-акаунта, name як display-імʼя
- parse_directory — читає `.mt/directory.json` і будує мапінг handle → PII; якщо файла нема, JSON зламаний або корінь не обʼєкт, повертає порожній мапінг; записи без email ігнорує
- resolve_email — повертає email для handle; якщо handle не знайдено, віддає `None`, тож адресний push без `to_account_id` не формується

## Гарантії поведінки

- Read-only: не виконує операцій запису (ФС/БД).
- За певних помилок повертає порожнє значення (напр. `null`) замість винятку.
