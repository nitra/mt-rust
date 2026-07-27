# napi-rs біндинг для `mt` (заміна spawn-виклику CLI з Node/Bun-споживачів)

**Дата:** 2026-07-27
**Статус:** погоджено — готово до реалізації
**Зв'язані документи:** [2026-07-23-mt-cli-rust.md](2026-07-23-mt-cli-rust.md), історичний прототип `crates/mt-napi` (napi v3, phase 0+1) у старому репозиторії `github.com/nitra/mt` (коміт `56c5f0e`)

## 1. Проблема / Мета

Node/Bun-споживачі `mt` (найпомітніший приклад — `auto-worktree.mjs` у `@7n/rules`,
`npm/scripts/lib/auto-worktree.mjs`) викликають CLI через `spawnSync('npx', ['@7n/mt', 'worktree', 'create', ...])`
на кожен цикл worktree-лайфсайклу. Це дає:

- overhead запуску сабпроцесу (fork/exec + `npx`-resolve) при частих викликах;
- втрату структури даних — результат парситься з stdout (текст або `--json`), а не отримується як типізований JS-об'єкт;
- залежність Node-споживача від встановленого системного бінарника `mt` (crates/mt), хоча логіка вже є Rust-бібліотекою (`mt-core`).

Мета — дати Node/Bun-процесам прямий виклик функцій `mt-core` без сабпроцесу, зберігаючи CLI (`crates/mt`) як
основний, незалежний інтерфейс.

## 2. Ухвалені рішення

