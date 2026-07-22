---
schema_version: 1
created_at: 2026-07-15T04:47:45Z
budget_sec: 3600
hint: atomic
---

## Mission

Знайти й зафіксувати робочі ACP-адаптери для підписочних CLI (claude / codex / cursor / pi): спайк 2026-07-14 показав, що жоден із чотирьох CLI не має вбудованого ACP-режиму у `--help` — адаптери зовнішні (напр. claude-code-acp). Для кожного CLI визначити команду адаптера для env `MT_ACP_AGENT_CMD`, перевірити живою сесією `agent-cli serve --acp-cmd …` + `attach` (хід зі стрімом `AgentTextDelta`), задокументувати таблицю адаптерів у npm/docs/architecture/runtime.md.

## Done when

- Таблиця «agent_cli → команда ACP-адаптера» у runtime.md, перевірена живими сесіями щонайменше для двох CLI;
- Зафіксовані розбіжності реальних адаптерів із v1-підмножиною клієнта (crates/agent-core/src/acp.rs) — issues або правки клієнта;
- `cargo test --workspace` зелений.

## Check

cargo test -p agent-core -p agent-server -q

## Context

ACP-клієнт: crates/agent-core/src/acp.rs (initialize/session\/new/session\/prompt/request_permission, ndjson JSON-RPC); runner: crates/agent-server/src/runner.rs AcpTurnRunner; wiring: crates/agent-cli (`serve --acp-cmd`, env MT_ACP_AGENT_CMD). ADR 260713-2110.
