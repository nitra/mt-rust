---
schema_version: 1
created_at: 2026-07-15T04:47:45Z
budget_sec: 3600
hint: atomic
---

## Mission

`## Check`-гейт перед done інтерактивного run: на `DoneSession` хост ганяє команди секції `## Check` вузла через `mt_core::signal::run_check` у worktree run-а; невдача → відмова сигналу (Event::Error у кімнату), run лишається живим для виправлення. Контракт graph.md: «## Check ганяється wrapper-ом перед done/audit; fail → відмова сигналу».

## Done when

- `DoneSession` із невдалим `## Check` НЕ публікує fact і шле Error з причиною; успішний Check → штатний fenced publish;
- інтеграційний тест WS+graph: вузол із `## Check` false → done відхилено; true → done проходить;
- `cargo test --workspace` зелений.

## Check

cargo test -p agent-server -q

## Context

Точка інтеграції: crates/agent-server/src/ws.rs (обробка DoneSession) + crates/agent-server/src/graph.rs; run_check — crates/mt-core/src/signal.rs.
