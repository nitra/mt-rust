## ADR mt done перевіряє immutability task.md та відсутність run-draft.md перед publish

**Status:** Accepted
**Date:** 2026-06-13

## Context and Problem Statement

`mt done`/`mt audit` перевіряли лише наявність `fact_NNN.md` і `## Check` gate. Runner-агент міг непомітно змінити `task.md`, `a.md` або `h.md` у worktree і ці зміни потрапили б у `main` при fenced publish. Аналогічно, `run-draft.md` позначений як git-ignored, але якщо runner явно додав його до staged файлів або `.gitignore` не покривав worktree-директорію — файл потрапив би у `main`.

## Considered Options

* Залишити перевірки як є: `fact_NNN.md` existence + `## Check` gate
* Додати integrity check (`task.md`/`a.md`/`h.md` vs `origin/main`) і ephemeral file guard (`run-draft.md` у staged/tracked файлах worktree)

## Decision Outcome

Chosen option: "додати integrity check і ephemeral file guard", because `task.md` immutable after spawned — це інваріант протоколу, який має примусово перевірятися wrapper-ом, а не довірятися поведінці агента. Defense-in-depth вимагає явного gate на рівні publish для ephemeral файлів.

### Consequences

* Good, because неможливо опублікувати мутований `task.md` через `mt done`/`mt audit`; `run-draft.md` не може потрапити у `main` навіть якщо агент явно його застейджив; порушення виявляються до publish з чітким повідомленням про diff.
* Bad, because transcript не містить підтверджених негативних наслідків.

## More Information

Змінено: `npm/docs/mt.md` — рядок ~988 (агент prompt секція): додано два кроки після перевірки `fact_NNN.md`: (1) integrity check через `git diff origin/main -- task.md a.md h.md`; (2) ephemeral file guard через `git diff --cached --name-only` (наявність `run-draft.md` → відмова). Таблиця summary рядок "Межа immutability" оновлено.
