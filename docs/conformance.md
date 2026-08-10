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
| Derived-стани вузла | ЧАСТКОВО | `lib.rs` `detect_state` | `stalled` не виводиться (немає інтеграції з remote claim refs); `blocked-invalid-dep` як warning (поверхня `TaskNode.warnings` уже є) |
| `failed_streak`: категорія `result` + межа «прийнятий fact» | РЕАЛІЗОВАНО | `lib.rs` `failed_streak`/`is_execution_failure`/`accepted_fact_nnn` | — |
| Файловий контракт `a.md`/`h.md` | ЧАСТКОВО | `lib.rs` `write_executor_flag`, `runner.rs` `read_executor_flag` | Формат — YAML-фронтматер (закрито); читаються `model_tier`, `agent_cli`, `retry_ladder`. Ще не спожиті: `secrets` (брокер), `interactive`, `assignee`, `parent` |
| Артефакти version chain (читання) | РЕАЛІЗОВАНО | `artifacts.rs` | — |
| `schema_version` fail-closed | РЕАЛІЗОВАНО | `frontmatter.rs` `schema_version_of`/`check_schema_version`; гейти — `runner.rs` preflight, `signal.rs` `node_dir`, `spawn.rs` `plan_review` | — (невідома версія — жорстка відмова; відсутнє поле — попередження скану, див. «Закриті питання») |
| Гейт immutability (`task.md`/`a.md`/`h.md` проти `origin/main`) | РЕАЛІЗОВАНО | `signal.rs` `check_contract_unchanged` (у `signal_success`) | — (стоїть на `done`/`audit`; `failed` свідомо не гейтиться) |
| Claims: CAS, lease, grace, takeover | РЕАЛІЗОВАНО | `claims.rs` | `fetch_remote_claims` існує, але не викликається з продакшн-коду |
| Fenced publish + failure-сімейство | РЕАЛІЗОВАНО | `publish.rs` `fenced_publish`/`publish_failure_run`, `runner.rs` `terminal_conflict_reason` | Батчинг кількох результатів одним push |
| Run-wrapper, watchdog | РЕАЛІЗОВАНО | `runner.rs` | — |
| Retry ladder + каскад CLI | РЕАЛІЗОВАНО | `runner.rs` | Телеметрія `tokens_in/out`/`cost_usd` не збирається |
| Stage 1 — inline-планування | РЕАЛІЗОВАНО | `runner.rs` `needs_planning`/`build_plan_prompt`/`latest_plan_decision` | — (Етап 1 пропускається за `hint: atomic` або наявного плану; порожній результат читається як неявний atomic) |
| Контекст агента (system-prompt, deps, plan, prior attempts) | РЕАЛІЗОВАНО | `runner.rs` `build_agent_prompt`/`dep_facts`/`prior_attempts` | — (другий шар стискання читає `run-summary.md`, якщо він є; генератора самого файлу ще немає — окремий рядок) |
| `run-summary.md` | ВІДСУТНЄ | — | Генератора немає |
| Сигнали `done`/`audit`/`failed`, `## Check` | РЕАЛІЗОВАНО | `signal.rs` | — |
| Composite-агрегація вгору | РЕАЛІЗОВАНО | `signal.rs` `propagate_composite` | — |
| Протокол spawn | РЕАЛІЗОВАНО | `spawn.rs` `spawn_approve`/`publish_spawn`, `publish.rs` `publish_lifecycle` | — (`plan_reject_max` закрито через `unresolvable`) |
| Git-протокол `invalidate`/`kill` | ЧАСТКОВО | `lifecycle.rs` `publish_mutation` | Транспорт закрито (atomic commit різниці станів). Лишається семантика re-run: порівняння hash нового fact (однаковий → нащадки розблоковуються, різний → cascade) і поглинання running-вузлів (`mt stop`) |
| Оркестрація `run --auto` | ЧАСТКОВО | `orchestrate.rs` | Батчинг замість continuous backfill; немає periodic rescan, remote claims, wake |
| Worktree lifecycle | РЕАЛІЗОВАНО | `worktree.rs` | — |
| Git-межа (`gix` + вузький shim) | РЕАЛІЗОВАНО | `git/` | — |
| Аудит-цикл: вердикт, clarification, amend, `audit_failed_streak` | РЕАЛІЗОВАНО | `audit.rs`; CLI — `mt verdict`/`mt clarify`/`mt amend` | — |
| Аудитор як актор (`mt run --actor auditor`, `audit_model`) | ВІДСУТНЄ | — | Вердикт наразі виносить людина або зовнішній агент через CLI; автоматичного аудитора і тригерів `audit_schedule_days`/`audit_on_patch` немає |
| EngineerAgent | ВІДСУТНЄ | — | Немає `--actor engineer`, GraphPatch |
| `unresolvable` (3 тригери + алерт) | ЧАСТКОВО | `lib.rs` `unresolvable_reason`/`write_unresolvable`; тригери — `runner.rs` (перед комітом), `spawn.rs` `spawn_reject` | Алерт власнику (relay push) — потребує orchestrator-ролі й relay-шляху (хвиля 3) |
| Recurrence | ВІДСУТНЄ | — | Уся глава `recurrence.md` |
| Secrets broker / sandbox `skill_profiles` | ВІДСУТНЄ | — | `a.md.secrets` не інжектиться, allowlist немає |
| `.mt.json` — дефолти для реалізованого | РЕАЛІЗОВАНО | `config.rs` `config_defaults` | — (ключі нереалізованих фіч свідомо без дефолтів, див. «Закриті питання») |

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

