---
schema_version: 1
created_at: 2026-07-15T06:03:19Z
budget_sec: 3600
hint: atomic
---

## Mission

Retry ladder ескалює лише model_tier, але не виконавця: слабка локальна модель (agent_cli: pi + 2B) може не пройти вузол ніколи — 7 failed-ранів поспіль без ескалації CLI (dogfood 2026-07-15; вузол пройшов лише після ручного перемикання на codex). Додати у драбину крос-CLI ескалацію: фінальний щабель (або N-та невдача поспіль) перемикає на наступний CLI з MT_CLOUD_AGENT_CLIS — узгодити з graph.md «Retry ladder» і зафіксувати ADR-ом.

## Done when

- Щабель драбини може нести зміну agent_cli (напр. escalate-cli) або після вичерпання драбини runner пробує наступний CLI каскаду;
- тести драбини+каскаду; graph.md/runtime.md оновлені; ADR записано;
- `cargo test -p mt-core` зелений.

## Check

cargo test -p mt-core -q

## Context

crates/mt-core/src/runner.rs: default_retry_ladder/resolve_retry_step/cascade_order. Каскад зараз спрацьовує лише на rate-limit; невдачі якості моделі не ескалюють CLI.
