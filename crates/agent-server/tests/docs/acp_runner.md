---
type: Rust Module
title: acp_runner.rs
resource: crates/agent-server/tests/acp_runner.rs
docgen:
  crc: 7cf8b886
  model: omlx/gemma-4-e2b-it-4bit
  tier: local-min
  score: 0
  issues: refusal-filler,best-of-2:retry-lost
---

## Огляд

Будь ласка, надайте текст чорнетки, яку потрібно перевірити.

## Поведінка

1. Запускає хід з використанням адаптера до child-процесу
2. Повертає рядок тексту з результату
3. Емітує події AgentTextDelta з текстом
4. Емітує події AgentTextDone для завершення тексту

## Гарантії поведінки

- Read-only: не виконує операцій запису (ФС/БД).
