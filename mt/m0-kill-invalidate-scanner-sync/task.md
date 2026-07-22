---
schema_version: 1
created_at: 2026-07-15T04:47:45Z
budget_sec: 3600
hint: atomic
---

## Mission

Розсинхрон invalidate-семантики: JS `mt kill` писав sentinel `invalidated`, якого Rust-сканер (mt-core scan) не знає — вузол лишався `waiting`. Kill вузла вже мігровано на `mt_core::lifecycle::kill` (napi `killNode`, 2026-07-15); лишилась каскадна інвалідація залежних (kill.mjs крок 4 досі пише sentinel) і рішення: або сканер вчить стан `invalidated`, або каскад мігрує на mt-core з іншою семантикою (blocked-by-missing-dep). Узгодити з graph.md і зафіксувати ADR-ом.

## Done when

- Сканер і kill користуються однією семантикою інвалідації (одна імплементація в mt-core);
- тести: kill вузла з залежними → залежні мають узгоджений derived-стан (не waiting);
- graph.md оновлено; `cargo test --workspace` і `npx vitest run` зелені.

## Check

cargo test -p mt-core -q

## Context

kill.mjs крок 4 (каскад, sentinel `invalidated`); mt-core: lifecycle.rs kill, scan — стани derived. Виявлено при кураторстві графа 2026-07-15.
