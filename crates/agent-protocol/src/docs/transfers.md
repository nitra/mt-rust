---
type: Rust Module
title: transfers.rs
resource: crates/agent-protocol/src/transfers.rs
docgen:
  crc: d2fec6bc
  model: openai-codex/gpt-5.4-mini
  score: 100
  issues: judge:inaccurate:0.99
  judgeModel: openai-codex/gpt-5.4-mini
---

## Огляд

Файл описує canonical акт transfer ownership у Membership API та його byte-for-byte сумісність із relay через доменний префікс і NUL-роздільник полів. Він потрібен, щоб пристрій поточного owner-а міг підписати цей акт Ed25519, а relay — перевірити підпис проти pubkey пристрою поточного owner-а за тим самим форматом повідомлення. `TransferPayload`, `message`, `sign_transfer`, `verify_transfer` працюють read-only, fail-safe, не кидають винятків назовні й за окремих помилок повертають порожнє значення замість винятку.

## Поведінка

- `TransferPayload` — описує акт передачі ownership між поточним і новим owner для кореневої задачі.
- `message` — формує canonical bytes акта transfer для підпису й перевірки, сумісні з relay.
- `sign_transfer` — створює Ed25519-підпис акта transfer приватним ключем пристрою owner-а.
- `verify_transfer` — перевіряє підпис акта transfer проти public key пристрою та повертає помилку, якщо підпис хибний або має некоректну довжину.

## Публічний API

- TransferPayload — Фіксує акт передачі права власності на кореневу задачу.
- message — Формує канонічні байти акта передачі для підпису й звірки.
- sign_transfer — Підписує акт transfer ключем пристрою власника.
- verify_transfer — Звіряє підпис акта з pubkey пристрою, що ініціював передачу.

## Гарантії поведінки

- Read-only: не виконує операцій запису (ФС/БД).
- Перехоплює помилки і не пропускає винятків назовні (fail-safe).
- За певних помилок повертає порожнє значення (напр. `null`) замість винятку.
