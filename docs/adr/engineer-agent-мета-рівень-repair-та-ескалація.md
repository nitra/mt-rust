# Engineer Agent — мета-рівень, time budget та ієрархічна ескалація

**Status:** Accepted
**Date:** 2026-06-06

## Context and Problem Statement
При помилці вузла у Recursive Compound DAG потрібен механізм самовідновлення: хто виконує відновлення і з яким рівнем доступу до структури графа, як зберігати пам'ять між спробами, як запобігти нескінченним циклам виправлень і яка структура ескалації до людини.

## Considered Options
* EngineerAgent як вузол у графі (рекурсивна self-repair)
* EngineerAgent як мета-рівень поза графом з необмеженим доступом
* `max_attempts: N` як convergence guard
* Time budget (фіксований час, необмежена кількість спроб у межах бюджету)
* Пам'ять у стані агента (stateful engineer)
* Пам'ять на вузлі у файлі (`repair_history.json`, stateless engineer)

## Decision Outcome
Chosen option: "EngineerAgent як мета-рівень + time budget + repair_history на вузлі", because інженер не є вузлом графа і може підніматись вгору, змінюючи будь-який рівень ієрархії; time budget (замість ліміту спроб) дозволяє адаптувати стратегію залежно від залишку часу; знання про спроби зберігаються разом із вузлом і доступні будь-якому наступному виклику агента.

### Consequences
* Good, because інженер може виконати `replace node`, `insert nodes`, `rewire edges`, `modify inputs` на будь-якому рівні ієрархії.
* Good, because time budget дає передбачуваний максимальний час до ескалації: `depth × budget`; стратегія адаптується до `deadline - now()`.
* Good, because `repair_history.json` спільний для послідовних викликів на один вузол — нові виклики не повторюють невдалих стратегій.
* Bad, because необмежений доступ означає ризик cascade invalidation вниз по successors при патчі батьківського вузла.
* Bad, because `depth × budget` зростає лінійно з глибиною ієрархії — при глибокій вкладеності час до ескалації може бути тривалим.

## More Information
Алгоритм відновлення:
```
on node.state = failed:
  EngineerAgent(error.json, path_from_root, repair_history.json):
    analyze → GraphPatch на будь-якому рівні → retry
    if deadline reached → node.state = "unresolvable" → escalate вгору
```

`repair_history.json` пишеться на вузлі, де внесено зміну; дочірній логує `{"triggered_parent_patch": "<patch_id>"}`:
```json
[{"attempt": 1, "engineer_reasoning": "...", "patch_applied": {}, "result": "failed", "failure_reason": "..."}]
```

`repair_context.json` (встановлюється при першому виклику):
```json
{"deadline": "<ISO>", "started_at": "<ISO>", "time_budget_sec": 600, "attempts": []}
```

Ієрархічна ескалація: кожен батьківський рівень отримує СВІЖИЙ `time_budget_sec`; root timeout → `senior_report.json`:
```json
{"failed_node": "<path від root>", "escalation_chain": [{"level": "node_7", "time_spent": "10хв", "attempts": []}], "current_graph_snapshot": "...", "suggested_next_steps": []}
```

Дизайн зафіксовано у `npm/docs/mt.md` (`/Users/vitaliytv/www/nitra/cursor/`). Аналоги: Dask, Prefect dynamic tasks, LangGraph.

## Update 2026-06-06

- Інженер працює як мета-рівень поза графом, а не як звичайний вузол графу.
- Для аналізу збою інженеру потрібен повний path від кореня до вузла, що впав, щоб обрати рівень втручання: сам вузол, батько або root.
- Памʼять repair-процесу зберігається біля вузла, щоб майбутні виклики інженера не повторювали невдалі підходи.
- Convergence guard для інженера — часовий бюджет, а не лічильник спроб; кожен рівень ескалації отримує свіжий budget.
- При патчі вузла залежні worktree мають бути зупинені перед зміною цілі, після чого залежний каскад перезапускається.

## Update 2026-06-06

- Інженерський repair-flow тригериться лише після `actor: agent` з `result: failed`, якщо `auto_engineer: true` у `.n-cursor.json`.
- Для запобігання нескінченним петлям transcript фіксує одну інженерську спробу перед ескалацією до людини.
- `budget_sec` у `task.md` є спільним бюджетом вузла для всіх акторів; при вичерпанні budget wrapper зупиняє виконання і викликає notify-flow.
- Після `actor: engineer result: failed` система має виконати `graph notify <path>`; transcript не містить підтвердження додаткових retry-циклів.
