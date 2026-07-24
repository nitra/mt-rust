# Реімплементація `mt` CLI на Rust у mt-rust

**Дата:** 2026-07-23
**Статус:** погоджено — готово до реалізації
**Зв'язані документи:** `crates/mt-core/`, `crates/mt-cli/` (транзиційний `mt-scanner`, видаляється), `crates/agent-cli/`, `crates/mt-napi/` (видаляється)

## 1. Проблема / Мета

Старий JS CLI `@7n/mt` (команди `setup/init/plan/verify/run/status/scan/watch/audit/done/failed/spawn/invalidate/kill/worktree`) переїхав із `nitra/mt` у `nitra/mt-js` і з версії 0.29.0 не публікується в npm — dogfood-граф задач у `mt-rust/mt/` лишився без повноцінного CLI. Наявний `crates/mt-cli` (бінарник `mt-scanner`) вміє лише `scan`/`workspaces`/`create` — тонкий транзиційний шар, задокументований у власному `Cargo.toml` як тимчасовий до переходу на napi-аддон.

Мета — новий `mt` CLI, написаний на Rust і покладений у `mt-rust`, що:

- розблоковує dogfood (`mt-rust/mt/`) без залежності від JS/napi;
- прибирає napi/subprocess-посередників — CLI лінкується на `mt-core` напряму;
- ревізує командну поверхню старого CLI там, де семантика це виправдовує, а не копіює 1:1.

## 2. Ухвалені рішення

