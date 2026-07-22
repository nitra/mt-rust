# Rust-порт run-оркестрації доведено до паритету; run.mjs — тонкий клієнт mt-core

**Status:** Accepted
**Date:** 2026-07-14

## Context and Problem Statement

Після ADR `260713-2110` (ACP, підписочні CLI, каскад) run-оркестрація існувала у двох реалізаціях, що розійшлися: (1) `npm/lib/commands/run.mjs` — актуальний шлях із таблицею `AGENT_CLIS` (claude | codex | cursor | pi), каскадом `MT_CLOUD_AGENT_CLIS`, ENV-конфігом виконавців і спільним `## Check`-гейтом, але на **локальній** worktree-моделі 0.2.x (гілка `mt/<task-epoch>`, mkdir-lock, локальний merge); (2) `crates/mt-core/src/runner.rs` — порт спекового git-режиму (CAS claim → detached worktree від `origin/main` → watchdog → fenced publish), але із захардкодженим `DEFAULT_AGENT_CMD` `claude …`, без agent_cli/каскаду/ENV. «Правило одного коду контракту» (`npm/docs/architecture/stack.md`) забороняє дві імплементації; водночас сам stack.md суперечив собі: розділ правила казав «контракт один раз у `@7n/mt`, agent-server викликає `mt … --json`», а розділ контракт-пакета — «Rust (mt-core) — єдина імплементація, JS — тонкий клієнт». Фактично `agent-server/src/graph.rs` уже лінкує `mt-core` (claims/publish/worktree/signal) для інтерактивного шляху.

## Considered Options

* Довести Rust-порт до паритету і зробити `run.mjs` тонким клієнтом (napi).
* Видалити `runner.rs`/`orchestrate.rs` з mt-core до появи ACP-клієнта; канон — `run.mjs`.
* Статус-кво (дві реалізації, що розходяться далі).

## Decision Outcome

Chosen option: «паритет у Rust, run.mjs — тонкий клієнт», because mt-core вже є єдиною імплементацією контракт-примітивів для інтерактивного шляху (`graph.rs`), тож автономний шлях у JS робив би contract-логіку двоядерною назавжди; паритет закриває розбіжність в один бік — той, що зафіксований розділом контракт-пакета stack.md.

- **Паритет виконавців у `mt-core`:** ENV-конфіг — `config.rs` (`AgentCliEnv`: `MT_AGENT_CLI` / `MT_CLOUD_AGENT_CLIS` / `MT_AGENT_CLI_MODEL_MAP`; `normalize_model_tier`, `resolve_model_for_cli`); `runner.rs` — таблиця CLI (claude | codex | cursor → `cursor-agent` | pi) з headless-argv, каскад за rate-limit (текстова евристика — тимчасово, до структурованих ACP-помилок), a.md-прапори (`## Model tier`, `## Retry ladder`, `## Agent cli`), retry ladder з ескалацією тиру MIN→AVG→MAX, ENV-контракт `MT_*` (у т.ч. `MT_RETRY_STRATEGY`, `MT_MODEL_TIER`, ISO `MT_STARTED_AT`), фактичний `agent_cli` у frontmatter `run_NNN.md`. `DEFAULT_AGENT_CMD`/`agent_cmd` видалені. Точка розширення `node_executor` **не портована**: її видалено з контракту паралельним рішенням (PR #48, останній консюмер мігрував на підписочні CLI) — єдиний agent-шлях у Rust-порті одразу канонічний.
- **`run.mjs` — тонкий клієнт:** napi-експорти `run_node` / `run_auto` / `run_preflight`; у JS лишаються argv, резолв `mt_dir`, human-шлях (інструкції без спавну і без claim) і мапінг помилок в exit-коди (`claim-lost` → 2 — штатний skip). Поведінкові тести run переїхали в cargo (PATH-шими фейкових CLI); vitest перевіряє wiring тонкого клієнта.
- **`mt run` переходить на git-режим спеки:** CAS claim → detached worktree від `origin/main` → fenced publish в `origin/main`; вимагає push-доступ до `origin`. Локальна модель 0.2.x (гілка `mt/<task-epoch>`, mkdir-lock, локальний merge, `max_worktrees`-гейт у run) видалена разом зі старим кодом.
- **`## Check` — спільна семантика `signal.rs`:** виконується з кореня worktree (як у `mt done`), а не з директорії вузла (стара run.mjs-поведінка відкинута). Fact із проваленим `## Check` **відкликається** (видаляється до publish): `accepted_fact_state` рахує лише файли, і опублікований fact поруч із failed-run хибно робив би вузол resolved — виправлення й для старого Rust-шляху.
- **stack.md вирівняно:** «Правило одного коду контракту» тепер прямо каже — єдина імплементація в `mt-core`, `@7n/mt` — тонкий napi-клієнт, `agent-server` лінкує crate; це і був «окремий ADR про перенесення контракту в Rust», який розділ анонсував.

### Consequences

* Good, because зникає остання двоядерність контракту: каскад/тири/Check однакові для автономного (runner) та інтерактивного (graph.rs) шляхів, з одними тестами в cargo.
* Good, because автономний шлях отримує спековий claim/fenced-publish (мультимашинна коректність, run ref для recovery) замість локального merge без клеймів.
* Good, because закрита діра з хибним resolved при проваленому `## Check`.
* Bad, because `mt run` тепер вимагає git-репозиторій з `origin` і push-доступом — offline/без-remote сценарій свідомо втрачено (повернеться хіба окремим рішенням про local-режим).
* Bad, because napi-виклик блокуючий (синхронний run у процесі CLI); для довгих ранів це прийнятно (CLI і так чекає), для agent-server — не використовується (він має власний шлях).
* Bad, because JS-юніт-тести більше не покривають поведінку runner-а — планка тепер у cargo-тестах (13 тестів runner, включно з каскадом через PATH-шими).

## More Information

- `crates/mt-core/src/runner.rs`, `crates/mt-core/src/config.rs` (`AgentCliEnv`), `crates/mt-core/src/signal.rs` (`done_fm`/`audit_fm`), `crates/mt-napi/src/lib.rs` (`run_node`/`run_auto`/`run_preflight`).
- `npm/lib/commands/run.mjs` — тонкий клієнт; `npm/lib/tests/run.test.mjs` — wiring-тести.
- `npm/docs/architecture/stack.md` — «Правило одного коду контракту» (оновлено), компонентна таблиця (`mt-core`).
- `npm/docs/architecture/runtime.md` — «Підписочні CLI-виконавці», «Зовнішній екзекутор вузла» (нормативний контракт — без змін, реалізація тепер у Rust).
- ADR `260713-2110` — ACP як єдиний транспорт, ENV-конфіг, каскад, MIN-канон; ADR `20260613-071723` — заміна JS-сканера на Rust-шим (перший крок цього ж напряму).
