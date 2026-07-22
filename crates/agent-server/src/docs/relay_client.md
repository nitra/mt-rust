---
type: Rust Module
title: relay_client.rs
resource: crates/agent-server/src/relay_client.rs
docgen:
  crc: 64b7eedb
  model: openai-codex/gpt-5.4-mini
  score: 100
  issues: judge:inaccurate:0.99
  judgeModel: openai-codex/gpt-5.4-mini
---

## Огляд

Міст для host-runtime, що тримає вихідне WS-з’єднання з relay і ретранслює broadcast сесій хоста в relay як `{kind:"envelope", root, envelope}`. На вхід приймає `!from_host` і передає ці кадри в штатну обробку кадру клієнта; `from_host` ставить relay за роллю пристрою, а host-echo bridge ігнорує, щоб не утворювався цикл. Після збоїв виконує reconnect з експоненційним backoff; після відновлення стрічка лишається цілісною через журнал сесій, а replay залишається обов’язком клієнтів, не relay. Компонент працює fail-safe: не кидає винятків назовні, а за певних помилок повертає порожнє значення. Кешування відсутнє.

## Поведінка

- RelayBridgeConfig — конфігурює міст до relay: адресу relay, токен host-пристрою та кореневий вузол кімнати задачі.
- spawn_relay_bridge — запускає фоновий міст до relay з автоматичним reconnect і ретрансляцією сесій хоста; fail-safe, не пише у ФС/БД і не кидає помилки назовні.

Changelog: не перевірено (потрібен `npx @nitra/cursor lint changelog`)

## Публічний API

- RelayBridgeConfig — налаштовує міст для підключення до relay.
- spawn_relay_bridge — запускає міст у фоні, тримає його до аборту хоста і відновлює зʼєднання через reconnect із backoff від 1s до 30s.

## Гарантії поведінки

- Read-only: не виконує операцій запису (ФС/БД).
- Перехоплює помилки і не пропускає винятків назовні (fail-safe).
- За певних помилок повертає порожнє значення (напр. `null`) замість винятку.
