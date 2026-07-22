## ADR Перенесення створення задач (write-side) у Rust-крейт `mt-scanner`

**Status:** Accepted
**Date:** 2026-06-14

## Context and Problem Statement

Принцип проєкту: *усе, що стосується роботи з файловою системою, має бути в Rust* — read-side
(скан) уже делеговано бінарнику `mt-scanner` (`scanner/src/lib.rs`). Створення задачі лишалося в
JS (`npm/lib/commands/init.mjs` + `buildTaskFrontMatter`), що дублювало файловий контракт і давало
дрейф. До того ж стара `mt init` писала `mode: human` у frontmatter, але **не** створювала
прапор `h.md` → свіжа задача сканувалася як `unassigned` замість `pending`. Спека:
`docs/spec-task-create-rust-integration.md`.

## Considered Options

* Лишити авторинг у JS — зберігає дрейф контракту й баг із прапором.
* (виконавець у frontmatter) тримати `executor.model_tier` у `task.md` — конфліктує з рішенням
  «істина = прапор `a.md`/`h.md`».
* (виконавець у прапорі) `a.md` як машинний YAML — vs markdown із секціями.

## Decision Outcome

Chosen: **створення задачі — у крейті `mt-scanner`**, симетрично до скану. Одна реалізація, три
споживачі (npm CLI shim, бінарник `mt-scanner create`, Tauri-команда в репо `task`).

* `pub fn create_task(tasks_dir, name, opts) -> Result<CreateOutcome, String>` + типи
  `Mode`/`CreateOpts`/`CreateOutcome` (serde) у `scanner/src/lib.rs`; підкоманда `create` у
  `main.rs` з JSON-виходом (`created: true/false`).
* `task.md`: `schema_version: 1` першим полем, `created_at` (через `chrono`), `budget_sec`, `hint`.
  **Без** `mode`/`executor`/`interactive`/`deps` у frontmatter.
* **Виконавець = прапор-файл**: `--mode agent` → `a.md` (markdown із секціями `## Model tier`,
  `## Skills`); `--mode human` → `h.md` (`## Qualification`, вільна форма). Ніколи обидва. Це
  виправляє баг із `unassigned`.
* **Залежності** — лише порожні `deps/<id>.md` (топологічне ребро); поле `deps:` прибрано.
* **Валідація імен** (`validate_name`, §8) **відхиляє** (не санітизує): сегменти `[a-z0-9-]+`,
  без великих літер/`_`/пробілів/`..`/traversal. Спільні тест-вектори Rust↔JS —
  `npm/lib/tests/fixtures/name-vectors.json` (`validateTaskName` у `state.mjs` — дзеркало).
* **Дефолти** з `.mt.json`: додано `default_mode: 'human'`, `default_model_tier: 'AVG'` у
  `CONFIG_DEFAULTS`; budget — наявний `default_budget_sec` (1800).
* **Атомарність**: запис tmp-файл + `rename`; при частковій відмові — відкат щойно створеної
  гілки директорій.
* `init.mjs` → тонкий шим: `spawnSync(bin, ['create', mtDir, name, ...flags])` + parse JSON;
  `buildTaskFrontMatter`/`mkdir`/`writeFile` видалено.
* `run.mjs` — `resolveExecutor` читає `model_tier` із `a.md` (секція `## Model tier`), fallback на
  старий frontmatter→`.mt.json` (інакше `--model-tier` губився б при `run`).

### Consequences

* Авторинг `task.md` має одне джерело істини (Rust); coverage переноситься в `cargo test`
  (38 тестів зелені).
* Свіжа задача одразу має коректний стан (`pending`/`waiting`) завдяки прапору.
* Контракт `a.md` тепер змістовний (раніше читалась лише наявність файла) — `run.mjs` його споживає.
* Tauri-команда `create_task` у репо `task` — окремий крок (крейт уже лінкується через
  `[patch]` на локальний `mt/scanner`).

## More Information

`docs/spec-task-create-rust-integration.md` (write-side), `docs/spec-scanner-rust-integration.md`
(read-side counterpart), `docs/mt.md` (файловий контракт вузла).

## Update 2026-06-14

### validate_name відхиляє некоректні імена замість sanitize

Специфікація §8 вимагає суворої відмови (exit 2) замість мовчазного виправлення символів. Нова функція `validate_name` (Rust) / `validateTaskName` (JS) відхиляє імена з uppercase, пробілами, `_`, `..`, traversal, порожніми сегментами, загальною довжиною > 100 символів. Існуючий `sanitize` у `scanner/src/lib.rs:183` залишається незмінним (використовується у worktree-matching з іншою семантикою).

### Спільні тест-вектори Rust↔JS у `name-vectors.json`

Для гарантування синхронності правил між реалізаціями використовується єдиний файл `npm/lib/tests/fixtures/name-vectors.json`. Rust-тести споживають його через `include_str!`, JS-тести — через `import ... with { type: "json" }` у `init.test.mjs`. Структура: `{ "valid": [...], "invalid": { "uppercase": [...], "spaces": [...], "underscore": [...], "double_dot": [...], "traversal": [...], "empty_segment": [...], "too_long": [...] } }`.

### Атомарний запис задачі через tmp-dir + rename

Запис відбувається у тимчасову директорію `<name>.<uuid>.tmp` всередині `tasks_dir`, після чого `fs::rename` переміщує її у фінальний шлях. При помилці tmp-директорія видаляється через `fs::remove_dir_all`. `fs::rename` атомарна в межах одного filesystem; розміщення tmp-dir у тому ж `tasks_dir` мінімізує ризик cross-filesystem операції. Залежність: `uuid = { version = "1", features = ["v4"] }` у `scanner/Cargo.toml`.

### chrono для ISO-8601 у created_at

Поле `created_at` у frontmatter `task.md` генерується через `chrono::Utc::now().to_rfc3339()`. Залежність: `chrono = { version = "0.4", features = ["serde"] }` у `scanner/Cargo.toml`.

## Update 2026-06-14

Драфт уточнює вже прийняте рішення про write-side створення task з JS CLI, Rust API і Tauri bridge.

- CLI контракт: `mt init <name> [--mode agent|human] [--model-tier AVG|MAX] [--budget-sec 3600] [--hint "..."] [--dep upstream]`.
- Exit codes CLI: `0` = created/exists, `1` = usage-помилка, `2` = validate/FS-помилка.
- Rust crate `mt_scanner` відкриває API `create_task(tasks_dir, name, CreateOpts)` і повертає `CreateOutcome::Created` або `CreateOutcome::Exists`.
- Defaults у Rust API: `mode` з `.mt.json`, `model_tier` з `.mt.json`, `budget_sec` default `1800`, `hint` default `"atomic"`, `deps` default `[]`, `skills` default `["bash", "write-files"]`.
- Tauri command `create_task` повертає `{ created, name, task_path, flag?, deps? }`, де `flag` дорівнює `"a.md"` або `"h.md"`.
- `validate_name` дозволяє тільки сегменти `[a-z0-9-]` через `/`; заборонені `..`, uppercase, пробіли та `_`.
