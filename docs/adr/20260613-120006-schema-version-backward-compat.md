## ADR schema_version: backward compatibility для всіх відомих версій

**Status:** Accepted
**Date:** 2026-06-13

## Context and Problem Statement

Оркестратор відмовляв читати файли з "невідомою" `schema_version`. При частковій міграції `v1`→`v2` repository з сумішшю файлів обробка зупинялась би повністю. Reviewer запропонував `migration_state: migrating | migrated` у `.mt.json` для graceful degradation під час міграції.

## Considered Options

* `migration_state` у `.mt.json`: читати обидві версії за `migrating`, логувати конфлікти, не зупинятись
* Backward compatibility: orchestrator читає всі версії які він знає (включаючи попередні); відмовляє лише версії вищі за власну максимальну

## Decision Outcome

Chosen option: "backward compatibility", because orchestrator завжди знає всі попередні версії схеми, які він обробляв. Відмова потрібна тільки для майбутніх версій (невідомих поточному бінарнику). `migration_state` додає зайву складність і ризик застрягти у стані `migrating` невизначено довго.

### Consequences

* Good, because часткова міграція не блокує обробку; не потрібен `migration_state`; major release MT постачає migration script і новий orchestrator читає і стару, і нову версію без додаткової конфігурації.
* Bad, because transcript не містить підтверджених негативних наслідків.

## More Information

Змінено: `npm/docs/mt.md` рядок ~144: "Оркестратор підтримує backward compatibility: читає всі версії, які він знає (включаючи попередні). Відмовляє лише файли з версією вищою за власну максимальну (майбутні релізи). Breaking schema changes постачаються як окремий major release MT з явним описом переходу і migration script."
