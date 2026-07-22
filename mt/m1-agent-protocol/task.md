---
schema_version: 1
created_at: 2026-07-08T04:31:30Z
budget_sec: 7200
hint: atomic
---

## Mission

Заскафолдити Rust crate `agent-protocol` (перший компонент M1 з roadmap): типи `Envelope`/`Event` протоколу v4 за npm/docs/architecture/runtime.md (serde-серіалізація), Ed25519-підписи approvals за npm/docs/architecture/access.md (`ed25519-dalek`), константа `PROTOCOL_VERSION = 4` і перевірка сумісності хендшейку з явною помилкою.

## Done when

- `crates/agent-protocol/` компілюється у workspace (`cargo check -p agent-protocol`);
- усі варіанти `Event` з runtime.md представлені типами; roundtrip serde-тест на кожен;
- підпис/верифікація `(request_id, approved, node_hash, run_token)` покриті тестом (валідний + зіпсований підпис);
- `ClientHello` без `lang` не десеріалізується (обовʼязкове поле v4);
- crate НЕ залежить від tokio/tauri (`cargo tree -p agent-protocol -e normal` чистий — фізична межа зі stack.md);
- `cargo test -p agent-protocol` зелений.

## Context

- Нормативні джерела: npm/docs/architecture/runtime.md (протокол, Envelope/Event, хендшейк), npm/docs/architecture/access.md (підписи, три гейти), npm/docs/architecture/stack.md (межі crate, залежності).
- Референсні кодові бази для рішень (не для копіювання) — перелік у stack.md.
- Це перша задача M0-dogfood: тертя контракту MT, помічене під час виконання, занотувати у run-нотатках.
