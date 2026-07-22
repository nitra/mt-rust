## ADR `mt stop` не додається як окрема CLI-команда

## Context and Problem Statement
Потрібно було вирішити чи виносити логіку зупинки процесу і звільнення claim в окрему команду `mt stop`, чи інтегрувати її в `mt invalidate`.

## Considered Options
* `mt stop` як окрема CLI-команда (рекомендація рев'юера)
* Інтегрувати SIGTERM + CAS-delete claim як перший крок `mt invalidate`

## Decision Outcome
Chosen option: "Інтегрувати stop-логіку в `mt invalidate`", because конкретного сценарію де людині потрібен `mt stop` без подальшого `mt invalidate` або `mt kill` — не знайдено; інтеграція усуває клас помилок між окремими викликами.

### Consequences
* Good, because patch protocol потребує одного кроку (`mt invalidate`) замість двох (`mt stop` + `mt invalidate`).
* Good, because між stop і invalidate немає вікна для retake claim.
* Bad, because transcript не містить підтверджених негативних наслідків.

## More Information
`mt invalidate` на running вузлі: локальний runner → SIGTERM + CAS-delete claim перед архівацією; remote runner → CAS-delete claim (remote детектує втрату при наступному renewal). Файл: `npm/docs/mt.md`.
