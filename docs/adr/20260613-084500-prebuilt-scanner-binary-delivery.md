## ADR Доставка Rust-сканера `mt-scanner` через 2 prebuilt-підпакети + видалення JS-реалізації

## Context and Problem Statement

У проєкті існували дві паралельні реалізації сканування DAG задач: Rust-бінарник `mt-scanner`
(`scanner/src/`) і чистий JS `npm/lib/core/scanner.mjs` + `deriveNodeState` у `state.mjs`. Вони
дивергували (різний порядок пріоритетів станів) і JS жодного разу не викликав Rust. Рішення:
JS-реалізації не повинно існувати — npm має делегувати сканування Rust-бінарнику. Це впиралося в
невирішений блокер: **як доставляти бінарник у рантаймі** (`target/` у `.gitignore`, у `files`
його немає, build-кроку в npm немає → опублікований `@7n/mt` не мав звідки взяти бінарник).

Уточнено також принцип: **усе, що стосується роботи з файловою системою, має бути в Rust** —
включно з деривацією `running` від активного git-worktree.

## Considered Options

Доставка бінарника:
* (A) postinstall `cargo build` — вимагає Rust-тулчейн у кожного користувача CLI.
* (B) Prebuilt-бінарники по платформах через `optionalDependencies` (модель esbuild/swc).
* (C) Зібрати локально й покласти бінарник у `files` — крихко для чужих платформ.

Набір платформ (для B): від мінімального (mac+linux) до повного (9 матриць як esbuild).

## Decision Outcome

Chosen: **(B) prebuilt через `optionalDependencies`**, на старті — **рівно 2 підпакети**:

