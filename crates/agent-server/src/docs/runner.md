---
type: Rust Module
title: runner.rs
resource: crates/agent-server/src/runner.rs
docgen:
  crc: 169199c2
  model: openai-codex/gpt-5.4-mini
  score: 100
  issues: judge:inaccurate:0.98
  judgeModel: openai-codex/gpt-5.4-mini
---

## Огляд

Виконавці ходу інтерактивної сесії: `UserMessage` клієнта запускає хід, усі події ходу емітяться в сесію (Envelope збирає session host). Транспорт виконавця — **ACP (Agent Client Protocol)**: майбутній `AcpTurnRunner` підключає зовнішній підписочний CLI (claude / codex / cursor / pi) через ACP і мапить `permission-request` на `ApprovalRequest` (ADR `260713-2110`). Власного agent loop/provider у модулі немає.

## Поведінка

- **TurnRunner** — трейт одного ходу кімнати: емітить події ходу і повертає відповідь виконавця; `workdir` — робоча директорія ходу (worktree run-а).
- **TurnError** — текстова помилка ходу (транспорт/виконавець повідомляє причину).
- **ScriptedTurnRunner** — скриптований виконавець для тестів транспорту/сесій: на кожен хід віддає наступний текст зі скрипту (`AgentTextDelta` + `AgentTextDone`), без LLM.
- **EchoTurnRunner** — заглушка без LLM: віддзеркалює текст користувача; для demo `attach` без підключеного ACP-виконавця.

## Гарантії поведінки

- Модуль не пише у ФС/БД; помилки ходу повертаються значенням `TurnError`, не панікою.
