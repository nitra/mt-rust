# mt cleanup — окрема CLI-команда для очищення orphan worktrees

**Status:** Accepted
**Date:** 2026-06-13

## Context and Problem Statement

Після failed runs worktrees залишались у `.worktrees/` для debug. Без явного cleanup-механізму при частих падіннях директорія накопичувала сотні worktrees. Єдиним GC-механізмом був `mt watch`, але він може не запускатись у CI/CD або single-run environments.

## Considered Options

* Тільки автоматичне очищення всередині `mt watch`
* Окрема команда `mt cleanup [--older-than N]` плюс виклик з `mt watch`

## Decision Outcome

Chosen option: "окрема команда `mt cleanup` плюс виклик з `mt watch`", because `mt watch` може не запускатись у CI/CD або single-run environments; оператор повинен мати явний інструмент без залежності від watch.

### Consequences

* Good, because orphan worktrees не накопичуються в середовищах де watch не запущений.
* Bad, because transcript не містить підтверджених негативних наслідків.

## More Information

Файл `npm/docs/mt.md`; секція "mt cleanup"; CLI-список команд. Default `--older-than 7` (днів).
