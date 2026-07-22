# Конвеєрна оркестрація docgen: детермінований JS-оркестратор + локальна LLM

**Status:** Accepted
**Date:** 2026-06-06

## Context and Problem Statement
One-shot генерація документації локальними моделями (`gemma3:4b`, `gemma4:4b`) демонструвала три стабільних класи помилок: витік деталей реалізації (stdlib, regex, приватні імена), галюцинації в секції «Гарантії поведінки» та пропуск крайових деталей. Причина — модель одночасно відповідальна за факти, структуру і прозу; ці класи не залежали від транспорту. Окремо: бенчмарк A/B/C (A: прямий ollama без system ~71%; B: через pi ~87%; C: прямий + system-prompt ~85%) встановив, що різниця B vs C у межах шуму ±3 п.п. — якість визначає system-prompt, не транспорт.

## Considered Options
- One-shot промпт із посиленим system-prompt (варіанти A, B, C2 з бенчмарків).
- Конвеєрна оркестрація: детермінований JS-екстрактор (Stage 0) + точкові LLM-промпти на секцію (Stage 1) + детермінована зборка (Stage 3).

## Decision Outcome
Chosen option: "Конвеєрна оркестрація з JS-оркестратором (`docgen-gen.mjs`) як входною точкою", because JS-оркестратор усуває три класи помилок детерміновано: Stage 0 витягує імена, skip-маркери, readOnly/throwsErrors без LLM-токенів, а модель отримує лише вузьку задачу «перефразуй ці факти». Benchmark v2 підтвердив: оркестрована `gemma3:4b` ~86% vs one-shot ~80% (+6 п.п.), часово конкурентна (overlay 77с vs 61с, k8s 31с vs 45с).

### Consequences
- Good, because Stage 0 (`docgen-extract.mjs`) детерміновано витягує JSDoc, класифікує stdlib vs internal, маркери `readOnly`/`catchesErrors`/`skips`/`caches` — заземлення без жодного LLM-токена прибирає галюцинації й витоки.
- Good, because `gemma3:4b` + оркестрація (~86%) наближається до `gemma4:4b` one-shot (~92%), лишаючись у GPU (3.3 GB, ~20 tok/s) без офлоаду.
- Good, because KV-cache ollama на стабільному system+код префіксі амортизує секційні виклики в межах одного файлу.
- Bad, because реалізація складніша: `docgen-extract.mjs` + `docgen-prompts.mjs` + `docgen-gen.mjs` + зборка — більше точок відмови.
- Bad, because Stage 0 прив'язаний до JS/MJS; `.vue`/`.py` деградують до one-shot fallback.
- Neutral, because v1 оркестрації (код у всіх секціях) була 3–5× повільніша — виправлено у v2 (код лише в секцію «Поведінка»).

## More Information
Файли в `.worktrees/feat-docgen-orchestrator-pi/npm/skills/docgen/js/`: `docgen-extract.mjs` (Stage 0, детермінований парсер), `docgen-prompts.mjs` (Stage 1, секційно-мінімальний контекст), `docgen-gen.mjs` (Stage 2–3, входна точка; режим `--oneshot` для AB-порівняння). Гілка `feat/docgen-orchestrator-pi`. Моделі: `gemma3:4b` (3.3 GB, 100% GPU, ~20 tok/s); `gemma4:4b` q4 — alias `batiai/gemma4-e4b:q4` (~6.2 GB, 56%/44% CPU/GPU, ~11 tok/s), скопійований через `ollama cp` (спільний blob, 0 місця). Виявлений баг: `gemma4:4b` через `/api/chat` кладе вихід у `j.response`, а не `j.message.content` — фікс: `j.message?.content || j.response || ''`. Транспорт pi залишено за ергономікою (read/write tools, sessions), а не якісною перевагою. Критерій придатності моделі на 8 GB: розмір ≤ ~4.2 GB (100% GPU) або q4-квантизація ≤ 6.5 GB (допустимий частковий офлоад без своп-деградації).

## Update 2026-06-06

Первісна мотивація переходу на конвеєрну оркестрацію: три стабільних класи помилок one-shot генерації (незалежно від транспорту ollama vs pi): (1) витік деталей реалізації (stdlib, regex, приватні імена); (2) галюцинації у секції «Гарантії поведінки»; (3) пропуск крайових деталей (наприклад «корінь не перевіряється»). Усі три класи є стелею архітектури «один промпт» і не усуваються поліпшенням system-prompt.

Транспорт pi не дає якісної переваги над прямим `/api/chat` + system-prompt для конвеєрного режиму — збережено за ергономікою (read/write tools, sessions). Гілка реалізації: `.worktrees/feat-docgen-orchestrator-pi` (`feat/docgen-orchestrator-pi`).

## Update 2026-06-06

### Вибір транспорту: прямий `ollama /api/chat` з явним system prompt

Обрано прямий виклик `POST /api/chat` замість CLI-утиліти `pi`. Бенч показав різницю між pi (87%) і прямим+system (85%) у межах ~2 п.п., тоді як різниця між «без system» і «з system» — системна (+15 п.п.). Приріст pi над прямим викликом дає виключно вбудований system-prompt, а не архітектурна перевага. Усувається ~4 с overhead node-старту pi на файл (≈+1.1 год на 1042 файли). pi RPC-режим (`--mode rpc`) для амортизації старту виявився непридатним: персистентна сесія між незалежними файлами накопичує контекст.

- Бенч-скрипти: `/tmp/docgen-bench3/run.py`, `~/docgen-bench3/duel.py`, `~/docgen-bench3/confirm.py`
- pi-конфіг провайдера: `~/.pi/agent/models.json`; overhead: ~3.8–6.4 с/виклик

### Вибір моделі Ollama для Tier 1 docgen на Mac M2 8 GB RAM

Обрано обидві моделі з різними сценаріями застосування:

- `gemma3:4b` (3.3 GB, 100% GPU, ~20 tok/s, ~85% якості) — для швидких/чорнових прогонів
- `gemma4:4b` alias `batiai/gemma4-e4b:q4` (5.3 GB, 56%/44% CPU/GPU, ~11 tok/s, ~92%) — для якість-first генерації, де документацію читатимуть люди

`gemma4:e4b` full (9.6 GB) відхилено одразу: своп, ~0.4 tok/s. Ollama alias: `ollama cp batiai/gemma4-e4b:q4 gemma4:4b`. Вимірювання: `~/docgen-bench3/g4.py` — RAM 6.2 GB, cold-load 14 с. `gemma4:4b` при частковому офлоаді дає ~2× більший час і нестабільний throughput при конкуренції за пам'ять.
