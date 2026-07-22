---
type: Rust Module
title: session.rs
resource: crates/agent-server/src/session.rs
docgen:
  crc: a5f3196b
  model: openai-codex/gpt-5.4-mini
  score: 100
  issues: judge:inaccurate:0.97
  judgeModel: openai-codex/gpt-5.4-mini
---

## Огляд

Модуль веде інтерактивні сесії як `run` вузла: збирає `Envelope`, призначає їм монотонний `seq` у межах run і працює з `session.jsonl` як append-only журналом подій. Ефемерні події `AgentTextDelta` і `PreviewScreenshot` не журналяться; натомість у журнал потрапляє агрегат `AgentTextDone`. Через `replay_from` сесію можна відновити після рестарту хоста з `session.jsonl`. `SessionHost`, `new`, `get_or_open` і `session_list` керують відкритими сесіями хоста, а `publish`, `subscribe` і `broadcast` забезпечують обмін подіями між учасниками. Модуль fail-safe: помилки не виходять назовні.

## Поведінка

- `is_ephemeral` — визначає, чи подія є ефемерною й не має потрапляти в журнал.
- `Session` — тримає стан однієї сесії: `seq`, `run_token`, журнал і шлях до `session.jsonl`.
- `append` — збирає `Envelope`, призначає `seq` і час, журналить неефемерні події та повертає конверт для подальшої розсилки.
- `replay_from` — віддає журнальовані події сесії, починаючи з указаного `seq`, для відновлення після реконекту.
- `SessionHost` — керує набором сесій хоста й спільною broadcast-розсилкою між клієнтами.
- `new` — створює хост сесій і готує директорію стану.
- `get_or_open` — повертає сесію кімнати або ліниво відкриває її з журналу.
- `publish` — додає подію в сесію і одразу розсилає конверт підписникам.
- `subscribe` — підписує на broadcast-потік нових `Envelope`.
- `session_list` — повертає перелік активних сесій для `ServerHello`.
- `replay_from` — збирає журнальовані події всіх сесій, починаючи з указаного `seq`, у стабільному порядку.

## Публічний API

- is_ephemeral — позначає події, що йдуть лише через relay/WS і не потрапляють у журнал чи git.
- Session — тримає один run вузла: seq, журнал і `session.jsonl`.
- append — додає конверт із host-поставленими seq і ts, записує неефемерні події та готує broadcast.
- replay_from — повертає журнальовані події з `seq >= from` для replay після reconnect.
- SessionHost — веде реєстр сесій хоста й один спільний broadcast-канал для всіх кімнат; на відправці хост фільтрує за `node_hash` і `capabilities`.
- new — створює host-реєстр і спільний канал для сесій.
- get_or_open — ліниво відкриває кімнатну сесію або відновлює її з журналу.
- publish — додає подію в сесію й розсилає її підключеним клієнтам.
- subscribe — підписує клієнта на broadcast поточної сесії.
- session_list — віддає список активних сесій для `ServerHello.session_list`.
- replay_from — повертає replay усіх журнальованих подій з `seq >= from`, відсортований за ``.

## Гарантії поведінки

- Перехоплює помилки і не пропускає винятків назовні (fail-safe).
