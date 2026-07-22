---
schema_version: 1
created_at: 2026-07-12T14:56:40Z
budget_sec: 10800
hint: atomic
---

## Task

M2, graph-рівень міграції сесії між хостами (runtime.md, «Міграція сесії між хостами», кроки 2-3; git.md, claim-операція `handoff`): кооперативна передача claim-а — тримач пише `run_NNN.md (result: handoff)` + push run ref → CAS-delete claim; новий хост attach-иться заново, але worktree матеріалізується зі стану СТАРОГО run ref (не `origin/main`) — журнал `.nitra/session.jsonl` і мідфлайт-правки успадковані, розмова продовжується. Generation продовжує лічильник через handoff (git.md: «новий хост: create, generation+1»), хоч механічно це create-only CAS (старий claim уже видалено).

## Done when

- `graph::HandoffTicket { run_token, generation }` (Serialize/Deserialize — піде через relay у наступній задачі);
- `InteractiveRun::handoff(self) -> Result<HandoffTicket, String>`: синтезує `run_NNN.md (result: handoff)` через `mt_core::signal::write_run_fm` (той самий NNN-лічильник, що й success/fail), комітить, пушить run ref БЕЗ стрипу `.nitra/` (checkpoint-режим — окрема задача), потім CAS-delete claim; повертає тікет;
- `graph::attach_resume(config, node, &ticket) -> Result<InteractiveRun, String>`: fetch старого run ref за `ticket.run_token` (недоступний → явна помилка, не паніка) → CAS-create claim з `generation = ticket.generation + 1` → worktree від tip старого run ref (не `origin/main`) → push нового run ref новим token;
- рефакторинг: `attach`/`attach_resume` діляться спільною реалізацією (`resume_token: Option<&str>` перемикає worktree-базу і generation), дублювання виключено;
- тести (герметична фікстура, патерн наявних у graph.rs): (1) handoff пише run-файл result:handoff, `.nitra/session.jsonl` присутній у run ref, claim знято, worktree прибрано; (2) attach_resume після handoff — успіх, generation = old+1, worktree містить мідфлайт-файл і journal з попереднього ходу, новий run ref існує; (3) attach_resume з тікетом на неіснуючий run ref → явна помилка (не паніка); (4) наскрізний: attach → хід → handoff → attach_resume → done — публікує ту саму серію NNN без розривів;
- `cargo test --workspace` зелений; без tauri.

## Check

cargo test -p agent-server -p mt-core -q

## Inputs

- Нормативні: npm/docs/architecture/runtime.md («Міграція сесії між хостами»), git.md (таблиця claim-операцій, рядок `handoff`; «Checkpoint-handoff» — checkpoint-режим ПОЗА цією задачею).
- Побудовано на: graph.rs (`attach`/`done`/`release`, PR #28/#32/#37), `mt_core::signal::{next_run_nnn, write_run_fm}` (pub з PR #37).
- Поза скоупом (наступні задачі): relay-кадр `HandoffRequest`/маршрутизація (крок 1 протоколу), ws-рівень (виклик handoff/attach_resume з `agent-server::ws`, реплей `.nitra/session.jsonl` у нову `SessionHost`-сесію так, щоб клієнти бачили безшовне продовження), checkpoint-режим (дистильований summary замість повного журналу), lease-expiry takeover-шлях (уже покритий `renew`/CAS у claims.rs — тут лише кооперативний шлях).
