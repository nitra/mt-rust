---
schema_version: 1
created_at: 2026-07-12T15:43:09Z
budget_sec: 10800
hint: atomic
---

## Mission

M2, ws-рівень кооперативного handoff (продовження #39: graph-рівень `InteractiveRun::handoff`/`graph::attach_resume` вже готовий, ws-обвʼязка — з явних «поза скоупом» пунктів). Мета: `AppState`-рівень API, що знімає run з обліку сесій, викликає `handoff`/`attach_resume`, і — ключова частина — засіває журнал сесії на новому хості так, щоб `SessionHost`/`Session` продовжили той самий seq, що й до передачі (реплей для клієнтів, що реконектяться, залишається безшовним). Wire-протокол для «хто ініціює handoff» (relay `HandoffRequest`, новий client→host Event) — свідомо ПОЗА цією задачею: `Event`-варіантів для handoff в agent-protocol ще немає, і його дизайн — окреме рішення разом із relay-маршрутизацією.

## Done when

- `SessionHost::seed_journal(&self, node: &str, jsonl: &str) -> Result<(), String>`: пише вміст напряму у локальний файл сесії (`<state_dir>/<node>.session.jsonl`) ДО першого відкриття; помилка, якщо сесія для цього ключа вже відкрита (живий стан у пам'яті заднім числом не перечитується). Той самий формат, що й `.nitra/session.jsonl` (по Envelope на рядок) — `Session::open` після сіву продовжує seq природно (як після рестарту хоста);
- `AppState::handoff_node(node) -> Result<HandoffTicket, String>`: знімає run з `runs`-мапи, викликає `InteractiveRun::handoff`, публікує `ClaimChanged { holder_device_id: None, .. }` у сесію (той самий сигнал, що й release — деталь «це handoff, не пауза» лишається в `run_NNN.md`);
- `AppState::resume_node(node, &ticket) -> Result<(), String>`: `graph::attach_resume` → читає `.nitra/session.jsonl` відновленого worktree → `seed_journal` (найкраще зусилля: помилка сіву не валить resume — сесія просто почне з чистого seq) → вкладає run у `runs`-мапу → `spawn_renewal`;
- тести: unit `seed_journal` (сіяний журнал підхоплюється `get_or_open`, seq продовжується; сів після відкриття — явна помилка); інтеграційний на двох `AppState` (той самий bare-origin, різні `state_dir` — симуляція «двох хостів»): attach на «хості 1» → хід → `handoff_node` → `resume_node` на «хості 2» з тим самим тікетом → `get_or_open` на хості 2 віддає `replay_from(0)`, що включає успадковані Envelope з хоста 1, і НАСТУПНИЙ хід продовжує seq без розривів;
- `cargo test --workspace` зелений; без tauri.

## Check

cargo test -p agent-server -q

## Context

- Нормативні джерела: npm/docs/architecture/runtime.md («Міграція сесії між хостами», кроки 2-3 — «клієнти... продовжують у тій самій кімнаті з новим активним хостом»), git.md (`handoff`).
- Побудовано на: `graph::{InteractiveRun::handoff, attach_resume, HandoffTicket}` (PR #39), `session::{SessionHost, Session}` (формат журналу — PR #25/#30).
- Поза скоупом (наступні задачі): будь-який новий `Event`-варіант для handoff і relay `HandoffRequest`-маршрутизація (окреме рішення дизайну протоколу), CLI-поверхня (`agent-cli handoff`/`resume`) — потребує саме wire-протоколу, checkpoint-режим, GC старого run ref-а після успішного resume+done.
