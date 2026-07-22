## ADR Deferred differential cascade як поведінка mt invalidate за замовчуванням

**Status:** Accepted
**Date:** 2026-06-13

## Context and Problem Statement

`mt invalidate` рекурсивно архівував version chain всіх descendants одразу (eager cascade). Але специфікація одночасно стверджувала що після re-run descendants можна залишити `resolved` якщо content-addressed hash не змінився — що логічно неможливо: їх `fact_*.md` вже заархівовані eager cascade і видалені з робочого дерева. Суперечність унеможливлювала реальну диференційну оптимізацію.

## Considered Options

* Eager cascade: архівувати всіх descendants одразу (поточна поведінка)
* Deferred cascade: архівувати лише target вузол; нащадки природно стають `blocked` через відсутність resolved upstream; cascade запускається тільки після re-run і hash-порівняння

## Decision Outcome

Chosen option: "deferred cascade як default", because eager cascade завжди >= deferred за обсягом роботи: якщо hash однаковий — нащадки не чіпаються; якщо різний — виконується той самий cascade. Eager ніколи не краще. Окремий стан `blocked-stale` не потрібен: стандартний `blocked` достатній через природну відсутність resolved upstream.

### Consequences

* Good, because усунено суперечність у специфікації; зекономлено re-execution нащадків при незмінному upstream-результаті; `mt kill` зберігає eager cascade (там hash-порівняння безглузде).
* Bad, because transcript не містить підтверджених негативних наслідків.

## More Information

Змінено: `npm/docs/mt.md` — секція `mt invalidate` (~883–890, прибрано рядок про recursive cascade нащадків), секція "Каскад інвалідації" (схема з двома гілками: однаковий hash → розблокування, різний hash → cascade), таблиця summary (рядок "Інвалідація").
