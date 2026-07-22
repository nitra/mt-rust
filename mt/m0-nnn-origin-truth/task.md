---
schema_version: 1
created_at: 2026-07-15T06:03:19Z
budget_sec: 3600
hint: atomic
---

## Mission

NNN наступного run обчислюється з ЛОКАЛЬНОГО дерева (mt-core runner preflight), а істина — origin/main: при розсинхроні runner повторно публікує run_NNN з тим самим номером і ПЕРЕЗАПИСУЄ immutable-файл на main (dogfood 2026-07-15: два коміти «run 003», перезапис run_004). NNN має рахуватись від стану worktree/base_sha (origin/main) після fetch — до створення claim.

## Done when

- preflight/run_node рахують NNN від origin/main (base_sha), не від локального дерева;
- тест: локальне дерево відстає на 2 run-и → новий run отримує наступний вільний NNN;
- `cargo test -p mt-core` зелений.

## Check

cargo test -p mt-core -q

## Context

crates/mt-core/src/runner.rs: preflight (nnn/attempt) і run_node (base_sha = rev-parse origin/main ПІСЛЯ обчислення nnn — інвертувати порядок або рахувати з worktree). Порушення інваріанта immutability graph.md.
