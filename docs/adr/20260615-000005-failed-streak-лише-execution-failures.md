# failed_streak рахує лише execution failures

**Status:** Accepted
**Date:** 2026-06-15

## Context and Problem Statement

Лічильник `failed_streak` відстежує послідовні невдачі вузла для прийняття рішень про ескалацію. Попередня реалізація включала до підрахунку події `decomposed` (lifecycle-перехід) та `claim-lost` (ownership-подія), що могло спричинити хибне обмеження нормальних операцій. Формула обчислення також мала помилку: використовувала арифметичний `max(run NNN) - max(fact NNN)` замість читання поля `result:` з frontmatter.

## Considered Options

* Рахувати всі типи невдач (failed, decomposed, claim-lost, progress-timeout, budget-exceeded, merge-conflict)
* Рахувати лише execution failures: failed, progress-timeout, budget-exceeded, merge-conflict

## Decision Outcome

Chosen option: "Лише execution failures", because `decomposed` — це нормальний lifecycle-перехід, а `claim-lost` — ownership-подія; включення їх у streak призводить до помилкового обмеження. Нескінченні decompose-цикли обмежуються `budget_total_sec`, а не streak. Формула виправлена: читати `result:` з frontmatter `run_NNN.md`.

### Consequences

* Good, because усунуто хибні спрацювання streak-ескалації через lifecycle та ownership-події.
* Good, because формула стає детермінованою та простою: одне поле з frontmatter замість арифметичної різниці.
* Bad, because transcript не містить підтверджених негативних наслідків.

## More Information

- `npm/docs/mt.md` ~lines 515-535 — змінено визначення `failed_streak`: виключено `decomposed` та `claim-lost`.
- `npm/docs/mt.md` ~lines 740-755 — виправлено формулу: `result:` field з `run_NNN.md` frontmatter замість `max(run NNN) - max(fact NNN)`.
- Events що рахуються: `failed`, `progress-timeout`, `budget-exceeded`, `merge-conflict`.
- Events що не рахуються: `decomposed` (lifecycle), `claim-lost` (ownership).
