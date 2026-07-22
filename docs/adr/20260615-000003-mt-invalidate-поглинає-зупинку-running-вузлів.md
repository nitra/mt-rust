# mt invalidate поглинає зупинку running-вузлів

**Status:** Accepted
**Date:** 2026-06-15

## Context and Problem Statement

Якщо вузол перебуває у стані `running` під час виклику `mt invalidate`, необхідно було вирішити: зупиняти процес явно перед архівуванням чи вимагати окремого кроку `mt stop`. Також потрібно було переглянути протокол engineer-агента щодо роботи з вузлами, що виконуються.

## Considered Options

* Окремий `mt stop` перед `mt invalidate` (два-кроковий протокол)
* `mt invalidate` автоматично виконує SIGTERM + CAS-delete claim перед архівуванням (поглинання)

## Decision Outcome

Chosen option: "`mt invalidate` автоматично виконує SIGTERM + CAS-delete claim", because не виявлено самостійного use case для `mt stop` у людському сценарії — lifecycle вузла обмежений `budget_total_sec`; поглинання спрощує протокол і виключає розрив між зупинкою та архівуванням.

### Consequences

* Good, because протокол engineer-агента спрощується: `mt stop` + `mt invalidate` замінюється одним `mt invalidate` для dependency patches.
* Good, because виключається стан, коли claim існує після архівування вузла.
* Bad, because transcript не містить підтверджених негативних наслідків.

## More Information

- `npm/docs/mt.md` ~line 913 — додано логіку SIGTERM + CAS-delete claim у `mt invalidate` для running-вузлів.
- `npm/docs/mt.md` ~line 1476 — оновлено протокол engineer-агента: для dependency patches використовувати `mt stop` + `mt invalidate` замість `mt kill`.
- CAS-delete claim: compare-and-swap гарантує, що лише власник claim виконує видалення.
