# Відповідність специфікації: карта реалізації

**Дата зрізу:** 2026-08-09
**Специфікація:** `@7n/mt` — [цільова архітектура 0.3.0-draft](https://github.com/nitra/mt/blob/main/docs/architecture/index.md)
**Метод:** прочитання глав архітектури проти коду `crates/`, `relay/`, `layers/`; вердикт ставиться за наявністю реальної логіки, не за назвою файлу чи згадкою в коментарі.

> Живий документ. Правило: зміна вердикту йде тим самим PR, що й код, який його змінює. Зріз без дати й методу — не зріз.

## Навіщо цей документ

Архітектура позначена `0.3.0-draft`. «Draft» знімається не декларацією, а доказом: кожна нормативна вимога має реалізацію і тест, який її стереже. Цей файл — burn-down між специфікацією і кодом: він робить «перейти від draft до повної реалізації» вимірюваним, а не оцінковим.

**Критерій зняття `-draft` з 0.3.0:** усі рядки таблиць нижче в стані РЕАЛІЗОВАНО, кожна вимога має тест у `cargo test --workspace` або `bun test`, і demo-критерії M0–M6 із [roadmap](https://github.com/nitra/mt/blob/main/docs/roadmap.md) відтворюються.

## Зведення за мілстоунами

| Мілстоун | Стан | Головне, чого бракує |
| --- | --- | --- |
| M0 — dogfood ядра | значною мірою | Stage 1 (inline-планування), контекст агента, аудит-цикл, EngineerAgent, `unresolvable`, recurrence, git-протокол lifecycle-операцій |
| M1 — agent-server | значною мірою | orchestrator-роль і wake, backpressure за спекою, глибокий реплей, злиття `agent-cli` у `mt serve\|attach` |
| M2 — mission control | частково | матеріалізація підпису в `## Approvals`, персистентний store і auth, реальний push-транспорт, handoff між машинами через relay, presence |
| M3 — dashboard і поверхні | не починався | surface-профілі, MCP-сервери, preview/`ContextSelected`, `client_kind: mt-dashboard` |
| M4 — файловий шар i18n | не починався | `refs/mt/i18n`, worktree-матеріалізація, write path у base, lazy-мови (`layers/` — суміжна задача, інший контейнер і конфіг) |
| M5 — мета-цикл retro | не починався | увесь рушій; дані для нього вже накопичуються |
| M6 — мандати й Дельта | не починався | уся глава; під нею вже лежить retry ladder |

## Ядро графа (`mt-core`)

| Підсистема | Вердикт | Де в коді | Чого бракує |
| --- | --- | --- | --- |
| Сканування графа, `deps/`, denylist | РЕАЛІЗОВАНО | `lib.rs` `scan_tasks`/`scan_dir` | — |
| Derived-стани вузла | ЧАСТКОВО | `lib.rs` `detect_state` | `stalled` не виводиться (немає інтеграції з remote claim refs); `blocked-invalid-dep` як warning |
| `failed_streak` за категорією `result` | РЕАЛІЗОВАНО | `lib.rs` `failed_streak`/`is_execution_failure` | — (межа рахунку — останній `fact_NNN`; спека каже «останній *прийнятий* fact» — відкрите питання нижче) |
| Файловий контракт `a.md`/`h.md` | ЧАСТКОВО | `lib.rs` `write_executor_flag` | Пишуться markdown-секціями, спека вимагає YAML-фронтматер; немає `secrets`, `interactive`, `assignee`, `notify`, `parent` — **конфлікт спека↔код, потребує рішення** |
| Артефакти version chain (читання) | РЕАЛІЗОВАНО | `artifacts.rs` | — |
| `schema_version` fail-closed | ВІДСУТНЄ | — | Поле пишеться, але жоден читач не валідує версію |
| Гейт immutability (`task.md`/`a.md`/`h.md` проти `origin/main`) | ВІДСУТНЄ | — | Немає ні в `signal.rs`, ні в `runner.rs` |
| Claims: CAS, lease, grace, takeover | РЕАЛІЗОВАНО | `claims.rs` | `fetch_remote_claims` існує, але не викликається з продакшн-коду |
| Fenced publish | РЕАЛІЗОВАНО | `publish.rs` | Батчинг кількох результатів; `result: merge-conflict` не матеріалізується у `run_NNN.md` |
| Run-wrapper, watchdog | РЕАЛІЗОВАНО | `runner.rs` | — |
| Retry ladder + каскад CLI | РЕАЛІЗОВАНО | `runner.rs` | Телеметрія `tokens_in/out`/`cost_usd` не збирається |
| Stage 1 — inline-планування | ВІДСУТНЄ | `runner.rs` `build_agent_prompt` | Немає фази «спершу `plan_NNN.md`», немає `result: decomposed`, немає динамічної декомпозиції |
| Контекст агента (system-prompt, deps, plan, prior attempts) | ВІДСУТНЄ | `runner.rs` | Інлайниться лише тіло `task.md` |
| `run-summary.md` | ВІДСУТНЄ | — | Генератора немає |
| Сигнали `done`/`audit`/`failed`, `## Check` | РЕАЛІЗОВАНО | `signal.rs` | — |
| Composite-агрегація вгору | РЕАЛІЗОВАНО | `signal.rs` `propagate_composite` | — |
| Протокол spawn | ЧАСТКОВО | `spawn.rs` | Немає fenced atomic commit — файли лише на диску; немає `plan_reject_max` і ескалації |
| `invalidate`/`kill` | ЧАСТКОВО | `lifecycle.rs` | Без git-протоколу: немає fenced publish, порівняння hash нового fact, поглинання running-вузлів |
| Оркестрація `run --auto` | ЧАСТКОВО | `orchestrate.rs` | Батчинг замість continuous backfill; немає periodic rescan, remote claims, wake |
| Worktree lifecycle | РЕАЛІЗОВАНО | `worktree.rs` | — |
| Git-межа (`gix` + вузький shim) | РЕАЛІЗОВАНО | `git/` | — |
| Аудит-цикл (auditor, clarification, amend) | ВІДСУТНЄ | — | Пишеться лише `pending-audit_NNN.md`; актора, `audit-result_NNN.md`, `clarification`/`amended` немає |
| EngineerAgent | ВІДСУТНЄ | — | Немає `--actor engineer`, GraphPatch |
| `unresolvable` (3 тригери + алерт) | ВІДСУТНЄ | — | Маркер лише читається, ніхто не пише |
| Recurrence | ВІДСУТНЄ | — | Уся глава `recurrence.md` |
| Secrets broker / sandbox `skill_profiles` | ВІДСУТНЄ | — | `a.md.secrets` не інжектиться, allowlist немає |
| `.mt.json` — повний набір ключів | ЧАСТКОВО | `config.rs` | Бракує `engineer_retry_max`, `plan_reject_max`, `audit_*`, `budget_total_sec`, `run_ref_ttl_days`, `surface_profiles`, `mcp_servers`, `relay_url`, `i18n.*` та ін. |

## Протокол, сесії, поверхні

| Підсистема | Вердикт | Де в коді | Чого бракує |
| --- | --- | --- | --- |
| Envelope + Event v4 (усі варіанти, forward-compat) | РЕАЛІЗОВАНО | `agent-protocol/envelope.rs` | — |
| Хендшейк, `lang`, exact version check | РЕАЛІЗОВАНО | `agent-protocol/handshake.rs` | — |
| Ed25519: approvals + transfers | РЕАЛІЗОВАНО | `agent-protocol/approvals.rs`, `transfers.rs` | — |
| Сесії: журнал, seq, відновлення | РЕАЛІЗОВАНО | `agent-server/session.rs` | Глибокий реплей із run ref (поза буфером) |
| WS-транспорт, capability-фільтр | ЧАСТКОВО | `agent-server/ws.rs` | Backpressure за спекою (скидання лише ефемерних + disconnect), ліміт кадру 2 MB |
| Discovery / single-instance | ЧАСТКОВО | `agent-server/discovery.rs` | Живої перевірки stale-lock немає |
| Інтерактивний run = run вузла | РЕАЛІЗОВАНО | `agent-server/graph.rs` | Інтерактивні політики (`progress_timeout_sec`), телеметрія ходів |
| Handoff між хостами | ЧАСТКОВО | `agent-server/graph.rs`, `ws.rs` | Немає `HandoffRequest` як події й доставки через relay; немає checkpoint-режиму |
| Approvals-гейт mid-run | ЧАСТКОВО | `agent-server/approvals_gate.rs` | **Немає матеріалізації підпису в `## Approvals`** — це блокує demo-критерій M2 |
| ACP-транспорт | РЕАЛІЗОВАНО | `agent-core/acp.rs` | `mcpServers` жорстко порожній |
| Orchestrator-роль у agent-server + wake | ВІДСУТНЄ | — | Скан/dispatch/GC/алерти при wake |
| `client_kind: mt-dashboard` | ВІДСУТНЄ | — | Типи подій є, ніхто не емітить |
| Surface-профілі, MCP, preview, `ContextSelected` | ВІДСУТНЄ | — | Уся глава `surfaces.md` |

## Люди, доступ, relay

| Підсистема | Вердикт | Де в коді | Чого бракує |
| --- | --- | --- | --- |
| Ролі owner⊃host⊃approver⊃viewer | РЕАЛІЗОВАНО | `relay/lib/store.mjs`, `relay.mjs` | — |
| Схема даних relay | ЧАСТКОВО | `relay/lib/store.mjs` | Лише in-memory; персистентного сховища немає — рестарт зносить membership |
| Auth акаунтів (email + passkey) | ВІДСУТНЄ | — | Немає HTTP-поверхні, логіну, recovery |
| Реєстрація пристрою | РЕАЛІЗОВАНО | `relay/lib/store.mjs` | — |
| Ротація/revocation pubkey | ВІДСУТНЄ | — | Немає історії ключів і видалення пристрою |
| Membership: invite/accept/decline | РЕАЛІЗОВАНО | `relay/lib/relay.mjs` | — |
| Membership: зміна ролі / видалення | ВІДСУТНЄ | store-методи є, API немає | `MemberChanged {role: null}` ніколи не емітується |
| Transfer ownership із підписом | РЕАЛІЗОВАНО | `relay/lib/relay.mjs`, `agent-protocol/transfers.rs` | — |
| Кімнати, буфер, viewer read-only | РЕАЛІЗОВАНО | `relay/lib/rooms.mjs` | — |
| Presence | ВІДСУТНЄ | — | Лише `last_seen` |
| Гейт 2 — plan-review з підписом | ЧАСТКОВО | `spawn.rs` | У фронтматері рішення немає блоку `approved_by` з підписом |
| Гейт 3 — аудит-вердикт людини | ЧАСТКОВО | `signal.rs` | Немає блоку підпису в `audit-result_NNN.md` |
| Push тип 2 / тип 3 | ЧАСТКОВО | `relay/lib/push.mjs` | Тип 1 відсутній; тип 3 не покриває `notify` для `h.md`-assignee; транспорт FCM/APNs — in-memory sink |
| PII-directory | ЧАСТКОВО | `mt-core/directory.rs` | Мертвий код: парсер є, викликів немає → `Escalation.to_account_id` ніхто не заповнює |

## i18n, retro, мандати

| Підсистема | Вердикт | Коментар |
| --- | --- | --- |
| i18n: конфіг, `refs/mt/i18n`, worktree-матеріалізація, write path у base, lazy-мови | ВІДСУТНЄ | У `crates/` i18n немає. `layers/` покриває суміжну задачу (derived-переклади доків) іншим контейнером (`x.<lang>.md` у робочому дереві), іншим конфігом (`layers.json`) і однонапрямним потоком; contract-awareness там — інструкція моделі, не парсер із fail-closed |
| i18n: `lang` у ClientHello | РЕАЛІЗОВАНО | `agent-protocol/handshake.rs`; далі хендшейку `lang` не використовується |
| retro: рушій, пропозиції, innovation, impact | ВІДСУТНЄ | Дані накопичуються (`ledger.rs`, run-історія dogfood-графа), читача немає |
| Мандати: карта, `decision-request`, `leverage_facets`, `chosen_option`, `awaiting-decision`, маршрутизатор, квіз-гейт, прецеденти, селектор, профілі, watcher, ШІ-мандати | ВІДСУТНЄ | Уся глава `mandates.md`. Половина інваріанта retry-before-escalate уже є: драбина реалізована (`runner.rs`), бракує виходу «розвилка → `decision-request`» |
| Подія `Escalation` | РЕАЛІЗОВАНО (інша річ) | Записка «вгору» за handle з owner-app spec, не mandates-розвилка: без карти, фасетів, варіантів і підписаного рішення |

## Хвилі робіт

Порядок обраний так, щоб кожна наступна хвиля спиралась на замкнений інваріант попередньої, а не на обіцянку.

1. **Контрактний борг.** `schema_version` fail-closed, гейт immutability, формат `a.md`/`h.md`, повний набір ключів `.mt.json`, матеріалізація `merge-conflict`, `orphan-node` warning. Дешево, і без цього кожна наступна хвиля успадковує розходження зі спекою. `failed_streak` за категорією — **закрито**.
2. **Замкнути M0 як автономний цикл.** Stage 1 + контекст агента, аудит-цикл, `unresolvable` з трьома тригерами, EngineerAgent, git-протокол для spawn/invalidate/kill. Це і є «перший продукт» зі стратегії: автономне досягнення мети з людиною на гейтах.
3. **M1 доведення + wake.** Orchestrator-роль, continuous backfill, remote claims у скані, `stalled`, злиття `agent-cli` у `mt serve|attach`, backpressure, глибокий реплей.
4. **M2 mission control.** Першим — матеріалізація підпису в `## Approvals` (це буквально demo-критерій), далі персистентний store, auth, push-транспорт, `HandoffRequest` через relay, presence.
5. **M6 фаза 0 — модельний трек Дельти.** Паралельно від хвилі 2, як велить roadmap: `mandates.yaml` (включно з `kind: model` і `audacity`), `decision-request` із `leverage_facets`, `chosen_option`, стан `awaiting-decision`, квіз-гейт, конверсія вичерпаної драбини в розвилку. Соціальних ризиків нема — механіка обкатується на моделях.
6. **M3 / M5 / M4.** Dashboard і поверхні; retro (MVP не чекає M1–M4 — дані вже є); файловий шар i18n.

## Відкриті питання

- **Межа `failed_streak`.** Спека каже «NNN > останнього *прийнятого* fact»; реалізація рахує від останнього `fact_NNN` незалежно від вердикту аудиту. Розходження проявляється лише в rework-циклі після провального аудиту. Потрібне рішення: уточнити спеку чи змінити код.
- **Формат `a.md`/`h.md`.** Спека вимагає YAML-фронтматер, код пише markdown-секції. Один із двох має поступитись — це контракт, який читають і люди, і агенти.
- **`mt-napi`.** За рішенням Г специфікації `2026-07-23-mt-cli-rust.md` крейт мав бути видалений, але лишається у workspace members.
