---
type: Rust Module
title: discovery.rs
resource: crates/agent-server/src/discovery.rs
docgen:
  crc: c3f1df7b
  model: openai-codex/gpt-5.4-mini
  tier: cloud-min
  score: 100
  issues: judge:inaccurate:0.99
  judgeModel: openai-codex/gpt-5.4-mini
---

## Огляд

Файл описує файлову discovery-точку для одного agent-server на машині: через `server.port`, `server.token` і `server.lock` він дає thin client знайти процес за контрактом runtime.md. `server.port` містить port, pid і sha256-хеш token; `server.token` зберігає сирий token з правами `0600` і читається лише тим самим користувачем. Перевірка, чи запис живий або stale, — обовʼязок клієнта через пробний `ClientHello`; stale lock перезаписується. Модуль працює fail-safe: перехоплює помилки й не кидає винятків назовні.

## Поведінка

- `token_hash` — рахує sha256-хеш токена у hex для запису в `server.port` без сирого токена.
- `PortFile` — описує вміст `server.port`: порт, pid і хеш токена.
- `Discovery` — представляє файлову discovery-точку для одного agent-server у конфігурованій директорії.
- `new` — створює discovery з вказаною директорією.
- `write` — записує `server.port`, `server.token` і `server.lock`; `server.token` зберігає з правами 0600.
- `read` — читає `server.port` і `server.token` та звіряє хеш токена; при розбіжності повертає помилку.
- `remove` — прибирає discovery-файли і не ламається, якщо частини вже немає.

## Публічний API

- token_hash — SHA-256 hex від токена для port-file; сам токен туди не записується.
- PortFile — вміст `server.port`.
- Discovery — точка файлового виявлення в налаштованій директорії: у продакшні `~/.nitra`, у тестах `tempdir`.
- new — створює новий discovery-набір для запуску сервера.
- write — записує port-file, token-файл із правами 0600 і lock; існуючий lock замінює, а живість сервера потім звіряє клієнт через ClientHello.
- read — читає port-file і сирий токен, звіряє хеш; якщо файли не узгоджені, повертає помилку про підміну.
- remove — прибирає discovery-файли після завершення сервера.

## Гарантії поведінки

- Перехоплює помилки і не пропускає винятків назовні (fail-safe).
