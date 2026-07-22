---
type: ADR
title: "Окремий модуль task orchestration DAG у npm/scripts/graph"
description: Нову систему task orchestration DAG реалізуємо в окремому модулі, не переписуючи legacy dispatcher graph.
---

**Status:** Accepted
**Date:** 2026-06-06

## Context and Problem Statement

Існуючий `npm/scripts/dispatcher/graph.mjs` є read-only прототипом для `docs/graphs/<g>/nodes/*.md` і деривує статус з artifact-файлів `plan`, `claim`, `fact`, `ask`, `ans`. Документ `npm/docs/mt.md` описує іншу архітектуру: автономний DAG задач на основі `tasks/<node>/task.md`, worktree-ізоляції та lifecycle через сигнальні файли. Потрібно було вирішити, чи розширювати legacy dispatcher, чи створювати окрему реалізацію для нового формату.

## Considered Options

- Розширити існуючий `dispatcher/graph.mjs` з backward-compat шаром для нового формату.
- Створити окремий модуль `npm/scripts/graph/` і залишити старий `dispatcher/graph.mjs` незайманим.

## Decision Outcome

Chosen option: "Створити окремий модуль `npm/scripts/graph/`", because архітектури несумісні на рівні file layout і state machine: старий формат читає `docs/graphs/` з artifact-типами, новий читає `tasks/` і стан з присутності sentinel-файлів; пряма заміна зламала б існуючі тести `dispatcher/tests/graph.test.mjs` без практичної користі.

### Consequences

- Good, because старі тести `dispatcher/tests/` залишаються зеленими для legacy-реалізації.
- Good, because нова система отримує ізольований простір для власних тестів, зокрема `graph/tests/state.test.mjs`.
- Bad, because transcript фіксує потенційну плутанину: `mt` тимчасово має дві реалізації під різними route-ами до видалення legacy `dispatcher/graph.mjs`.
- Neutral, because transcript містить факт окремого top-level route для `watch`, але не містить підтвердження додаткових наслідків цього рішення.

## More Information

- Нові файли, згадані в transcript: `npm/scripts/graph/config.mjs`, `state.mjs`, `scan.mjs`, `setup.mjs`, `init.mjs`, `invalidate.mjs`, `signals.mjs`, `run.mjs`, `kill.mjs`, `watch.mjs`, `index.mjs`, `tests/state.test.mjs`.
- CLI routing: `npm/bin/n-cursor.js`, `case 'graph'` імпортує `../scripts/graph/index.mjs`.
- Доданий route: `case 'watch'` → `../scripts/graph/watch.mjs`.
- Стан вузла в новій системі деривується з файлів у `tasks/<node>/`: `task.md`, `run_NNN.md`, `outputs_NNN.md`, `invalidated` та активного worktree.
- Сигнальні команди агента згадані в transcript: `mt done <path>`, `mt audit <path>`, `mt failed <path>`, `mt spawn <path>`; вони пишуть `.signal` у директорію вузла, wrapper читає сигнал після завершення процесу.

## Update 2026-06-06

- Уточнено sentinel-based state machine для `tasks/<node>/`: `waiting` — лише `task.md`; `running` — активний worktree; `resolved` — існує `outputs_NNN.md`; `failed` — є `run_NNN.md` без відповідного `outputs_NNN.md` і без worktree; `invalidated` — існує sentinel `invalidated`.
- Реалізаційні функції, згадані в transcript: `deriveNodeState`, `latestNumbered`, `nextNumbered`, `sanitizePathToWorktreePrefix` у `npm/scripts/graph/state.mjs`.
- CLI routing уточнено як `case 'graph'` → `scripts/graph/index.mjs`, `case 'graph-dag'` → legacy `scripts/dispatcher/graph.mjs`, `case 'watch'` → `scripts/graph/watch.mjs`.
- Для комунікації агент→wrapper використовується sentinel `.ncursor-signal` з `type: done|audit|failed|spawn`; wrapper читає файл після завершення процесу, видаляє sentinel і виконує відповідну дію.
- Після повної реалізації transcript фіксує відкат: `git checkout -- npm/bin/n-cursor.js`, `rm -rf npm/scripts/graph/`, `rm -f .changes/260606-2107.md`, після чого команда переходить до ітеративного проектування.
