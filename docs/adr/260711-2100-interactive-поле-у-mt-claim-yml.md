## ADR Поле `interactive:` у `.mt-claim.yml`

## Context and Problem Statement

Архітектура 0.3.0 (git.md, «Claim») додає до `.mt-claim.yml` поле `interactive: false` і ототожнює `token = session_id`: інтерактивна сесія (attach) — це той самий run вузла, але з коротшим lease (`interactive_claim_lease_sec`, дефолт 900) і людиною за кермом. Реалізація claim-ів у `mt-core` (та graph-міст agent-server поверх неї) писала claim без цього поля — оркестратор і зовнішні спостерігачі не могли відрізнити інтерактивний claim від автономного, а отже не могли застосувати різні політики (watchdog/progress_timeout не діє на інтерактивні; бюджети — soft-alert замість kill; черга dispatch не чіпає вузол, який людина тримає в чаті).

## Considered Options

* Додати `interactive: bool` у `ClaimFields`/`claim_yaml` і читати його в `ClaimInfo` (schema_version лишається 1 — нове поле backward-сумісне: старі парсери читають лише відомі ключі)
* Розрізняти інтерактивність за `actor: human` (без зміни схеми)
* Підняти schema_version до 2

## Decision Outcome

Chosen option: "Додати `interactive: bool` зі schema_version 1", because канон 0.3.0 явно фіксує це поле у схемі claim-а; `actor` — семантика виконавця, не режиму (агент теж може бути під інтерактивним наглядом, а `actor: human` існує й в автономному h.md-потоці); бампити schema_version немає потреби — додавання optional-поля не ламає читачів 0.2.x (fail closed стосується лише невідомих МАЙБУТНІХ версій, не невідомих полів).

### Consequences

* Good, because оркестратор/дашборд бачать режим run-а безпосередньо з claim ref (без евристик за lease-довжиною).
* Good, because renewal/takeover успадковують поле природно — воно частина `ClaimFields`, які контролює тримач.
* Bad, because до перегенерації старих claim-ів поле відсутнє — читачі мусять трактувати відсутність як `false` (закладено в парсер).

## More Information

Реалізація: `mt_core::claims::{ClaimFields, claim_yaml, parse_claim, ClaimInfo}`; автономний runner пише `interactive: false`, `agent_server::graph::attach` — `true`. Канон: npm/docs/architecture/git.md («Claim»), runtime.md («Інтерактивна сесія = run вузла»).
