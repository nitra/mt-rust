# Відповідність специфікації: карта реалізації

**Дата зрізу:** 2026-08-09
**Специфікація:** `@7n/mt` — [цільова архітектура 0.3.0-draft](https://github.com/nitra/mt/blob/main/docs/architecture/index.md)
**Метод:** прочитання глав архітектури проти коду `crates/` і `relay/`; вердикт ставиться за наявністю реальної логіки, не за назвою файлу чи згадкою в коментарі.

> Живий документ. Правило: зміна вердикту йде тим самим PR, що й код, який його змінює. Зріз без дати й методу — не зріз.

## Навіщо цей документ

Архітектура позначена `0.3.0-draft`. «Draft» знімається не декларацією, а доказом: кожна нормативна вимога має реалізацію і тест, який її стереже. Цей файл — burn-down між специфікацією і кодом: він робить «перейти від draft до повної реалізації» вимірюваним, а не оцінковим.

**Критерій зняття `-draft` з 0.3.0:** усі рядки таблиць нижче в стані РЕАЛІЗОВАНО, кожна вимога має тест у `cargo test --workspace` або `bun test`, і demo-критерії M0–M6 із [roadmap](https://github.com/nitra/mt/blob/main/docs/roadmap.md) відтворюються.

## Зведення за мілстоунами

| Мілстоун | Стан | Головне, чого бракує |
| --- | --- | --- |
| M0 — dogfood ядра | цикл замкнено | recurrence; телеметрія вартості |
| M1 — agent-server | ✅ закрито | — |
| M2 — mission control | частково | checkpoint-handoff, CLI `mt sessions` |
| M3 — dashboard і поверхні | почався | preview-модуль (MCP — розвиток спинено, див. «Закриті питання») |
| M4 — файловий шар i18n | ядро | сховище `refs/mt/i18n`, write path, черга регенерації, live-шар |
| M5 — мета-цикл retro | MVP | LLM-крок, innovation/baseline, impact-зрізи, фоновий прогін |
| M6 — мандати й Дельта | фаза 0 закрита | escalation-intake, маршрутизація за важелем, прецеденти, селектор, профілі, watcher |

## Ядро графа (`mt-core`)

| Підсистема | Вердикт | Де в коді | Чого бракує |
| --- | --- | --- | --- |
| Сканування графа, `deps/`, denylist | РЕАЛІЗОВАНО | `lib.rs` `scan_tasks`/`scan_dir` | — |
| Derived-стани вузла | РЕАЛІЗОВАНО | `lib.rs` `detect_state`/`apply_remote_claims`/`scan_tasks_with_claims` | — (`blocked-invalid-dep` як warning — поверхня `TaskNode.warnings` є, окремий рядок беклогу) |
| `failed_streak`: категорія `result` + межа «прийнятий fact» | РЕАЛІЗОВАНО | `lib.rs` `failed_streak`/`is_execution_failure`/`accepted_fact_nnn` | — |
| Файловий контракт `a.md`/`h.md` | ЧАСТКОВО | `lib.rs` `write_executor_flag`, `runner.rs` `read_executor_flag` | Формат — YAML-фронтматер (закрито); читаються `model_tier`, `agent_cli`, `retry_ladder`, `secrets`. Ще не спожиті: `interactive`, `assignee`, `parent` |
| Артефакти version chain (читання) | РЕАЛІЗОВАНО | `artifacts.rs` | — |
| `schema_version` fail-closed | РЕАЛІЗОВАНО | `frontmatter.rs` `schema_version_of`/`check_schema_version`; гейти — `runner.rs` preflight, `signal.rs` `node_dir`, `spawn.rs` `plan_review` | — (невідома версія — жорстка відмова; відсутнє поле — попередження скану, див. «Закриті питання») |
| Гейт immutability (`task.md`/`a.md`/`h.md` проти `origin/main`) | РЕАЛІЗОВАНО | `signal.rs` `check_contract_unchanged` (у `signal_success`) | — (стоїть на `done`/`audit`; `failed` свідомо не гейтиться) |
| Claims: CAS, lease, grace, takeover | РЕАЛІЗОВАНО | `claims.rs`; читач — `lib.rs` `scan_tasks_with_claims`, CLI `mt status` | — |
| Fenced publish + failure-сімейство | РЕАЛІЗОВАНО | `publish.rs` `fenced_publish`/`publish_failure_run`, `runner.rs` `terminal_conflict_reason` | Батчинг кількох результатів одним push |
| Run-wrapper, watchdog | РЕАЛІЗОВАНО | `runner.rs` | — |
| Retry ladder + каскад CLI | РЕАЛІЗОВАНО | `runner.rs` | Телеметрія `tokens_in/out`/`cost_usd` не збирається |
| Stage 1 — inline-планування | РЕАЛІЗОВАНО | `runner.rs` `needs_planning`/`build_plan_prompt`/`latest_plan_decision` | — (Етап 1 пропускається за `hint: atomic` або наявного плану; порожній результат читається як неявний atomic) |
| Контекст агента (system-prompt, deps, plan, prior attempts) | РЕАЛІЗОВАНО | `runner.rs` `build_agent_prompt`/`dep_facts`/`prior_attempts` | — (другий шар стискання читає `run-summary.md`, якщо він є; генератора самого файлу ще немає — окремий рядок) |
| `run-summary.md` (другий шар стискання) | РЕАЛІЗОВАНО | `runner.rs` (генерація за `run_summary_threshold`), читач — `build_agent_prompt` | — |
| Сигнали `done`/`audit`/`failed`, `## Check` | РЕАЛІЗОВАНО | `signal.rs` | — |
| Composite-агрегація вгору | РЕАЛІЗОВАНО | `signal.rs` `propagate_composite` | — |
| Протокол spawn | РЕАЛІЗОВАНО | `spawn.rs` `spawn_approve`/`publish_spawn`, `publish.rs` `publish_lifecycle` | — (`plan_reject_max` закрито через `unresolvable`) |
| Git-протокол `invalidate`/`kill` + re-run семантика | РЕАЛІЗОВАНО | `lifecycle.rs` `publish_mutation`/`stop`/`reconcile_after_rerun`; CLI — `mt stop` | — |
| Оркестрація `run --auto` | РЕАЛІЗОВАНО | `orchestrate.rs` `run_auto`; подієвий запуск — `agent-server/orchestrator.rs` | — |
| Worktree lifecycle | РЕАЛІЗОВАНО | `worktree.rs` | — |
| Git-межа (`gix` + вузький shim) | РЕАЛІЗОВАНО | `git/` | — |
| Аудит-цикл: вердикт, clarification, amend, `audit_failed_streak` | РЕАЛІЗОВАНО | `audit.rs`; CLI — `mt verdict`/`mt clarify`/`mt amend` | — |
| Аудитор як актор + тригери аудиту | РЕАЛІЗОВАНО | `audit.rs` `run_auditor`; черга — `orchestrator.rs` `audit_queue`; `audit_on_patch` — `signal.rs` `audit_policy` | — |
| EngineerAgent | РЕАЛІЗОВАНО | `runner.rs` `Actor`/`build_engineer_prompt`/`full_run_history`; CLI — `mt run --actor engineer` | — (GraphPatch реалізовано як дозволені втручання через штатні команди, окремого артефакту спека не задає) |
| `unresolvable` (3 тригери + алерт) | РЕАЛІЗОВАНО | `lib.rs` `unresolvable_reason`/`write_unresolvable`; алерт — `agent-server/orchestrator.rs` `pending_alerts`; доставка — push тип 3 (`relay/lib/push.mjs`) | — |
| Recurrence | ВІДСУТНЄ | — | Уся глава `recurrence.md` |
| Secrets broker | РЕАЛІЗОВАНО | `mt-core/secrets.rs` (сховища: Keychain / файл `0600` / `MT_SECRETS_FILE`, `resolve_keys`, `resolve_ref` для `secret:<key>`, `Masker`); інжекція й маскування — `runner.rs` | — (маскування ловить дослівне значення; закодовані форми — межа, названа в модулі) |
| Sandbox `skill_profiles` | ЧАСТКОВО | `mt-core/sandbox.rs` (політика: allowlist, мережа, fs-scope; ENV); жорстка відмова — ACP-гейт у `mt serve`; **ізоляція запису рівня ОС** — `mt-core/isolation.rs` (`sandbox.isolation: worktree`, macOS `sandbox-exec`) | Ізоляція покриває **лише файловий запис** і лише macOS (Linux — fail closed); мережа й allowlist команд лишаються декларативними |
| `.mt.json` — дефолти для реалізованого | РЕАЛІЗОВАНО | `config.rs` `config_defaults` | — (ключі нереалізованих фіч свідомо без дефолтів, див. «Закриті питання») |

## Протокол, сесії, поверхні

| Підсистема | Вердикт | Де в коді | Чого бракує |
| --- | --- | --- | --- |
| Envelope + Event v4 (усі варіанти, forward-compat) | РЕАЛІЗОВАНО | `agent-protocol/envelope.rs` | — |
| Хендшейк, `lang`, exact version check | РЕАЛІЗОВАНО | `agent-protocol/handshake.rs` | — |
| Ed25519: approvals + transfers | РЕАЛІЗОВАНО | `agent-protocol/approvals.rs`, `transfers.rs` | — |
| Сесії: журнал, seq, відновлення, глибокий реплей | РЕАЛІЗОВАНО | `agent-server/session.rs` `replay_from`/`replay_from_disk`, буфер `SESSION_BUFFER` | — |
| WS-транспорт, capability-фільтр, backpressure | РЕАЛІЗОВАНО | `agent-server/ws.rs` (наздоганяння журнальованих на `Lagged`, disconnect з `Error`, `MAX_FRAME_BYTES`) | — |
| Discovery / single-instance | ЧАСТКОВО | `agent-server/discovery.rs` | Живої перевірки stale-lock немає |
| Інтерактивний run = run вузла | РЕАЛІЗОВАНО | `agent-server/graph.rs` | Інтерактивні політики (`progress_timeout_sec`), телеметрія ходів |
| Handoff між хостами | ЧАСТКОВО | `agent-protocol` `HandoffRequest`/`HandoffAck`/`HandoffPull`; `agent-server/ws.rs` `pull_node`/`serve_handoff_request`, доставка — `relay_client.rs` (виняток host↔host з анти-циклу); CLI — `mt handoff` | Checkpoint-режим (`mt handoff --checkpoint`, git.md): дистильований summary + archive ref для повного журналу |
| Approvals-гейт mid-run + матеріалізація | РЕАЛІЗОВАНО | `agent-server/approvals_gate.rs` `ApprovalRecord::to_yaml_line`; запис — `ws.rs` → `InteractiveRun::add_approval` → `run_NNN.md` | — |
| ACP-транспорт | РЕАЛІЗОВАНО | `agent-core/acp.rs` | — |
| Orchestrator-роль у agent-server + wake | РЕАЛІЗОВАНО | `agent-server/orchestrator.rs` `Wake`/`Orchestrator::tick`; relay push → `AppState::wake_orchestrator` | — |
| `client_kind: mt-dashboard` | РЕАЛІЗОВАНО | фільтр стрічки — `agent-server/ws.rs` `allowed`/`is_graph_event`; емітер `NodeState` — `orchestrator.rs` `state_changes` (лише зміни), публікація — `broadcast_only` у циклі `mt serve` | — (піддерево не звужується на хості: спека кладе агрегацію на клієнта) |
| Surface-профілі + `ContextSelected` | РЕАЛІЗОВАНО | `mt-core/surfaces.rs` (парсер обох форм конфігу, резолюція hint→липкість→default, стеля `a.md.skills`, `check_context_kind`); хост — `agent-server/ws.rs` (`resolve_turn_surface`, гейт `ContextSelected`, surface у ехо `UserMessage`) | — |
| MCP-сервери surface | ЧАСТКОВО (розвиток спинено) | `mt-core/mcp.rs` (декларація, `mcp:<name>` у `tools`, резолв `secret:` через брокер, ACP-payload); доставка — `agent-core/acp.rs` `session/new`, збірка — `mt serve` | Звуження набору до surface ходу **свідомо не планується** — рішення Vitalii 2026-08-14, див. «Закриті питання». Чинний стан робочий: оголошується обʼєднання профілів проєкту; лінивий старт і `idle_ttl_sec` виконує ACP-виконавець |
| Preview-модуль (`PreviewScreenshot`, picker) | ВІДСУТНЄ | — | Подія й capability-фільтр є; самого модуля немає |

## Люди, доступ, relay

| Підсистема | Вердикт | Де в коді | Чого бракує |
| --- | --- | --- | --- |
| Ролі owner⊃host⊃approver⊃viewer | РЕАЛІЗОВАНО | `relay/lib/store.mjs`, `relay.mjs` | — |
| Схема даних relay | РЕАЛІЗОВАНО | `relay/lib/store.mjs` (dev), `sqlite-store.mjs` + `schema.sql` (персистентна), міграції колонок — `SqliteStore.migrate`, вибір — `create-store.mjs`; контракт — `tests/store-contract.test.mjs` | — |
| Auth акаунтів | ЧАСТКОВО | `relay/lib/auth.mjs` (`DevMagicAuth`, `KratosAuth`), вибір — `create-auth.mjs`; контракт — `tests/auth-contract.test.mjs` | Passkey/WebAuthn і recovery — у Kratos, свого flow relay не має; сумісність із живим Kratos не перевірена (stub-`fetch` доводить лише обробку форми `whoami`) |
| Реєстрація пристрою | РЕАЛІЗОВАНО | `relay/lib/relay.mjs` `registerDevice` за сесією, кадр `register_device`; запис — `store.mjs` | — |
| Ротація/revocation pubkey | РЕАЛІЗОВАНО | `relay/lib/relay.mjs` `rotateDevice`/`revokeDevice`, кадри `rotate_device`/`revoke_device`; store — `retireDevice`/`deleteDevice`/`devicesOf`, колонка `retired_at` + міграція | — (retired лишається в історії, з роздачі зникає одразу) |
| Membership: invite/accept/decline | РЕАЛІЗОВАНО | `relay/lib/relay.mjs` | — |
| Membership: зміна ролі / видалення | РЕАЛІЗОВАНО | `relay/lib/relay.mjs` `changeMemberRole`/`removeMember`, кадри `set_member_role`/`remove_member`; `MemberChanged {role: null}` на видалення | — (пониження/видалення останнього owner-а відхиляється — див. «Закриті питання») |
| Transfer ownership із підписом | РЕАЛІЗОВАНО | `relay/lib/relay.mjs`, `agent-protocol/transfers.rs` | — |
| Кімнати, буфер, viewer read-only | РЕАЛІЗОВАНО | `relay/lib/rooms.mjs` | — |
| Presence | РЕАЛІЗОВАНО | `relay/lib/presence.mjs` (ефемерний реєстр із TTL), гейти й трансляція — `relay.mjs` `announcePresence`/`dropPresence`/`presenceOf`, кадри `presence`/`who`; оголошує сам хост — `agent-server/relay_client.rs` (heartbeat 30 с) | — (`mt sessions` як поверхня — окремий рядок CLI) |
| Гейт 2 — plan-review з підписом | ЧАСТКОВО | `spawn.rs` | У фронтматері рішення немає блоку `approved_by` з підписом |
| Гейт 3 — аудит-вердикт людини | ЧАСТКОВО | `signal.rs` | Немає блоку підпису в `audit-result_NNN.md` |
| Push типів 1/2/3 | РЕАЛІЗОВАНО | маршрутизація — `relay/lib/push.mjs`; транспорт — `fcm-sink.mjs` (FCM HTTP v1) і `push-sink.mjs` (dev), вибір — `create-push.mjs`; контракт — `tests/push-sink-contract.test.mjs` | — (APNs окремо не робимо: FCM доставляє на iOS; доставка живим FCM не перевірена — stub-`fetch` доводить лише форму запитів) |

## i18n, retro, мандати

| Підсистема | Вердикт | Коментар |
| --- | --- | --- |
| i18n: контрактне ядро (base-мова, `source_hash`, «що перекладається», contract-aware сегментація) | РЕАЛІЗОВАНО | `mt-core/i18n.rs`: `I18nConfig`, `TranslationMeta`/`is_fresh`, `is_translatable` (триступенева схема), `segment` (fail-closed), `materialize` (read path, лише свіжі); неоднозначність спеки щодо `## Task`/`## Done when` вирішена явно — див. «Закриті питання» |
| i18n: сховище `refs/mt/i18n`, write path, черга регенерації, live-шар | ВІДСУТНЄ | Ядро є (рядок вище); запис/читання i18n-ref разом із fenced publish; компіляція authored-правки в base; фонова черга agent-server; live-переклад Envelope за capability `self-translate` (власник — relay, див. «Закриті питання») |
| i18n: `lang` у ClientHello | РЕАЛІЗОВАНО | `agent-protocol/handshake.rs`; далі хендшейку `lang` не використовується |
| retro: датасет і детерміновані пропозиції | РЕАЛІЗОВАНО | `mt-core/retro.rs` (`collect_runs`, `analyze`, приватний звіт `~/.nitra/retro/<period>.md`); CLI — `mt retro`/`mt retro --show`; — (opt-in `retro.enabled`, дефолт `false` — контрактна вимога глави) |
| retro: LLM-крок, innovation, impact | ВІДСУТНЄ | Детермінований датасет є (рядок вище); LLM-аналіз поверх датасету; `innovation_NNN.md` і baseline; impact-зрізи; фоновий прогін за `schedule_days`; агрегатор компетенцій як другий вихід |
| Мандати: карта, валідація змін, квіз-гейт, ШІ-мандати | РЕАЛІЗОВАНО | `crates/mt-mandates`: `parse_mandates`, `effective_owner`, `validate_mandate_change` (generation fencing, подвійний підпис `escalates_to`, «остання константа»), `validate_approval` |
| Мандати: `decision-request`, `awaiting-decision`, `chosen_option` | РЕАЛІЗОВАНО | `mt-core/decision.rs` (артефакт у `decisions/`, маркер стану у вузлі, відповідь), стан — `lib.rs` `detect_state`; CLI — `mt escalate`/`mt decide`; `chosen_option` — `agent-protocol` |
| Мандати: решта глави | ВІДСУТНЄ | — | Агент escalation-intake (фаза 1 — у фазі 0 тригер ручний, див. «Закриті питання»), маршрутизація за важелем, прецедентний рушій, селектор призначення, профілі компетенцій, process watcher |
| Подія `Escalation` | РЕАЛІЗОВАНО (інша річ) | Записка «вгору» за handle з owner-app spec, не mandates-розвилка: без карти, фасетів, варіантів і підписаного рішення |

## Хвилі робіт

Порядок обраний так, щоб кожна наступна хвиля спиралась на замкнений інваріант попередньої, а не на обіцянку.

1. **Контрактний борг — ✅ закрито.** `failed_streak` (категорія + межа), формат `a.md`/`h.md`, видалення `mt-napi`, `schema_version` fail-closed, гейт immutability, дефолти `.mt.json`, `orphan-node`, матеріалізація `result: merge-conflict`.
2. **Замкнути M0 як автономний цикл — ✅ закрито.** `unresolvable` з трьома тригерами, контекст агента, `run-summary.md`, git-протокол `spawn`/`invalidate`/`kill` з re-run семантикою і `mt stop`, аудит-цикл разом із агентом-аудитором, Stage 1, EngineerAgent. Це і є «перший продукт» зі стратегії: автономне досягнення мети з людиною на гейтах. Хвости, що належать orchestrator-ролі хвилі 3: алерт при `unresolvable` і тригери аудиту за розкладом (`audit_schedule_days`/`audit_on_patch`).
3. **M1 доведення + wake.** Orchestrator-роль, continuous backfill, remote claims у скані, `stalled`, злиття `agent-cli` у `mt serve|attach`, backpressure, глибокий реплей.
4. **M2 mission control.** Першим — матеріалізація підпису в `## Approvals` (це буквально demo-критерій), далі персистентний store ✅, auth ✅, push-транспорт ✅, `HandoffRequest` через relay ✅, presence ✅.
5. **M6 фаза 0 — ✅ закрито.** `mandates.yaml` (включно з `kind: model` і `audacity`), валідація змін, квіз-гейт — `crates/mt-mandates`; `decision-request` із `leverage_facets`, `chosen_option`, стан `awaiting-decision` і вихід «вичерпана драбина → розвилка» — `mt-core/decision.rs` + `mt escalate`/`mt decide`. Лишається агент escalation-intake: механіка розвилки є, судження «це вибір, а не баг» поки робить людина, що викликає `mt escalate`.
6. **M3 / M5 / M4.** Dashboard і поверхні (`client_kind: mt-dashboard` ✅, surface-профілі + `ContextSelected` ✅; MCP-сервери ✅; лишається preview-модуль); retro MVP ✅ (детермінований датасет і пропозиції; LLM-крок, innovation та impact — далі); файловий шар i18n (контрактне ядро ✅; сховище в ref-і, write path і черга регенерації — далі).

## Закриті питання

- **`mt escalate` вручну — штатний шлях фази 0** (2026-08-14): рішення Vitalii. Спека каже, що `decision-request` ніколи не пише виконавець — його пакує агент escalation-intake; це лишається метою, але фазою 1. Причина: автоматична класифікація «баг чи вибір» — рівно те судження, заради якого гейт існує, і фальшива евристика тут гірша за її відсутність (вона виглядала б як рішення системи). У фазі 0 тригер натискає людина, механіка розвилки далі повністю автоматична — маршрутизація адресата, `retry_history` з run-файлів, квіз-гейт. Інваріант «виконавець не пише сам собі розвилку» витриманий: `mt escalate` викликає не агент вузла.

- **`layers/` видалено з `mt-rust`** (2026-08-14): рішення Vitalii. Перевірка перед видаленням спростувала припущення, з яким питання формулювалось: у `mt-rust` `layers/` генерував **нуль** перекладів (`find . -name '*.en.md'` → 0). 18 англомовних дзеркал живуть у репо специфікації, і теки `layers/` там немає взагалі — їх робить інший інструмент. Тобто конфлікту «два рушії одного перекладу» в цьому репо не існувало: був мертвий воркспейс на 40 файлів із власним CLI, схемою і 95 тестами. Прибрано разом із записом у `workspaces`, скриптом `layers` і рядком `.v8rignore` (останнє заразом знімає одну з причин хронічно червоного v8r). Ядро `mt-core/i18n.rs` лишається єдиним рушієм перекладу вмісту графа.

- **Live-переклад Envelope робить relay** (2026-08-14): рішення Vitalii, ухвалене з відомим наслідком. **Це змінює модель довіри, а не лише місце коду:** `access.md` фіксує поведінкову межу relay — «не парсить payload далі роутінгових полів», а `overview.md` (принцип 4) називає relay ефемерним координатором. Переклад вимагає читати вміст текстових подій і віддавати його моделі, тобто вміст розмов виходить за периметр до провайдера моделі. **Спека потребує правки** в двох місцях: рядок relay у таблиці меж довіри (`access.md`) і принцип 4 (`overview.md`); інакше це мовчазне співіснування двох контрактів. Пом’якшення вже є в каноні й лишається чинним: «якщо вміст розмов не можна показувати навіть relay-оператору — розгортайте власний relay» (self-hosted-first). Окремо лишається інженерне: переклад дельт токен-за-токеном рве речення, тож одиницею перекладу має бути завершене повідомлення (`AgentTextDone`), а не `AgentTextDelta`.

- **Політика гейта — вузла, не хоста; вузол без `a.md` падає на стелю** (2026-08-14): рішення Vitalii. Заразом виправлено хибне твердження карти «гейт не знає вузла»: `PermissionFactory` отримує `node_hash`, тож звуження не потребувало зміни публічного API. Fallback саме на стелю хоста, а не на заборону всього: `a.md` описує **агентний** контракт, і його відсутність (людський вузол, ad-hoc кімната `mt attach`) не є заявою «нічого не можна» — заборона ламала б штатний сценарій заради принципу. `skills: []` читається так само, як відсутній список, інакше порожня декларація мовчки блокувала б вузол.

- **`directory.rs` видалено** (2026-08-14): рішення Vitalii. Ланцюг адресної ескалації мав три ланки, працювала одна: handle → email (парсер із тестами) є; email → `account_id` немає ніде — мапінг живе лише на relay, і кадру для запиту не існує; `Escalation` не надсилав **ніхто** (нуль емітерів по всіх крейтах). Наслідок був гірший за відсутність: `push.mjs` свідомо не розсилає подію без `to_account_id`, тож ескалація дійшла б нікому й без сліду, а рядок карти обіцяв частково готовий PII-шлях. Прецедент `mt-napi`: крейт без споживача видаляється. Парсер на 109 рядків перепишеться в M6 разом із емітером і з обдуманою межею резолву — кадр «резолвни акаунт за email» робить relay оракулом наявності акаунтів, тож мінімально безпечна форма мусить обмежуватись membership кореня.

- **Ізоляція — тільки файловий запис і тільки worktree** (2026-08-14): рішення Vitalii. `operations.md` називає fs-scope worktree, але не каже, чи ізолювати мережу й exec. Повна ізоляція ламає підписочні CLI (вони ходять у власні API і запускають тулчейни), тож її вимикали б назад цілком — а вузька межа, яку не хочеться вимикати, захищає більше за широку, яку вимкнули. Реалізовано `sandbox.isolation: worktree` (macOS `sandbox-exec`, вимкнено за замовчуванням): запис дозволений у worktree, `TMPDIR` і явно перелічені `sandbox.isolation_writable`; усе інше — read-only. Мережа й allowlist команд лишаються декларативними. На непідтримуваній платформі — **відмова run-а**, а не тихий запуск без ізоляції.

- **MCP: звуження до surface ходу не робимо** (2026-08-14): рішення Vitalii — розвиток MCP спинено. Аналіз трьох варіантів: (А) набір проєкту — контекст усіх тулів у кожному ході, але розмова безперервна; (Б) переоткриття ACP-сесії на зміну surface — рве **контекст розмови** заради економії контексту тулів, тобто платить найдорожчим за найдешевше; (В) кімнати по парі (вузол, surface) — вузький контекст без розриву, ціною N процесів і розділених тредів. Технічна межа: `mcpServers` задається лише в `session/new`, додати сервер у живу сесію нічим. Чинний варіант А лишається робочим; рядок карти лишається `ЧАСТКОВО` не як борг, а як зафіксована межа.

- **Останнього owner-а не понизити й не прибрати** (2026-08-14): `access.md` описує Membership API (`PATCH role` / `DELETE`) і окремо каже, що для **зниклого єдиного owner-а** потрібна адміністративна процедура оператора relay «за явною згодою всіх учасників із роллю host». Тобто стан «задача без власника» спека визнає, але виводить із нього не API, а ручною процедурою. Рішення: звичайний API такий стан **створити не може** — пониження або видалення останнього owner-а відхиляється. Дешевше не дати створити стан, ніж будувати з нього вихід; штатний шлях лишається той самий — `transfer ownership` або другий owner наперед (succession зі спеки).

- **Sandbox не вмикається сам** (2026-08-13): `operations.md` каже «команда поза allowlist → відмова», але не каже, що робити з проєктом, який `skill_profiles` не налаштував. Deny-by-default для такого проєкту зламав би кожен наявний вузол мовчазною відмовою — і «безпека» звелася б до того, що її вимикають назад. Рішення: **секції немає → політика не enforcing** (поведінка як до її появи); **секція є → у її межах allowlist жорсткий, а `network` вимкнено, доки його не ввімкнули явно**. Це властивість коду (`Policy::is_enforcing`), а не домовленість у коментарі.

- **`## Task`/`## Done when` перекладаються, заголовки — ні** (2026-08-13): `i18n.md` перелічує ці дві секції серед «контрактних, які парсить скрипт», і поруч каже «перекладається лише людський текст між ними» — а це і є той людський текст. Читати список буквально означало б, що вузол не перекладається взагалі, тобто крос-мовність (ключовий вимір `vision.md`) не працює саме там, де потрібна. Рішення: **заголовки всіх секцій лишаються base завжди** — вони і є те, що парсить скрипт; тіла машинних секцій (`Check`, `Children`, `Inputs`, `Approvals`, `Ref`) — теж; тіла `Task`/`Done when` перекладаються. Fail-closed від цього не страждає: жоден рядок, який читає машина, перекладачу не видно. Прив'язано тестом до констант `MACHINE_SECTIONS`/`PROSE_SECTIONS`.

- **Маркер `awaiting-decision` у теці вузла** (2026-08-12): спека кладе `decision-request` у run branch (`refs/mt/runs/{run-id}/decisions/`), і це не змінено. Але derived-стан рахує `scan_tasks` по **робочому дереву**, куди run branch не розгорнутий — тобто зі спеки як є стан `awaiting-decision` був би невидимий для `mt status`. Рішення: артефакт лишається там, де велить спека, а в теці вузла лежить маркер-**вказівник** `awaiting-decision_NNNN.md` (і `decided_NNNN.md` на закриття). Це той самий патерн, що вже діє для `pending-audit_NNN.md`, чий стан теж матеріалізований маркером, а зміст живе окремо. Дублювання вмісту свідомо немає — два джерела істини розійшлися б.

- **Сховище relay: SQLite замість PostgreSQL** (2026-08-11): рішення Vitalii. `stack.md` фіксує «Bun + PostgreSQL», але обсяг персистентних даних relay — акаунти й membership, тобто одиниці мегабайтів; SQLite дає персистентність без інфраструктури і, що важливіше, **перевіряється в тестах на кожній машині**, а не лише там, де піднято БД (PG-варіант лишався б прогнаним лише за наявності `RELAY_DATABASE_URL`). Контракт store параметризований, тож перехід на PostgreSQL — заміна однієї реалізації, не переписування relay. **Розбіжність зі спекою закрито** ([nitra/mt#69](https://github.com/nitra/mt/pull/69)): `stack.md` більше не називає один продукт БД — там тепер інтерфейс store і три дозволені реалізації (in-memory, SQLite, PostgreSQL), а нормативною умовою є проходження спільного контрактного набору.

- **Обсяг дефолтів `.mt.json`** (2026-08-09): рішення — дефолт додається разом із кодом, що його читає, а не наперед за списком зі спеки. Причина: `merge_config` пропускає **будь-який** ключ користувача наскрізь, тож відсутній дефолт нічого не ламає — а два десятки констант для нереалізованих фіч (аудит-цикл, recurrence, i18n, relay, surfaces) були б мертвим конфігом, який виглядає як реалізована поверхня. Закрито реальне розходження: `agent_retry_max` читався в обхід `config_defaults` власним хардкодом `3`.

- **Обсяг гейта immutability** (2026-08-09): спека називає `mt done`/`mt audit` — `failed` свідомо **не** гейтиться. Причина: сенс гейта в тому, що виконавець не може переписати власний контракт (послабити `## Done when`, викинути рядок `## Check`, підняти собі `model_tier`) і на цьому **оголосити успіх**; провал контракту не привласнює, а блокування `failed` лише сховало б діагностику ретраю. Гейт fail-open там, де порівнювати нема з чим (немає репо, немає `origin/main`, файлу ще немає в базі) — це стан «до worktree», у якому спека дозволяє вільні правки.

- **Строгість `schema_version`** (2026-08-09): спека каже і «перше поле всіх файлів із фронтматером», і «невідома версія → fail closed». Реалізовано двома рівнями, бо це дві різні вимоги: **невідома версія** (число не наше або взагалі не число) — жорстка відмова на гейтах `preflight`/`signal`/`spawn`, тобто вузол не виконується й результат не публікується; **відсутнє поле** — попередження скану, не відмова. Причина: файл без поля не є файлом з майбутньої схеми, тож читати його безпечно, а вимога «перше поле» адресована запису. Усі 47 фронтматер-файлів dogfood-графа поле мають, тож попередження не шумить.

- **`mt-napi`** (2026-08-09): рішення — видалити з репо. Питання було поставлене неточно («лишився всупереч рішенню Г»): рішення Г від 2026-07-23 скасувала специфікація від 2026-07-27, яка свідомо повернула крейт заради `@7n/rules`. Але 2026-07-30 `@7n/rules` зробив власний `rules-core`/`rules-napi` із власним `worktree.rs` — і крейт лишився **без жодного споживача**, тягнучи CI-матрицю zigbuild і два платформні npm-підпакети. Видалено `crates/mt-napi`, `npm/mt-napi`, `packages/`, `npm-publish.yml`; опубліковані версії в npm не знімались (unpublish зламав би невідомих зовнішніх споживачів).

- **Формат `a.md`/`h.md`** (2026-08-09): рішення — **YAML-фронтматер у markdown**, жорсткий перехід. Ні голий YAML (як пропонувала спека), ні markdown-секції (як робив код): файл має розширення `.md`, тож фронтматер узгоджений з рештою `.md`-артефактів, перевикористовує один код-шлях `parse_front_matter`, рендериться при рев'ю і лишає місце для прози в тілі. Голий YAML лишається контрактом `.yml`-файлів (`.mt-claim.yml`). Спеку уточнено (nitra/mt), 19 файлів dogfood-графа мігровано, старий формат відхиляється явною помилкою в preflight — тести `preflight_rejects_legacy_section_flag`, `preflight_rejects_flag_without_frontmatter`.
- **Межа `failed_streak`** (2026-08-09): рішення — спека виграє, код приведено до неї. Межа — останній *прийнятий* fact (`accepted_fact_nnn`); відхилений аудитом fact межу не рухає. Причина: інакше цикл «провал → провал → сирий fact → аудит відхилив» обнуляє лічильник вічно, і драбина ретраїв ніколи не доходить до EngineerAgent чи `unresolvable` — livelock. Тест-сторож: `rejected_fact_livelock_terminates`.

## Відкриті питання

Конфлікти зрізу 2026-08-09 закриті; нижче — питання, що виникли під час реалізації і чекають рішення.

1. **Rust-тести без CI-гейта.** `cargo test` не ганяє жоден workflow. Просто додати не можна: частина тестів драбини міряє реальний час і чутлива до навантаження (спостережено 2026-08-14: під паралельним clippy+лінтом падає `budget-exceeded` замість `success`, щоразу в інших тестах), а червоний, що блимає, навчаються ігнорувати. Спершу розвʼязати їх від годинника (інʼєкція, як уже зроблено в `presence` і `push`) або винести в окремий повільний джоб — і аж тоді вмикати гейт. Часткове полегшення вже зроблено: `PATH_LOCK` більше не отруюється, тож одна невдача не тягне за собою десяток `PoisonError`.
