---
type: JS Module
title: store.mjs
resource: relay/lib/store.mjs
docgen:
  crc: c3b0fdd5
  model: openai-codex/gpt-5.4-mini
  score: 100
  issues: judge:inaccurate:0.97
  judgeModel: openai-codex/gpt-5.4-mini
---

## Огляд

Локальне relay-сховище за схемою `access.md` для `accounts`, `devices`, `tasks`, `task_members` і `invitations`. Це dev/test реалізація `InMemoryStore` для того самого store-інтерфейсу, який описує `stack.md` у блоці «Relay-інфраструктура». Межі relay задає `access.md` у блоці «Relay: обов'язки і межі»: у relay персистентні лише `accounts`, `membership` і `invitations`, а журнали сесій, git і lease тут не зберігаються. `ROLES` і `roleAtLeast` описують рольову модель, яку цей store відображає на дані схеми.

## Поведінка

- `ROLES` — фіксує порядок ролей учасника задачі для перевірки прав доступу.
- `roleAtLeast` — визначає, чи достатня фактична роль для потрібного рівня.
- `InMemoryStore` — надає in-memory сховище relay для акаунтів, пристроїв, задач, membership і запрошень; це dev/тестова реалізація store-інтерфейсу, без персистенції журналів сесій, git і lease.

## Публічний API

ROLES — задає ієрархію ролей у задачі: owner вище за host, host вище за approver, approver вище за viewer.
roleAtLeast — визначає, чи має фактична роль не нижчий рівень, ніж мінімально потрібна.
InMemoryStore — зберігає стан relay у пам’яті без зовнішнього сховища.

## Гарантії поведінки

- (специфічних машинно-виведених гарантій немає)