| # | Питання | Рішення |
|---|---|---|
| А | Транспорт Node↔Rust | **napi-rs** аддон (`crates/mt-napi`) над `mt-core`. Відкинуто: `bun:ffi`/`dlopen` — підриває мету "структуровані дані" (без ручного маршалінгу складних типів довелось би або JSON-серіалізувати через FFI-межу, або ризикувати пам'яттю); `worker_threads` — не окрема альтернатива, а опційний тюнінг поверх napi-rs для довгих команд, не в MVP. |
| Б | Цільовий runtime | **Тільки Bun.** Чистий Node.js — не ціль цієї ітерації (mt-js README згадує `node bin/mt.js`, але реальний біль і головний споживач — `7n-rules`, повністю на Bun). Bun має Node-API сумісність, тож стандартний napi-rs `.node`-аддон працює без адаптацій. |
| В | API-контракт | Відновити `crates/mt-napi`, **але не 1:1 зі старою формою** (стара версія була написана поверх дотодішнього `scanner/`-підходу, до появи сьогоднішнього clap-based `mt-core`/`crates/mt`). Новий napi-шар — тонка обгортка над **поточним** `mt-core` (`crates/mt-core` у цьому репозиторії), API-форма узгоджується з сьогоднішньою структурою команд (`crates/mt/src/commands/*`), а не переноситься зі старого репо. |
| Г | Обсяг MVP-функцій | `worktreeCreate`, `worktreeRemove`, `worktreeStatus` (найбільший поточний біль — `auto-worktree.mjs`) **+** `scan`, `plan`, `spawn` (часто викликані команди графа задач). Інші команди (`run`, `auto`, `done`/`audit`/`failed`, `kill`/`invalidate`, `doctor`) — поза MVP, додаються пізніше за потреби. |
| Д | Дистрибуція | Відновити platform-пакети `@7n/mt-darwin-arm64` / `@7n/mt-linux-x64` (той самий набір таргетів, що вже є в `release-mt.yml` для CLI-бінарників: `aarch64-apple-darwin`, `x86_64-unknown-linux-musl`) з публікацією через `JS-DevTools/npm-publish` + `--provenance`/OIDC (`id-token: write`), як було в старому `mt`-репозиторії (`.github/workflows/npm-publish.yml`, коміт `166d770`). |
| Е | Fallback-стратегія | napi-модуль з fallback на CLI-spawn, якщо `.node`-біндинг не зібраний під платформу користувача — аналог старого `native.mjs`-лоадера (`MT_NATIVE_ADDON` env → platform-пакет → `target/`-fallback у розробці). |
| Ж | Типобезпека | Обов'язкова частина MVP, не окремий етап: napi-rs derive-макроси (`#[napi]`) генерують `.d.ts` напряму з Rust-типів; capability-based функції (`worktreeCreate()`, `scan()` тощо) замість generic `runCommand(argv: string[])`. |
| З | Regression-safety | Contract-тести, що порівнюють вивід napi-виклику з еквівалентним CLI-викликом (`mt --json ...`), щоб не розійтись поведінкою двох інтерфейсів. |

## 3. Деталі реалізації

**Структура крейтів:**

- Новий крейт `crates/mt-napi` (за зразком видаленого з `mt`, але залежить від `mt-core` цього репозиторію, не від старого `scanner/`).
- `mt-napi` — тонкий шар: кожна napi-функція викликає відповідну функцію `mt-core`, конвертує результат у типізовану napi-rs структуру (`#[napi(object)]`), помилки — у `napi::Error` з кодом, що відповідає кодам виходу CLI.

**Функції MVP (Кластер 3 сесії):**

- `worktreeCreate(branchSuffix: string, description: string): WorktreeCreateResult`
- `worktreeRemove(branchArg: string): void`
- `worktreeStatus(): WorktreeStatus[]`
- `scan(root?: string): GraphScanResult`
- `plan(taskPath: string, content: string): PlanResult`
- `spawn(planPath: string, decision: 'approve' | 'reject'): SpawnResult`

Точні поля структур узгоджуються з існуючими `commands::worktree`, `commands::graph`, `commands::plan` у `crates/mt/src/commands/` — napi-функції викликають ту саму логіку `mt-core`, яку сьогодні викликають ці CLI-команди (не дублювати бізнес-логіку в `mt-napi`).

**Node/Bun-споживач (`npm`-пакет):**

- `native.mjs`-лоадер: `MT_NATIVE_ADDON` (явний override) → platform-пакет (`@7n/mt-darwin-arm64`/`@7n/mt-linux-x64` через `optionalDependencies`) → fallback на `spawnSync('mt', ...)` (той самий контракт, що й `mt-js/bin/mt.js` сьогодні), якщо жоден `.node`-біндинг не резолвнувся.
- `auto-worktree.mjs` у `7n-rules` — перший реальний споживач; міграція з `spawnFn('npx', ['@7n/mt', 'worktree', ...])` на виклик napi-функції — окрема задача **після** того, як `mt-napi` опублікований (не входить у цю специфікацію).

**CI/публікація:**

- Новий workflow (за зразком `release-mt.yml` + історичного `npm-publish.yml`): збірка `.node` для `aarch64-apple-darwin` і `x86_64-unknown-linux-musl` через `napi build --release` (або `cargo zigbuild` + `napi-rs` артефакт-скрипт), публікація platform-пакетів, потім головного `@7n/mt-napi` з `optionalDependencies`, все — з `id-token: write` та `--provenance`.

**Edge cases з генерації:**

- Версія `.node`-аддону має бути прибита до версії `mt-core`, щоб уникнути дрейфу поведінки між CLI і napi-шляхом (та сама версійна схема, що й CLI-реліз `mt-v*`).
- Async-виклики (навіть у MVP-функцій типу `scan`, які можуть бути I/O-важкими на великому графі) — через napi-rs `AsyncTask`/tokio, не синхронний блокуючий виклик, щоб не блокувати Bun event loop навіть без окремого `worker_threads`.

## Відкриті питання

- Точна назва npm-пакета для головного napi-аддону (`@7n/mt-napi` як у старому репо, чи інша назва, що не конфліктує з `@7n/mt` = специфікація) — визначити перед першим публікуванням.
- Чи потрібен окремий workflow-файл, чи розширення `release-mt.yml` — вирішується на етапі реалізації CI.
