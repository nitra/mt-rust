---
schema_version: 1
created_at: 2026-07-15T04:47:45Z
budget_sec: 5400
hint: atomic
---

## Mission

Наскрізний mid-run approval через ACP (замінює вбитий m2-tool-approval-policy, що описував видалений власний tool-гейт): fake ACP-агент шле `session/request_permission` → хост шле `ApprovalRequest` у кімнату → верифікований `ApprovalResponse` (Ed25519; dev-політика без relay — непідписаний приймається) → агент отримує allow/reject-option → `ToolResult { ok }` у стрічці. Таймаут approve (120s) → відмова.

## Done when

- fake-acp-agent (crates/agent-server/src/bin/fake-acp-agent.rs) розширено сценарієм request_permission;
- інтеграційний WS-тест: approve → ToolResult ok:true; deny → ok:false; таймаут → відмова без падіння ходу;
- `cargo test --workspace` зелений.

## Check

cargo test -p agent-server -q

## Context

PermissionHandler-ланцюг: acp.rs handle_agent_request → AcpTurnRunner permission_factory → agent-cli request_approval (approvals_gate). Unit-рівень уже покрито в agent-core::acp::tests::permission_request_routes_through_handler.