1. **Контрактний борг — ✅ закрито.** `failed_streak` (категорія + межа), формат `a.md`/`h.md`, видалення `mt-napi`, `schema_version` fail-closed, гейт immutability, дефолти `.mt.json`, `orphan-node`, матеріалізація `result: merge-conflict`.
2. **Замкнути M0 як автономний цикл.** Лишилось: генератор `run-summary.md`, аудитор як актор, EngineerAgent, git-протокол `invalidate`/`kill` — транспорт (лишається hash-порівняння після re-run і `mt stop`). **Закрито:** `unresolvable` з трьома тригерами (крім алерту — потребує relay-шляху хвилі 3), контекст агента, git-протокол spawn і invalidate/kill, аудит-цикл, Stage 1. Це і є «перший продукт» зі стратегії: автономне досягнення мети з людиною на гейтах.
3. **M1 доведення + wake.** Orchestrator-роль, continuous backfill, remote claims у скані, `stalled`, злиття `agent-cli` у `mt serve|attach`, backpressure, глибокий реплей.
4. **M2 mission control.** Першим — матеріалізація підпису в `## Approvals` (це буквально demo-критерій), далі персистентний store, auth, push-транспорт, `HandoffRequest` через relay, presence.
5. **M6 фаза 0 — модельний трек Дельти.** Паралельно від хвилі 2, як велить roadmap: `mandates.yaml` (включно з `kind: model` і `audacity`), `decision-request` із `leverage_facets`, `chosen_option`, стан `awaiting-decision`, квіз-гейт, конверсія вичерпаної драбини в розвилку. Соціальних ризиків нема — механіка обкатується на моделях.
6. **M3 / M5 / M4.** Dashboard і поверхні; retro (MVP не чекає M1–M4 — дані вже є); файловий шар i18n.

## Закриті питання

- **Обсяг дефолтів `.mt.json`** (2026-08-09): рішення — дефолт додається разом із кодом, що його читає, а не наперед за списком зі спеки. Причина: `merge_config` пропускає **будь-який** ключ користувача наскрізь, тож відсутній дефолт нічого не ламає — а два десятки констант для нереалізованих фіч (аудит-цикл, recurrence, i18n, relay, surfaces) були б мертвим конфігом, який виглядає як реалізована поверхня. Закрито реальне розходження: `agent_retry_max` читався в обхід `config_defaults` власним хардкодом `3`.

- **Обсяг гейта immutability** (2026-08-09): спека називає `mt done`/`mt audit` — `failed` свідомо **не** гейтиться. Причина: сенс гейта в тому, що виконавець не може переписати власний контракт (послабити `## Done when`, викинути рядок `## Check`, підняти собі `model_tier`) і на цьому **оголосити успіх**; провал контракту не привласнює, а блокування `failed` лише сховало б діагностику ретраю. Гейт fail-open там, де порівнювати нема з чим (немає репо, немає `origin/main`, файлу ще немає в базі) — це стан «до worktree», у якому спека дозволяє вільні правки.

- **Строгість `schema_version`** (2026-08-09): спека каже і «перше поле всіх файлів із фронтматером», і «невідома версія → fail closed». Реалізовано двома рівнями, бо це дві різні вимоги: **невідома версія** (число не наше або взагалі не число) — жорстка відмова на гейтах `preflight`/`signal`/`spawn`, тобто вузол не виконується й результат не публікується; **відсутнє поле** — попередження скану, не відмова. Причина: файл без поля не є файлом з майбутньої схеми, тож читати його безпечно, а вимога «перше поле» адресована запису. Усі 47 фронтматер-файлів dogfood-графа поле мають, тож попередження не шумить.

- **`mt-napi`** (2026-08-09): рішення — видалити з репо. Питання було поставлене неточно («лишився всупереч рішенню Г»): рішення Г від 2026-07-23 скасувала специфікація від 2026-07-27, яка свідомо повернула крейт заради `@7n/rules`. Але 2026-07-30 `@7n/rules` зробив власний `rules-core`/`rules-napi` із власним `worktree.rs` — і крейт лишився **без жодного споживача**, тягнучи CI-матрицю zigbuild і два платформні npm-підпакети. Видалено `crates/mt-napi`, `npm/mt-napi`, `packages/`, `npm-publish.yml`; опубліковані версії в npm не знімались (unpublish зламав би невідомих зовнішніх споживачів).

- **Формат `a.md`/`h.md`** (2026-08-09): рішення — **YAML-фронтматер у markdown**, жорсткий перехід. Ні голий YAML (як пропонувала спека), ні markdown-секції (як робив код): файл має розширення `.md`, тож фронтматер узгоджений з рештою `.md`-артефактів, перевикористовує один код-шлях `parse_front_matter`, рендериться при рев'ю і лишає місце для прози в тілі. Голий YAML лишається контрактом `.yml`-файлів (`.mt-claim.yml`). Спеку уточнено (nitra/mt), 19 файлів dogfood-графа мігровано, старий формат відхиляється явною помилкою в preflight — тести `preflight_rejects_legacy_section_flag`, `preflight_rejects_flag_without_frontmatter`.
- **Межа `failed_streak`** (2026-08-09): рішення — спека виграє, код приведено до неї. Межа — останній *прийнятий* fact (`accepted_fact_nnn`); відхилений аудитом fact межу не рухає. Причина: інакше цикл «провал → провал → сирий fact → аудит відхилив» обнуляє лічильник вічно, і драбина ретраїв ніколи не доходить до EngineerAgent чи `unresolvable` — livelock. Тест-сторож: `rejected_fact_livelock_terminates`.

## Відкриті питання

Немає — усі три конфлікти зрізу 2026-08-09 закриті.
