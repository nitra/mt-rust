---
type: JS Module
title: stryker.config.mjs
resource: crates/mt-napi/stryker.config.mjs
docgen:
  crc: 2c7c9c37
  model: claude-fable-5
  tier: manual
  score: 100
---

## Огляд

Конфігурація Stryker (mutation testing) для workspace `@7n/mt-napi`: vitest-runner з per-test coverage і інкрементальним кешем результатів.

## Поведінка

- `testRunner: 'vitest'` з `configFile: 'vitest.config.mjs'` — мутанти ізолюються в пам'яті через AST-patching, без копіювання `node_modules` у sandbox (стара проблема command runner у Bun monorepo; тому `inPlace` не потрібен).
- `coverageAnalysis: 'perTest'` — на кожен мутант запускаються лише тести, що покривають мутовану лінію; головний приріст швидкості проти command runner з повним suite.
- `tempDirName: 'reports/stryker/.tmp'` — sandbox-и під `reports/`, щоб не засмічувати корінь (vitest їх виключає).
- Репортери `json` (`reports/stryker/mutation.json`) + `clear-text`.
- `incremental: true` з `incrementalFile: 'reports/stryker/incremental.json'` — зберігає результати між запусками і відновлюється після краш/kill; ~262× прискорення на noop-прогонах (див. benchmarks/runner-comparison/SPIKE.md).
- Concurrency не задано — Stryker бере `os.cpus().length - 1`.
