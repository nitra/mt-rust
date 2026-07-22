## ADR `decomposed` і `claim-lost` виключено з `failed_streak`; додано `plan_reject_max`

## Context and Problem Statement
Всі результати крім `success` рахувались у `failed_streak`, включаючи `decomposed` (штатна планова декомпозиція) і `claim-lost` (ownership event). Кілька rejected composite plans могли вичерпати retry budget без жодної execution failure. Також агент-ревʼюер і агент-виконавець могли зациклитись на відхиленнях планів без автоматичної ескалації.

## Considered Options
* Усі не-`success` результати в `failed_streak` (поточний підхід)
* Розділення на категорії: execution failures, lifecycle transitions, ownership events

## Decision Outcome
Chosen option: "Розділення на категорії", because `decomposed` є штатним lifecycle переходом, а `claim-lost` — ownership event; лише execution failures (`failed`, `progress-timeout`, `budget-exceeded`, `merge-conflict`) мають збільшувати `failed_streak`.

### Consequences
* Good, because агент може декілька разів пропонувати composite план без штучного вичерпання `agent_retry_max`.
* Good, because втрата claim (наприклад, lease expiry на повільній машині) не карає вузол ескалацією.
* Good, because план-відхилення між агентами отримують окремий `plan_reject_max` поріг з ескалацією до людини (не EngineerAgent).
* Bad, because формула `failed_streak = max(run NNN) - max(fact NNN)` стає складнішою: оркестратор тепер читає `result:` з frontmatter `run_*.md` при скані.

## More Information
Нова формула: `failed_streak = count(run_*.md де result ∈ {failed, progress-timeout, budget-exceeded, merge-conflict} і NNN > last_fact_NNN)`. Два окремих ескалаційних шляхи: `failed_streak ≥ agent_retry_max` → EngineerAgent; `count(plan-rejected_*.md) ≥ plan_reject_max` → `unresolvable` + алерт людині. Файл: `npm/docs/mt.md` рядки ~475, ~519, ~744.
