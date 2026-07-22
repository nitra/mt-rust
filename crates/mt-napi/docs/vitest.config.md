---
type: JS Module
title: vitest.config.mjs
resource: crates/mt-napi/vitest.config.mjs
docgen:
  crc: d32a8e2d
  model: claude-fable-5
  tier: manual
  score: 100
---

## Огляд

Конфігурація vitest для workspace `@7n/mt-napi` (build-обгортка napi-аддона): визначає, які тести підхоплюються, і гарантує процесну ізоляцію між test-файлами.

## Поведінка

- `include` — дві розкладки тестів: поряд із кодом (`**/*.test.{js,mjs}`, конвенція `test`-правила — піддиректорії `tests/`) і top-level integration suites у `<root>/tests/`.
- `exclude` — крім стандартних `node_modules`/`dist`, виключає `reports/stryker/**`: там лежать sandbox-копії тестів від Stryker (incremental або aborted-runs), які поза реальним repo root фейляться.
- `environment: 'node'` — без DOM.
- `pool: 'forks'` — defense-in-depth ізоляція: у дефолтному `threads` усі workers ділять один процес, і паралельний `process.chdir(dir)` у тестовій фікстурі перехоплює cwd сусіда посеред FS/`git`-операції (реальний інцидент: `git init`+`git commit` із tmp-фікстури потрапив у робочий репозиторій). Канон тестів — `withTmpDir(async dir => ...)` (test.mdc).
- `coverage` — провайдер `v8`, репортери `lcov` + `text-summary`.
