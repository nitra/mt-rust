---
schema_version: 1
created_at: 2026-07-11T15:54:04Z
budget_sec: 7200
hint: atomic
---

## Mission

Міст agent-server ↔ граф: інтерактивний run вузла (`mt attach`-семантика з runtime.md) поверх `mt-core` — attach (CAS claim + detached worktree від base_sha + push run ref), коміт ходу (файли + `session.jsonl` → push run ref), renewal lease, `done` (fenced publish зі стрипом `.nitra/`), release (пауза/відпустити claim). Без реімплементації контракту: всі graph-операції — виклики `mt-core` (та сама реалізація, що її використовує `@7n/mt` через napi).

## Done when

- модуль `graph` у `crates/agent-server`: `GraphBridge::attach(node)` → `InteractiveRun { node_hash, token, claim_sha, base_sha, worktree }`;
- другий attach того самого вузла → явна відмова claim-lost (CAS, не помилка транспорту);
- `commit_turn`: пише `.nitra/session.jsonl` у worktree, комітить і пушить run ref (recovery/handoff);
- `renew`: подовжує lease тим самим token/generation (CAS від попереднього claim SHA);
- `done`: перед fenced publish прибирає `.nitra/` з індексу (інваріант git.md — `.nitra/` ніколи не потрапляє у main) → publish просуває `main`, видаляє claim/run ref;
- `release`: CAS-delete claim + прибирає worktree (пауза без publish);
- тести на герметичній фікстурі (bare-репо як origin, як у mt-core test_support): attach/claim-lost/commit_turn/done/release, включно з перевіркою, що `.nitra/` немає у `main`;
- `cargo test -p agent-server` зелений; без tauri.

## Context

- Нормативні джерела: npm/docs/architecture/runtime.md («Інтерактивна сесія = run вузла»), git.md (claim CAS, run ref і журнал сесії, fenced publish, `.nitra/` поза main), stack.md (правило одного коду контракту — реалізація в mt-core).
- Використати: `mt_core::claims` (acquire/renew_or_takeover/release, node_hash), `mt_core::worktree` (create_run_worktree, push_run_ref, remove_run_worktree), `mt_core::publish::fenced_publish`.
- Поза скоупом: протокольна команда done/detach від клієнта (розширення Event — окрема задача), автоматичний renewal-цикл у serve, `interactive:`-поле у `.mt-claim.yml` (0.3.0-дельта схеми — окремий ADR).
