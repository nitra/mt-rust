---
schema_version: 1
created_at: 2026-07-11T16:54:17Z
budget_sec: 10800
hint: atomic
---

## Mission

Зшити WS-сесії agent-server із graph-мостом — серцевина demo-критерію M1 («`mt attach <node>` відкриває чат із задачею; `mt done` публікує fact тим самим fenced publish»): перший `UserMessage` вузла робить graph-attach (CAS claim + worktree), кожен хід комітиться у run ref разом із журналом сесії, нові протокольні команди `DoneSession`/`ReleaseSession` (мінорне розширення Event v4 — канон runtime.md оновити), авто-renewal lease у фоні.

## Done when

- `agent-protocol`: нові client→host варіанти `DoneSession {}` і `ReleaseSession {}`; roundtrip-тести; runtime.md (канон) доповнено; старі клієнти сумісні (невідомий варіант ігнорується);
- `agent-server`: `AppState.graph: Option<GraphConfig>`; перший `UserMessage` вузла → `graph::attach` (невдача → `Event::Error` у сесію, хід не виконується); хід агента виконується з `workdir = worktree`; після ходу — `commit_turn` (журнал сесії + правки файлів → push run ref);
- `DoneSession` → strip `.nitra/` + fenced publish → `Committed { commit_hash }` у сесію; `ReleaseSession` → release claim → `ClaimChanged { holder_device_id: None }`;
- авто-renewal: фонова задача на attach (перiод ~lease/3); `renew == false` → `Error` claim-lost у сесію, run прибирається;
- `agent-cli` attach: `/done` і `/release` у REPL шлють відповідні команди;
- інтеграційний тест: git-фікстура + WS: UserMessage → claim ref зʼявився, run ref має журнал; DoneSession → main просунувся (без `.nitra/`), claim/run ref прибрані; ReleaseSession → вузол знову вільний;
- `cargo test --workspace` зелений; без tauri.

## Context

- Нормативні джерела: npm/docs/architecture/runtime.md («Інтерактивна сесія = run вузла», «Протокол подій» — мінорні розширення = нові Event-варіанти), git.md (журнал сесії у run ref, `.nitra/` поза main).
- Побудовано на: `agent_server::graph` (attach/commit_turn/renew/done/release, PR #28), `agent_server::session`/`ws` (PR #25).
- Поза скоупом: `## Check`-виконання перед done (потребує контракту виконання Check — окрема задача), handoff/relay (M2), `interactive:` у `.mt-claim.yml` (окремий ADR).