| Підпакет | os/cpu/libc | Rust target | покриття |
|---|---|---|---|
| `@7n/mt-darwin-arm64` | darwin/arm64/— | `aarch64-apple-darwin` | усі Apple Silicon |
| `@7n/mt-linux-x64` | linux/x64/**без libc** | `x86_64-unknown-linux-musl` (static) | весь Linux x64 (Alpine, Ubuntu, Docker, CI) |

Обґрунтування «лише 2»: статичний musl-бінарник без поля `libc` покриває весь Linux x64 одним
пакетом (і glibc, і musl); решта платформ (Intel Mac, linux-arm64, Windows) додаються пізніше
**add-only** — новий підпакет + рядок в `optionalDependencies` + CI-job, без зміни коду.

Супутні рішення:
* **JS-сканер видалено**; `scanner.mjs` — тонкий шим: `spawnSync` бінарника → парсинг JSON →
  flatten дерева + мапінг полів (`dir=join(mtDir,path)`, snake_case→kebab стани, `is_composite`,
  `children`-шляхи). `topoSort`/`areDepsResolved`/`getActiveWorktrees`/`parseWorktreeList`
  лишились у JS (граф/git, не ФС-скан). `deriveNodeState`/`isComposite` з `state.mjs` видалено;
  лишились `sanitizeTaskName` (створення worktree) і `NODE_STATES`.
* **worktree→running перенесено в Rust**: `mt-scanner scan` сам виконує `git worktree list`
  (або приймає `--worktrees a,b,c` для тестів/прокидування) і підвищує стан. `sanitize`
  портовано в Rust (синхронність із JS — спільні тест-вектори).
* **Порядок пріоритетів станів у Rust вирівняно з авторитетним JS-тестом**: pending-audit >
  resolved > unresolvable > running > plan-review > spawned > waiting/failed > pending >
  unassigned. Виправлено: `unresolvable` більше не передує fact-станам; `pending-audit`
  рахується лише для останнього fact NNN.
* **Резолвер** `npm/lib/core/scanner-bin.mjs`: `MT_SCANNER_BIN` → `require.resolve(@7n/mt-<key>/<bin>)`
  → dev-fallback `target/release|debug` → зрозуміла помилка. Ім'я з `.exe` на win32 закладено
  наперед (Windows = add-only).
* **CI tooling — `cargo-zigbuild`**: один Linux-раннер крос-збирає musl-таргети; дешеве додавання
  linux-arm64 потім. macOS arm64 — native `macos-14`.
* **Покриття**: деривація станів тепер під `cargo test` (раніше нуль Rust-тестів) — додано
  відтворення кейсів зі `state.test.mjs` + worktree + sanitize. JS-`state.test.mjs` зрізано до
  `NODE_STATES`/`sanitizeTaskName`.

### Consequences

* Good: одна канонічна реалізація сканування (Rust), без дивергенції; весь ФС-доступ у Rust;
  доставка по платформах як у зрілих CLI; масштабування платформ add-only.
* Good: `cargo test` зелений (30 тестів), `vitest run` — 46/47 (єдиний фейл `docs.test.mjs` про
  кількість ADR — прееснуючий, поза цією зміною).
* Bad: `spawnSync` бінарника + внутрішній `git worktree list` на кожен скан (для CLI прийнятно;
  для частого `watch` — потенційно кеш/довгоживучий процес, поза scope).
* Bad: `sanitize`-конвенція дублюється (JS + Rust) — мусить лишатися синхронною.
* Bad: на непокритій платформі CLI впаде без `MT_SCANNER_BIN` (резолвер дає зрозумілу підказку).

## More Information

- Специфікація рефакторингу: `docs/spec-scanner-rust-integration.md`
- Попередні ADR сесії: `20260613-071723-заміна-js-сканера-на-виклик-rust-бінарника-mt-scanner.md`,
  `20260611-193434-вирівнювання-scanner-state-з-специфікацією-mt.md`
- Бінарник: `cargo build --release --manifest-path scanner/Cargo.toml`; CLI:
  `mt-scanner scan <tasks_dir> [--worktrees a,b,c]`
- Відкладено за рішенням користувача: інтеграція підкоманди `mt-scanner workspaces` у JS.

## Update 2026-06-13

### Вибір `cargo-zigbuild` для збірки Linux musl у CI

Для збірки `x86_64-unknown-linux-musl` обрано `cargo-zigbuild` (zig як лінкер) замість `musl-tools + rustup target add`. Причина (з transcript): дозволяє крос-збирати `aarch64-unknown-linux-musl` з того самого `ubuntu`-раннера без нового тулчейну — додавання `linux-arm64` = зміна лише `--target` у матриці. CI-job: `ubuntu-latest`, `cargo install cargo-zigbuild`, `cargo zigbuild --release --target x86_64-unknown-linux-musl`. Обмеження: Windows-таргет через zigbuild не збирається незалежно від вибору — потребує окремого `windows-latest` раннера.

## Update 2026-06-13

`.gitignore` містить записи `packages/*/mt-scanner` і `packages/*/mt-scanner.exe` — бінарники платформних підпакетів виключено з git. Резолвер `npm/lib/core/scanner-bin.mjs` додає суфікс `.exe` на `win32` як forward-compat для майбутнього Windows-підпакету. Повний порядок резолвингу: `MT_SCANNER_BIN` env → `require.resolve('@7n/mt-<key>/mt-scanner[.exe]')` → dev-fallback `target/release/mt-scanner` → зрозуміла помилка (не мовчки). CI-тригер у `npm-publish.yml` розширено: `scanner/**` і `packages/**` поряд із `npm/**`.

## Update 2026-06-13

Перша публікація нових scope-пакетів (`@7n/mt-darwin-arm64`, `@7n/mt-linux-x64`) блокується помилкою `ENEEDAUTH` — потребує `npm login` або налаштування trusted-publishing (OIDC) для нових пакетів у `@7n` scope. Процедура ручної першої публікації: скопіювати macOS-бінарник локально (`cp target/release/mt-scanner packages/mt-darwin-arm64/`), завантажити Linux-артефакт з CI (`gh run download <run-id> --name mt-linux-x64 --dir packages/mt-linux-x64/`), потім `npm publish` з кожної директорії підпакету, після — `npm publish` головного `npm/`.
