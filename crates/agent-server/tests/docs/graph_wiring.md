---
type: Rust Module
title: graph_wiring.rs
resource: crates/agent-server/tests/graph_wiring.rs
docgen:
  crc: ec604fa5
  model: openai-codex/gpt-5.4-mini
  score: 100
  judgeModel: openai-codex/gpt-5.4-mini
---

## Огляд

Файл інтегрує WS-сесії з graph-мостом для перевірки контракту між `UserMessage`, `DoneSession` і `ReleaseSession`. Тестовий контур працює на bare-репо як origin, використовує MockProvider-агент і реальний WS, щоб зафіксувати поведінку без втрати run ref: перше `UserMessage` прив’язує вузол, `DoneSession` запускає fenced publish, а `ReleaseSession` ставить сесію на паузу й звільняє вузол.

## Поведінка

1. Піднімає ізольоване середовище з bare-origin, робочим репозиторієм, вузлом `mt/demo` і WS-сервером із graph-мостом та scripted MockProvider.
2. Приймає перше `UserMessage` як момент захоплення вузла: сесія прив’язується до вузла, а хід агента стартує тільки після успішного attach.
3. Фіксує сесію в журналі окремого run ref, щоб історія звернення зберігалася незалежно від подальшого завершення або паузи.
4. Після завершення ходу через `DoneSession` публікує результат у `main` у fenced-режимі: службові refs прибираються, а `.nitra/` не потрапляє в публікацію.
5. Після `ReleaseSession` знімає блокування з вузла без втрати журналу: claim звільняється, run ref залишається доступним, і вузол можна знову захопити новим `UserMessage`.
6. Якщо вузол уже зайнятий іншим тримачем, завершує спробу помилкою `claim-lost` без виконання ходу.

## Гарантії поведінки

- (специфічних машинно-виведених гарантій немає)