| # | Питання | Рішення |
|---|---|---|
| А | Крейт | Новий crate `mt` (не перейменування `mt-cli`) — чисто закриває історію "transitional mt-scanner"; `crates/mt-cli` видаляється разом зі своїм застарілим коментарем про перехід на napi. |
| Б | Бінарник | Назва лишається `mt` — сумісність із dogfood-звичками/скриптами/докою. |
| В | Міст до ядра | Прямий статичний лінк `mt` → `mt-core` (звичайні виклики функцій у межах одного процесу). Без napi (`dlopen`), без subprocess+JSON-pipe до окремого бінарника. |
| Г | `mt-napi` | Видаляється повністю — підтверджено відсутність інших споживачів (VS Code extension тощо не існує). |
| Д | Парсер аргументів | `clap` (`Parser`/`Subcommand`) — той самий стиль, що вже в `agent-cli`. Ручний `parseGlobalOptions` зі старого CLI не переноситься. |
| Е | Конфіг | Завантаження `.mt.json` — через наявний `mt-core::config.rs` (`merge_config`/`effective_config`), не власний парсер у CLI-шарі. |
| Є | `--json` | Уніфіковано на **кожній** підкоманді (не лише `status`/`scan`), одна канонічна JSON-схема/версія на всі виводи (`TaskState` вже serde snake_case — база для схеми). |
| Ж | Помилки | Plain stderr, без emoji-стилю старого CLI (`❌ Помилка...`) — пріоритет скриптовності над людяністю виводу. |
| З | `setup` | Зникає як окрема команда — `init` сам бутстрапить `.mt.json`/`mt/` при першому запуску, якщо їх нема. |
| И | `done`+`failed` | Зливаються в `mt run-result --outcome=done\|failed` — обидва пишуть `run_NNN.md` через спільний `writeRunFile`/`nextRunNNN`, різняться лише merge-worktree (done) чи залишенням worktree (failed, для дебагу). |
| І | `kill`, `invalidate`, `audit`, `verify`, `plan`, `spawn` | **Не** зливаються між собою і з И — семантика й побічні ефекти різні: `kill` видаляє worktree + скидає `plan_*.md` + каскадно інвалідує залежних (найважча операція); `invalidate` — лише sentinel-файл з опційним каскадом; `audit` — рецензентський запис (`pending-audit_NNN.md`) + merge worktree, окремий актор від `verify` (self-check виконавця зі своєї task-директорії, Stage 2); `plan` пише пропозицію (Stage 1), `spawn` лише перевіряє що діти вже зареєстровані (не матеріалізує їх). |
| Ї | `audit`/`status` | Обидва отримують дані з `mt-core::ledger::build_cost_ledger` (бюджет/cost overrun) — старий JS CLI цього не мав. |
| Й | `watch` | Перейменовується в `check` — стара назва вводила в оману (це one-shot attention/CI-гейт скан: pending-audit без результату, stale worktrees, needs-plan; exit 0/1 — не fs-watch loop). |
| К | `mt auto` | Нова команда — батч-виконання поверх наявних `mt-core::orchestrate::run_auto`/`sort_for_auto` (яких у старому CLI не було, ядро вже готове). Включено в цей же PR. `check` і `auto` лишаються окремими командами — різна мета (гейт-перевірка vs виконання). |
| Л | `claim`/`release`/`publish` як окремі user-facing команди | **Не входять у v1.** `run_node` вже сам викликає `acquire_claim` (`runner.rs:662`), `fenced_publish` — вже викликається зсередини run/audit-флоу. Debug-обгортка (напр. звільнити зависший claim вручну) відкладена — не блокує dogfood. |
| М | Інтеграція з `agent-server` | **Поза скоупом цього CLI-PR.** `agent-cli`/`agent-server` мають викликати `mt-core` напряму як Rust-бібліотеку (без спавну `mt`-процесу з `--json`) — окрема інтеграційна задача; докстрінг у `agent-cli/src/main.rs`, що описує старий план через subprocess, оновлюється окремо. |
| Н | `worktree` | Підкоманди `create`/`remove` — з наявного `mt-core::worktree.rs` (`create_run_worktree`/`remove_run_worktree`). `list`/`prune`/`inventory` — **нового коду в ядрі немає**, пишеться з нуля в межах цього PR. |
| О | Нова команда `mt doctor` | Діагностика `.mt.json`/`mt/`-layout/git-стану — явний, ідемпотентний варіант того, що раніше приховано робив `setup`. |
| П | Rollout | Усе одним PR/задачею (не фазами) — повний перегляд command surface одразу, а не поетапна відповідність старому CLI. |
| Р | Тести | Розширити наявний `mt-core::test_support::TestRepo` до full-CLI black-box e2e (спавн `mt`-процесу, перевірка stdout/exit code). Прогін проти живого dogfood-дерева `mt-rust/mt/` — **тільки read-only команди** (`scan`/`status`/`check` `--json`); мутуючі команди — виключно в ізольованих `TestRepo`-фікстурах. |
| С | Дистрибуція | **Без npm.** Встановлення — `cargo install --path crates/mt` (dogfood) та GitHub Releases (prebuilt-бінарники). Ім'я `@7n/mt` в npm зайняте специфікацією (з 0.29.0), а платформні підпакети `@7n/mt-darwin-arm64`/`-linux-x64` містили саме `mt-scanner`+napi-аддон — обидва артефакти видаляються; тека `packages/` і npm-publish для CLI-артефактів видаляються разом з ними. |
| Т | Shell-completions | Додати через `clap_complete` — старий CLI цього не мав. |
| У | Глобальний `--root DIR` | Переноситься зі старого CLI як глобальна clap-опція (виконати команду в іншому корені проєкту) — критично для `verify` (CWD-залежна) та оркестраторів. |
| Ф | `run` vs `run-result` — розмежування | `run` — шлях оркестратора: `run_node` сам робить claim → detached worktree → spawn виконавця → fenced publish. `run-result` — шлях виконавця/людини: зафіксувати результат **ручного** циклу (виконавець уже працює у своєму worktree, claim/publish поза цим викликом). Два шляхи не перетинаються: `run-result` не викликається для вузла, який веде `run`. |
| Х | Міграція `watch`→`check` | У цьому ж PR — grep по mt-rust (скрипти, доки, `mt/`-задачі) на згадки `mt watch` і оновлення; згадки у спека-репо nitra/mt — окремим PR туди. |
| Ц | Windows | Поза скоупом — build-таргети лише darwin-arm64 і linux-x64 (як у наявному CI); `.exe`-резолв старого JS не переноситься. |
| Ч | CI та конфіги | У скоуп PR входить чистка всіх слідів видалених крейтів: `npm-publish.yml` (build-matrix mt-scanner/napi — видаляється), `package.json` workspaces (`crates/mt-napi`), `knip.json`, `bun.lock`. |
| Ш | Чистка nitra/mt-js | mt-js залишається **тільки** тонкою обгорткою над crates з mt-rust: весь JS-код command-логіки (`lib/`, `types/`, `index.js`, тести) видаляється; лишається bin-шим, що резолвить і запускає Rust-бінарник `mt`. Публікація в npm — на паузі (коміт `e0f2c93`), поки npm-канал не знадобиться. Виконується окремим PR у nitra/mt-js після ландингу цього PR. |

## 3. Деталі реалізації

### Нова командна поверхня `mt`

