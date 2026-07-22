---
schema_version: 1
created_at: 2026-07-12T05:26:51Z
budget_sec: 10800
hint: atomic
---

## Task

M2, міст agent-server ↔ relay — транспорт (в) із runtime.md («relay-клієнт — вихідне wss:// до relay для віддалених клієнтів»): хост підключається до relay, ретранслює host-події сесій у кімнату і приймає клієнтські кадри (UserMessage/DoneSession/ReleaseSession/ApprovalResponse) від віддалених пристроїв у штатну обробку. Захист від зациклення: relay ставить `from_host` на кадри пристроїв role=host — міст ігнорує host-ехо; тонкі клієнти рендерять лише host-кадри (з seq, який призначає хост).

## Done when

- relay (`relay/lib`): кадр кімнати `{kind:"envelope", envelope, from_host}` — `from_host` ставить relay за `device.role === 'host'` (НЕ з кадру клієнта — спуфінг виключено); тести оновлені;
- agent-server: модуль `relay_client` — `spawn_relay_bridge(state, config)`: reconnect із backoff, hello (device_token) → subscribe(root) → двонаправлена ретрансляція: broadcast сесій → relay; вхідні `!from_host` кадри → штатна обробка кадру клієнта (device_id — з envelope);
- зациклення виключено: host-ехо, що повертається з relay, ігнорується (тест: mock-relay ехоїть host-кадри назад — UserMessage у журналі рівно один);
- agent-cli serve: `--relay-url`/`--relay-token`/`--relay-root` вмикають міст;
- інтеграційний Rust-тест із mock-relay (tungstenite-сервер у тесті, кадровий протокол relay): віддалений UserMessage → хід агента → host-кадри доїжджають у relay;
- `cargo test --workspace` і `npx vitest run relay` зелені; без tauri.

## Check

cargo test -p agent-server -q
npx vitest run relay

## Inputs

- Нормативні: runtime.md (транспорти клієнтів; хост — єдиний тримач seq), access.md (кімната = задача, ролі).
- Побудовано на: relay/lib (PR #34), agent_server::{session,ws} (PR #25/#30).
- Поза скоупом: TLS/wss-конфіг (dev — ws до локального relay), FCM push, PostgreSQL-store, handoff.
