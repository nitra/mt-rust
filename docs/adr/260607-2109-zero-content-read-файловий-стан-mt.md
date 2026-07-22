---
type: ADR
title: Zero-content-read файловий стан MT
description: Стани вузлів MT визначаються через імена та наявність файлів без читання їхнього вмісту.
---

**Status:** Accepted
**Date:** 2026-06-07

## Context and Problem Statement

Оркестратор `mt watch` і `mt run --auto` сканують граф задач. Частина попереднього дизайну вимагала читати frontmatter `task.md`, `run_NNN.md` або `deps:` для визначення стану, залежностей і mode. Це суперечило бажаному інваріанту: стан має визначатися через directory listing, імена файлів та наявність директорій.

Також виникло питання, чи може `run_NNN.md` із `status: done|failed` замінити окремий `fact_NNN.md`.

## Considered Options

- `run_NNN.md` з `status: done|failed` замінює `fact_NNN.md` і fail-артефакт.
- Зберегти `fact_NNN.md` як sentinel успіху, а `run_NNN.md` використовувати для журналу спроб.
- Читати frontmatter `task.md` для `mode` і `deps:`.
- Перекодувати стан-визначальну інформацію в імена файлів, sentinel-файли та директорії.
- Для stalled: implicit detection через mtime worktree і budget-поля.
- Для stalled: sentinel-файл `stalled`.
- Для stalled: timestamp у `run_NNN.md`.
- Для stalled: `running_until_<ts>` з deadline у назві.
- Для mode: `task_h.md`/`task_a.md`.
- Для mode: стабільний `task.md` плюс `a.md`/`h.md` sentinel-прапори.
- Для залежностей: `deps:` у frontmatter `task.md`.
- Для залежностей: директорія `deps/`, де кожен файл відповідає одному dep-вузлу.

## Decision Outcome

Chosen option: "Зберегти `fact_NNN.md` як sentinel успіху і перекодувати стан у файли/директорії", because transcript фіксує інваріант zero-content-read: `ls` має бути достатньо для визначення станів вузла, залежностей і mode без читання frontmatter або body файлів.

### Consequences

- Good, because `resolved` визначається наявністю `fact_*.md`, без читання `run_*.md` і пошуку `status: done` серед N спроб.
- Good, because `stalled` визначається parse назви `running_until_<ts>`: `ts > now()` означає `running`, `ts <= now()` означає `stalled`.
- Good, because `task.md` лишається стабільним, а mode змінюється через mutable sentinel-прапори `a.md`/`h.md`, не руйнуючи git history основного task-файлу.
- Good, because `ls deps/` дає список залежностей без читання `task.md`.
- Bad, because transcript не містить підтверджених негативних наслідків для zero-content-read інваріанту.
- Neutral, because відсутність `a.md` і `h.md` створює окремий корисний стан `setup`/`unassigned`, але transcript не містить повного опису його lifecycle.

## More Information

Файли й контракти:

- `tasks/<node>/fact_NNN.md` — sentinel успішного виконання.
- `tasks/<node>/run_NNN.md` — журнал спроби; `run_NNN.md` без `fact_NNN.md` і без активного worktree трактувався як failed у transcript.
- `tasks/<node>/running_until_<unix-timestamp>` — git-ignored sentinel deadline; відсутність `running_until_*` при наявному worktree є safe fallback до orphan/stalled.
- `tasks/<node>/task.md` — стабільний основний файл місії.
- `tasks/<node>/a.md` — mutable sentinel для agent-mode; frontmatter може містити `model_tier`, `skills`.
- `tasks/<node>/h.md` — mutable sentinel для human-mode; frontmatter може містити `qualification`.
- `tasks/<node>/deps/<dep-node-id>.md` — один файл на одну залежність; імʼя дає dep-ID, вміст може містити `ref:` і додатковий контекст для агента.

Документ, у якому це фіксувалося: `npm/docs/mt.md`.

## Update 2026-06-07

- Зафіксовано дилему `run_NNN.md` як єдиного артефакта проти окремого success-sentinel: якщо `run_NNN.md` містить `status: success | failed`, scanner мусить читати frontmatter, а стан більше не визначається лише існуванням файлу.
- Цей аргумент став підставою для збереження окремого `fact_NNN.md` як sentinel успішного виконання.

## Update 2026-06-07

- `outputs_NNN.md` перейменовано на `fact_NNN.md`, щоб утворити семантичну пару `plan_NNN.md` / `fact_NNN.md`: план проти факту виконання.
- У `task.md`/плануванні додано оцінки виконавця: `executor: agent|human`, `model_tier: MIM|AVG|MAX`, `skills`, `qualification`; `plan_NNN.md` може override-ити базові значення.
- Нумерація `plan_NNN.md` продовжується для merged/active worktree, але після `mt kill` дозволено reset до `001`, бо `mt kill` видаляє `plan_*.md` як повний reset вузла.
- `mt watch` на стартовому етапі може бути periodic rescan раз на 5 хвилин, використовуючи той самий scan-код, що й `--auto`; daemon/file-watching і Telegram alerts лишаються TODO.
- Попередній implicit-підхід до stalled було зафіксовано як варіант, але пізніше замінено explicit `running_until_<ts>` sentinel-файлом.

## Update 2026-06-07

- Стан `stalled` визначається через git-ignored sentinel `running_until_<unix_ts>` у директорії вузла.
- `running` = sentinel існує і `ts > now()`, `stalled` = sentinel існує і `ts <= now()`; визначення виконується filename parse без читання вмісту.
- Wrapper пише sentinel після створення worktree і видаляє його під час success/failure cleanup; `mt kill` також видаляє `running_until_*`.
