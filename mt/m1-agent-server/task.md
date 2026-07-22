---
schema_version: 1
created_at: 2026-07-11T11:05:00Z
budget_sec: 10800
hint: atomic
---

## Mission

Мінімальний `agent-server` (M1, одна машина, локальний WS, без relay) + тонкий `agent-cli` (`serve`/`attach`): session host за runtime.md — збірка `Envelope` (seq/ts/адресація) навколо подій `agent-core`, журнал `session.jsonl`, broadcast клієнтам, реплей за `want_replay_from`, фільтрація за capabilities, хендшейк v4, port-file discovery.

## Done when

- `crates/agent-server`: компілюється, `cargo test -p agent-server` зелений;
- session host: `seq` монотонний у межах run (призначає хост), ефемерні події (`AgentTextDelta`, `PreviewScreenshot`) НЕ журналяться (журналиться `AgentTextDone`), решта — append у `session.jsonl`;
- WS-хендшейк: перший кадр `ClientHello` → перевірка `PROTOCOL_VERSION` (несумісна → `Error` + закриття), відповідь `ServerHello { session_list }`; реплей журнальованих подій від `want_replay_from`;
- capability-фільтр: `PreviewScreenshot` лише клієнтам із «preview»;
- інтеграція agent-core: `UserMessage` → `TurnRunner` (референс — `AgentTurnRunner` поверх `Agent`/`Provider`) → події ходу в сесію; офлайн-тест через `MockProvider`;
- discovery: port-file (`server.port`: port + pid + sha256-хеш токена) + token-файл 0600 + lock; шлях конфігурується (тести — tempdir);
- `crates/agent-cli` (clap): `serve` (стартує сервер), `attach <node>` (читає discovery, хендшейк, REPL: stdin → UserMessage, друк дельт);
- інтеграційний WS-тест: реальний сервер на ефемерному порту, tungstenite-клієнт: хендшейк, хід із MockProvider, реплей після реконекту, відмова несумісної версії;
- без tauri у `cargo tree` обох крейтів.

## Context

- Нормативні джерела: npm/docs/architecture/runtime.md (протокол, хендшейк, backpressure/реплей, discovery), stack.md (axum + tokio-tungstenite; правило одного коду контракту — graph-операції ЛИШЕ через `mt … --json`, у цій задачі graph-операції не потрібні), git.md (журнал сесії; push run ref — окрема задача інтеграції з `@7n/mt`).
- Поза скоупом M1-заділу: relay, міграція між хостами, підписи approvals у потоці (типи вже в agent-protocol), push run ref у git (наступна задача — інтеграція wrapper/`mt`).
