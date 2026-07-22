---
type: JS Module
title: llm.mjs
resource: layers/lib/llm.mjs
docgen:
  crc: 1f770193
  model: openai-codex/gpt-5.4-mini
  tier: cloud-min
  score: 100
  issues: judge:inaccurate:0.97
  judgeModel: openai-codex/gpt-5.4-mini
---

## Огляд

`createLlm` створює LLM-клієнт для одноразового отримання відповіді та повертає результат без побічних записів у ФС чи БД.

Для помилок, пов’язаних із цим шаром, використовується `LlmError`, щоб викликальний код міг відрізняти помилки LLM-взаємодії від інших збоїв.

`LlmError`  
`createLlm`

## Поведінка

- `LlmError` — помилка шару LLM із кодом причини `unavailable` або `output`, щоб відрізняти недоступність транспорту від невалідного результату моделі.
- `createLlm` — створює клієнт LLM із one-shot генерацією, валідацією виходу, ретраєм і підвищенням tier, а також завершує chain із підсумком прогону.

## Публічний API

- LlmError — помилка на рівні LLM-шару; `unavailable` означає, що немає доступного транспорту або моделі, а `output` — що відповідь моделі не пройшла приймання після всіх спроб.
- createLlm — створює один одноразовий LLM-прогін із retry та підвищенням tier; `llm-lib` підвантажує лише тоді, коли це справді потрібно, тому `status` його не чіпає.

Changelog: не перевірявся (потрібен `npx @nitra/cursor lint changelog`).

## Гарантії поведінки

- Read-only: не виконує операцій запису (ФС/БД).
