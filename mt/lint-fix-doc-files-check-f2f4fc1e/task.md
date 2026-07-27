---
schema_version: 1
created_at: 2026-07-27T09:04:53.060Z
budget_sec: 1800
audit: required
hint: atomic
---

## Task

Виправити порушення правила `doc-files` (concern `check`), які не закрила інлайн fix-драбина.

## Done when

- `doc-files` не повідомляє порушень у target-файлах (див. ## Check).

## Check

npx @7n/rules lint --no-fix --cwd ../.. doc-files

## Inputs

Target-файли:
- `crates/mt-core/src/lib.rs`
- `crates/mt-core/src/worktree.rs`
- `crates/mt-napi/build.rs`
- `crates/mt-napi/src/context.rs`
- `crates/mt-napi/src/graph.rs`
- `crates/mt-napi/src/lib.rs`
- `crates/mt-napi/src/plan.rs`
- `crates/mt-napi/src/worktree.rs`
- `crates/mt/src/commands/doctor.rs`
- `crates/mt/src/commands/graph.rs`
- `crates/mt/src/commands/lifecycle.rs`
- `crates/mt/src/commands/mod.rs`
- `crates/mt/src/commands/plan.rs`
- `crates/mt/src/commands/run.rs`
- `crates/mt/src/commands/signal.rs`
- `crates/mt/src/commands/task.rs`
- `crates/mt/src/commands/worktree.rs`
- `crates/mt/src/context.rs`
- `crates/mt/src/main.rs`
- `crates/mt/src/output.rs`
- `crates/mt/tests/cli.rs`
- `crates/mt/tests/common/mod.rs`
- `npm/mt-napi/index.mjs`
- `npm/mt-napi/native.mjs`
