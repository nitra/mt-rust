---
type: ADR
title: Видалення n-flow rule та flow-модулів dispatcher
description: Застаріле правило `n-flow.mdc` і dispatcher flow-модулі видалено після переходу на graph-архітектуру MT.
---

**Status:** Accepted
**Date:** 2026-06-07

## Context and Problem Statement

Правило `n-flow.mdc` описувало попередній MT workflow і мало `alwaysApply: true`, тому продовжувало потрапляти в агентський контекст після переходу на нову graph-архітектуру, описану в `npm/docs/mt.md`.

У dispatcher також лишалися `flow-*.mjs` модулі під стару схему, зокрема `outputs_NNN.md`, тоді як graph-реалізація вже використовувала новий контракт із `fact_NNN.md`.

## Considered Options

- Залишити `n-flow.mdc` і `flow-*.mjs` паралельно з graph-архітектурою.
- Видалити повністю і перемістити `flow-verify.mjs` у `graph/lib/` як `cmd-verify.mjs`.
- Інші варіанти в transcript не обговорювалися.

## Decision Outcome

Chosen option: "Видалити повністю і перемістити `flow-verify.mjs` у `graph/lib/` як `cmd-verify.mjs`", because `cmd-plan.mjs` і `cmd-signals.mjs` уже покривали функціонал `flow-plan.mjs` та `flow-signals.mjs` у новій схемі, `flow-resolve.mjs` був мертвим кодом, а підтримка паралельної старої схеми `outputs_NNN.md` створювала б дублювання.

### Consequences

- Good, because `n-flow.mdc` більше не інжектує застарілі інструкції через `alwaysApply: true`.
- Good, because dispatcher більше не має `flow`-файлів після видалення `dispatcher/lib/docs/flow-lock.md` і `dispatcher/lib/docs/flow-resolve.md`.
- Good, because transcript фіксує, що 61 тест проходить після рефакторингу, включно з `cmd-verify.test.mjs`.
- Bad, because transcript не містить підтверджених негативних наслідків.
- Neutral, because `dispatcher/lib/nnn.mjs` і `state-store.mjs` лишилися окремою старою utility/state-store областю, але не як `flow`-іменовані модулі.

## More Information

Видалено:

- `npm/rules/flow/flow.mdc`
- `.cursor/rules/n-flow.mdc`
- `dispatcher/lib/flow-plan.mjs`
- `dispatcher/lib/flow-signals.mjs`
- `dispatcher/lib/flow-resolve.mjs`
- `dispatcher/lib/flow-verify.mjs`
- `dispatcher/lib/tests/flow-plan.test.mjs`
- `dispatcher/lib/tests/flow-signals.test.mjs`
- `dispatcher/lib/tests/flow-resolve.test.mjs`
- `dispatcher/lib/tests/flow-verify.test.mjs`
- `dispatcher/lib/docs/flow-lock.md`
- `dispatcher/lib/docs/flow-resolve.md`

Створено або оновлено:

- `dispatcher/graph/lib/cmd-verify.mjs` — verify під схему `fact_NNN.md` і `latestFactNNN` з `graph/lib/nnn.mjs`.
- `dispatcher/graph/lib/tests/cmd-verify.test.mjs`.
- `dispatcher/index.mjs` — імпорти з `graph/lib/cmd-plan.mjs`, `graph/lib/cmd-verify.mjs`, `graph/lib/cmd-signals.mjs`.
- `.n-cursor.json` — `flow` прибрано з активних правил і використано як disabled safeguard під час sync.

Команди й факти transcript:

- `npx @nitra/cursor` підтвердив видалення `.cursor/rules/n-flow.mdc` як правила поза списком.
- `find ... dispatcher -name "*flow*"` після cleanup не повернув файлів.
- `npx @nitra/cursor change` створив `npm/.changes/pr-20260607-1946.md`.
