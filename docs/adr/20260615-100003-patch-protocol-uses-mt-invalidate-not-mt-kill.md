## ADR Patch protocol використовує `mt invalidate` замість `mt kill`

## Context and Problem Statement
Patch protocol для вузлів з нащадками використовував `mt kill` для successors, але `mt kill` виконує `git rm -r` і видаляє topology — після цього restart каскаду неможливий без повторної матеріалізації вузлів.

## Considered Options
* `mt kill` для successors (поточний підхід)
* `mt stop` + `mt invalidate` для successors
* `mt invalidate` для successors (інтегрований stop)

## Decision Outcome
Chosen option: "`mt invalidate` для successors", because `mt kill` видаляє topology (`git rm -r`) що унеможливлює restart каскаду; `mt invalidate` скидає execution state зберігаючи `task.md`, `a.md/h.md`, `deps/`, `plan_*`.

### Consequences
* Good, because topology зберігається — restart каскаду відбувається автоматично без повторного `mt spawn --approve`.
* Good, because `mt kill` тепер семантично чіткий — виключно для постійного видалення topology.
* Bad, because transcript не містить підтверджених негативних наслідків.

## More Information
`mt kill` — тільки для остаточного видалення вузла та піддерева з topology. Engineer protocol також виправлено: `mt stop + mt invalidate` замість `mt kill` при патчуванні залежного вузла. Файл: `npm/docs/mt.md` рядки ~1476, ~1483.
