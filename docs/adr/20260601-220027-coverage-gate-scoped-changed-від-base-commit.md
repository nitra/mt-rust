# Скоуп coverage-гейту турнікета через `--changed` від `base_commit`

**Status:** Accepted
**Date:** 2026-06-01

## Context and Problem Statement

Турнікет `mt verify` проганяє `DEFAULT_GATES = [lint, coverage]`, де `coverage` запускає vitest і Stryker по **всьому** проєкту (всі workspace-и, всі файли `src`), незалежно від того, які файли фактично змінено в задачі. Це призводить до надмірних прогонів Stryker — навіть після дрібних правок і навіть кілька разів за TDD-цикл. Додатково: `stryker.config.baseline.mjs` містить `incremental: true`, але `reports/stryker/` є в `.gitignore` (правило `n-test.mdc:221`), тому у свіжому worktree `incremental.json` завжди відсутній — перший прогін завжди повний (cold-start примусовий).

## Considered Options

- Повне видалення `coverage` з `DEFAULT_GATES` (лишається тільки на `release`/ручний виклик)
- `coverage --changed`: coverage-гейт лишається у турнікеті, але аналізує лише файли, змінені від `base_commit` задачі; передає scope у vitest (`--changed <base>`) та Stryker (`--mutate <список js-файлів>`)
- Конфіг-кероване увімкнення через `.n-cursor.json#flow.gates` (дефолт `['lint']`, opt-in `coverage`)
- Перенесення `coverage`-гейту на `release`-only (не в per-step `mt verify`)

## Decision Outcome

Chosen option: "`coverage --changed`", because користувач явно підтвердив: весь турнікет переходить на `--changed` (повний coverage лишається лише на `release`/ручний виклик), і при цьому coverage-гейт зберігається у `DEFAULT_GATES` — але завжди через `coverage --changed`.

### Consequences

- Good, because турнікет більше не ганяє весь Stryker і весь vitest-suite після кожної дрібної правки: scope обмежено `git diff <base_commit>` проти робочого дерева (uncommitted + committed рівноцінно).
- Good, because усувається примусовий cold-start Stryker у свіжому worktree (де `incremental.json` завжди відсутній через `.gitignore`).
- Bad, because порожній scope (наприклад, лише non-JS зміни) потрібно явно обробляти як `pass (0)`, а не поточний `exit 1` «Жодного провайдера»; без цієї обробки турнікет падатиме на правках документації.
- Bad, because coverage більше не форситься автоматично для всього проєкту на кожному `verify`; мутаційне покриття поза зміненими файлами перевіряється лише явно (`/n-coverage-fix`, `bun run coverage`) або на `release`.

## More Information

