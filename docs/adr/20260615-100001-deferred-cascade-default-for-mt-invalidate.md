## ADR Deferred cascade як поведінка за замовчуванням для `mt invalidate`

## Context and Problem Statement
`mt invalidate` рекурсивно архівував version chain усіх нащадків одразу (eager cascade), але пізніше документ стверджував що нащадки можуть залишитись `resolved` якщо hash не змінився — суперечність, бо їхні facts вже заархівовані.

## Considered Options
* Eager cascade (поточний підхід): архівувати нащадків одразу при `mt invalidate`
* Deferred cascade як default: архівувати тільки target-вузол, нащадки природно стають `blocked`
* `--defer-cascade` як окремий флаг (рекомендація рев'юера)

## Decision Outcome
Chosen option: "Deferred cascade як default", because eager cascade ніколи не краща за deferred — вона або рівна (hash змінився), або зайво знищує роботу (hash не змінився); нащадки природно стають `blocked` коли upstream не `resolved`, їх facts залишаються нетронутими.

### Consequences
* Good, because differential cascade тепер можливий — однаковий hash після re-run означає що нащадки не потребують повторного виконання.
* Good, because `mt kill` явно зберігає eager cascade для випадків постійного видалення topology.
* Bad, because transcript не містить підтверджених негативних наслідків.

## More Information
`mt kill` — завжди eager cascade (знищення topology). `mt invalidate` — тільки target-вузол; cascade відкладається до hash-порівняння після re-run. Файл: `npm/docs/mt.md` рядки ~883, ~1397.
