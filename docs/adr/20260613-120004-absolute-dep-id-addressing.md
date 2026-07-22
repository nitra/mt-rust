## ADR dep-id завжди абсолютний від tasks-root

**Status:** Accepted
**Date:** 2026-06-13

## Context and Problem Statement

`npm/docs/mt.md` описував "сусідній dep" як `deps/collect-data.md` з dep-id = `collect-data`. У вузлі `quarterly-anomalies/analyze` це резолвилося б у `mt/collect-data` (кореневий рівень), а не `mt/quarterly-anomalies/collect-data` (фактичний сусід). Адресація була неоднозначною для вкладених вузлів і ламалась при посиланнях вгору по ієрархії.

## Considered Options

* Відносна адресація: шлях у `deps/` відносно поточного вузла (`collect-data.md` → сусід, `../research.md` → батько)
* Абсолютна адресація: шлях файлу відносно `deps/` директорії (без `.md`) = dep-id = абсолютний шлях від `tasks-root`

## Decision Outcome

Chosen option: "абсолютна адресація від tasks-root", because відносна адресація ламається для посилань вгору через `../..` нотацію яка нагадує path traversal і вимагає знати глибину вузла. Абсолютна: `ls -R deps/` дає повний перелік — parser не потребує знати де знаходиться поточний вузол; `deps/` дзеркалює структуру `mt/`.

### Consequences

* Good, because однозначна адресація для будь-якого вузла незалежно від рівня (кореневий, сусід, батько, дочірній, будь-який); parser простий; cross-level deps вже так і працювали.
* Bad, because transcript не містить підтверджених негативних наслідків.

## More Information

Змінено: `npm/docs/mt.md` — рядок ~148 (опис dep-id addressing: "завжди абсолютний від tasks-root"), секція `deps/` (~371–385, приклад для `quarterly-anomalies/analyze` з абсолютними шляхами), scenario summary (~1733, виправлено `deps/collect-data.md` → `deps/quarterly-anomalies/collect-data.md`).