- `DEFAULT_GATES` визначено у `npm/scripts/dispatcher/lib/reviewer.mjs:14`
- Coverage-гейт оркеструється через `npm/rules/test/coverage/coverage.mjs` (orchestrator) → `npm/rules/js-lint/coverage/coverage.mjs` (js-lint provider)
- `base_commit` для `git diff` береться зі стану flow (MT file-presence state#metadata.base_commit`)
- `collectChangedFilesSince(base, cwd)` — новий helper у `npm/scripts/lib/changed-files.mjs` (поруч із наявним `collectChangedFiles`); об'єднує committed `git diff ${base}..HEAD`, uncommitted `git diff HEAD`, untracked `git ls-files --others`
- vitest 4.1.7 підтримує `--changed [since]`; Stryker 9 приймає `--mutate <файли>` (comma-separated)
- Fallback: якщо стан flow відсутній (ручний виклик поза flow), `coverage --changed` відступає до `collectChangedFiles` (working-tree від HEAD)
- `npm/scripts/dispatcher/lib/active.mjs:43` — `defaultVerify` → `runReview` (consumer без override)
- `npm/scripts/dispatcher/lib/commands.mjs:141` — `mt verify` (consumer)
- Тести `tests/reviewer.test.mjs` хардкодять `['lint','coverage']` — потребують оновлення при зміні `DEFAULT_GATES`

## Update 2026-06-01

Передісторія рішення: початковий аналіз розглядав повне видалення `coverage` з `DEFAULT_GATES`, перенесення на `release`-only та конфіг-кероване увімкнення через `.n-cursor.json#flow.gates` (дефолт `['lint']`, opt-in `coverage`). Аргумент проти видалення: `lint`-гейт вже задовольняє «лише змінені файли» (quick-режим + `changedFiles` з `changed-files.mjs`), тоді як `coverage` не має аналогічного режиму — вилучення без scoping лишало б coverage лише на `release`/ручний виклик без автоматичної гарантії.

Додатковий контекст cold-start: `stryker.config.baseline.mjs:16` містить `incremental: true`, але `reports/stryker/` є в `.gitignore` (n-test.mdc:221), тому у свіжому worktree `incremental.json` завжди відсутній незалежно від incremental-налаштування.

## Update 2026-06-01

Уточнення джерела scope: `--changed` базується на `git diff <base_commit>` (без `..`) — єдиний виклик, що покриває committed і uncommitted зміни від `base_commit` однаково, на відміну від `git diff HEAD` (не бачить закомічених змін від base у feature-гілці). `base_commit` читається з MT file-presence state (так само, як у `review.mjs:29`). Новий helper: `collectChangedFilesSince(base, cwd)` у `npm/scripts/lib/changed-files.mjs`; fallback на `collectChangedFiles` (HEAD-diff) при відсутньому стані flow для ручних викликів поза flow.

Окреме рішення: порожній `--changed`-scope → pass (exit 0) у `runCoverageSteps` при `rows.length === 0`. Root без змінених JS-файлів (документація, Rust, конфіги) — валідний стан, не помилка; `COVERAGE.md` не перезаписується. Тест: `coverage.test.mjs` (changed-scope, немає JS-файлів → exit 0, `COVERAGE.md` не створюється).

## Update 2026-06-02

Деталі реалізації (продовження сесії 37e16d83): `DEFAULT_GATES` у `reviewer.mjs` → `['npx','@nitra/cursor','coverage','--changed']`. Провайдери: `npm/rules/js-lint/coverage/coverage.mjs` (+111 рядків, `scopeToRoot`: vitest `--changed <base>`, Stryker `--mutate <changed-js>`; root без змінених JS — skip); `npm/rules/rust/coverage/coverage.mjs` (skip crate при відсутності змінених `.rs`); `npm/rules/test/coverage/coverage.mjs` (`--changed` резолвить base зі `MT file-presence state#metadata.base_commit`). 148 тестів зелені, lint exit 0. Зафіксований баг (`mt audit` L1): у `npm/rules/js-lint/coverage/coverage.mjs` (~рядок 335) exit code `runStryker` ігнорується (`await runner.runStryker(...)` без перевірки) — підриває контракт «змінений src без тестів має дати NoCoverage-мутанти й впасти»; на момент сесії не виправлено.

## Update 2026-06-02

Fail-closed поведінка `collectChangedFilesSince`: при недосяжному `base_commit` (відсутній у git-graph) — throw з повідомленням `недосяжний` замість мовчазного порожнього scope. Тести: `npm/scripts/lib/tests/changed-files.test.mjs` (committed changes видимі, uncommitted changes видимі, поза flow → fallback на HEAD-diff). `coverage --changed` як gate — exit-код без перезапису `COVERAGE.md`: часткові дані по підмножині файлів не замінюють повний звіт; `runCoverageSteps` при `opts.changed === true` і успішному прогоні повертає `0` без запису файлу (рядки 262–266). Тест: `'changed + провайдер з даними → exit 0, але COVERAGE.md НЕ перезаписується'` у `npm/rules/test/coverage/tests/coverage.test.mjs`.
