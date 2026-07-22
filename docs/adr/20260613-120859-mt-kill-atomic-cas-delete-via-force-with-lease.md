# mt kill: атомарне CAS-видалення claim через force-with-lease

**Status:** Accepted
**Date:** 2026-06-13

## Context and Problem Statement

`mt kill` виконував перевірку claim (крок 1) і CAS-delete (крок 3) як окремі, не атомарні операції. Між ними інший runner міг захопити claim — тоді CAS-delete знищував чужий claim, залишаючи нового власника без lease.

## Considered Options

* Зберегти двокроковий check + delete з retry-логікою
* Зробити кроки 1 і 3 атомарними через `git push --force-with-lease`
* Інші варіанти в transcript не обговорювалися.

## Decision Outcome

Chosen option: "force-with-lease для CAS-delete claim", because `git push --force-with-lease=refs/mt/claims/<hash>:<expected-sha> origin :refs/mt/claims/<hash>` атомарно перевіряє expected SHA і видаляє ref в одній операції — ідентично механізму direct publish. Якщо claim змінився між check і push — rejected non-fast-forward, kill безпечно завершується з помилкою.

### Consequences

* Good, because неможливо випадково видалити чужий claim у distributed середовищі.
* Bad, because transcript не містить підтверджених негативних наслідків.
* Neutral, because зауваження надійшло як пункт 6 code review; зміни у документ `npm/docs/mt.md` на момент завершення transcript не були внесені — рішення лише проаналізовано.

## More Information

Файл: `npm/docs/mt.md`, секція `mt kill`. Команда для атомарного CAS-delete: `git push --force-with-lease=refs/mt/claims/<hash>:<expected-sha> origin :refs/mt/claims/<hash>`. Рекомендацію надав колега-рецензент.