```
mt init <name>              # створює задачу; бутстрапить .mt.json/mt/ при першому запуску (замінює setup)
mt plan <name>
mt spawn <name>
mt verify                   # з директорії задачі (CWD)
mt audit <name>             # + cost-ledger дані
mt run <name>               # claim/publish — внутрішньо, без user-facing claim/release
mt run-result <name> --outcome=done|failed   # замінює done+failed
mt kill <name>
mt invalidate <name> [--no-cascade]
mt status [name] [--json]   # + cost-ledger дані
mt scan
mt check                    # замінює watch; exit 0/1 attention-гейт
mt auto [--concurrency N]   # нове: run_auto/sort_for_auto
mt worktree create|remove|list|prune|inventory
mt doctor
```

Кожна команда — `--json` first-class output.

### Структура crates

- **Видалити:** `crates/mt-cli` (бінарник `mt-scanner`), `crates/mt-napi`, `packages/` (платформні npm-підпакети — дистрибуція без npm), а також їхні сліди в `npm-publish.yml`, `package.json` workspaces, `knip.json`, `bun.lock`.
- **Створити:** `crates/mt` (бінарник `mt`) — `clap`-based, тонкий шар над `mt-core`:
  - `init`/`doctor` → `mt-core::create_task`, `config.rs` (bootstrap-гілка нова: якщо `.mt.json`/`mt/` відсутні — створити).
  - `plan`/`spawn`/`verify`/`audit` → нова тонка логіка над `frontmatter.rs`/`nnn.rs` (аналог `npm/lib/commands/{plan,spawn,verify,audit}.mjs`, портовано на Rust).
  - `run` → `runner.rs::run_node`/`preflight` (уже інкапсулює claim).
  - `run-result` → новий helper у стилі `writeRunFile`/`nextRunNNN` (`nnn.rs::next_run_nnn`), з гілкою merge-worktree (done) / leave-worktree (failed).
  - `kill`/`invalidate` → `lifecycle.rs::kill`/`invalidate`.
  - `status`/`scan` → `lib.rs::scan_tasks` + `ledger.rs::build_cost_ledger`.
  - `check` → портована логіка `watch.mjs` (pending-audit без результату, stale worktrees, needs-plan), нова назва.
  - `auto` → `orchestrate.rs::run_auto`/`sort_for_auto`.
  - `worktree` → `worktree.rs` для `create`/`remove`; `list`/`prune`/`inventory` — новий код (git worktree list + `discover_worktrees` з `lib.rs` як основа для `list`; `prune`/`inventory` — нові функції в `mt-core::worktree.rs`).
- **Без змін:** `mt-core` (уся бекенд-логіка вже існує, крім `worktree list/prune/inventory` і `run-result`-хелпера).

### Тести

- Юніт-тести на нову CLI-логіку (bootstrap в `init`, `run-result`, `check`, `worktree list/prune/inventory`).
- Black-box e2e: розширити `test_support::TestRepo` спавном скомпільованого `mt`-бінарника, перевіркою stdout (текст + `--json`) і exit-кодів.
- Прогін на живому `mt-rust/mt/` dogfood-дереві — **лише read-only** команди (`scan`/`status`/`check` `--json`); мутації тільки в ізольованих `TestRepo`-фікстурах.

### Follow-up: чистка nitra/mt-js (окремий PR після ландингу)

Локальний клон: `/Users/vitaliytv/www/nitra/mt-js`. Репозиторій зводиться до єдиної ролі — тонка обгортка над crates з mt-rust:

- **Видалити:** `lib/` (вся JS command-логіка), `types/`, `index.js`, vitest/stryker-тулінг цієї логіки.
- **Залишити:** `bin/mt.js` як шим, що резолвить і `exec`-ає Rust-бінарник `mt` (аналог старого `scanner-bin.mjs`-резолвера: env-override → відомі шляхи → зрозуміла помилка), README з вказівником на mt-rust, CHANGELOG.
- Публікація в npm лишається на паузі (коміт `e0f2c93` "пауза публікації CLI") до появи потреби в npm-каналі.

### Що явно поза скоупом

- Standalone `mt claim`/`mt release`/`mt publish` (debug-tooling) — окрема майбутня задача.
- Перехід `agent-server`/`agent-cli` на прямі виклики `mt-core` замість (гіпотетичного) spawn `mt`-процесу — окрема інтеграційна задача; оновлення докстрінга в `agent-cli/src/main.rs` іде разом із нею, не в цьому PR.
- Windows-таргет.
- npm-канал дистрибуції CLI (якщо колись знадобиться — через mt-js-обгортку, окреме рішення).

## Відкриті питання

Немає — усі пункти брейншторм-сесії доведені до явного рішення користувача.
