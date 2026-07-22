---
schema_version: 1
created_at: 2026-07-15T04:59:02Z
budget_sec: 600
hint: atomic
---

## Mission

Dogfood-smoke нового agent-шляху (підписочні CLI, ACP-канон): створи у директорії цього вузла файл `note.md` з одним рядком `acp smoke ok` і згенеруй fact поточної спроби.

## Done when

- `note.md` існує поряд із task.md і містить рядок `acp smoke ok`;
- записано `fact_NNN.md` поточної спроби з `## Summary`.

## Check

grep -q 'acp smoke ok' note.md

## Context

Перший повний цикл M0 після міграції на підписочні CLI (ADR 260713-2040/2110): перевіряємо claim → worktree → headless CLI → ## Check → fenced publish без ручного git.
