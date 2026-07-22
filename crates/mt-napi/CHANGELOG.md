# Changelog

## [0.3.1] - 2026-07-21

### Changed

- chore(deps): n-taze bump — 6 minor/patch (npm), 0 major (Rust unchanged)

## [0.3.0] - 2026-07-15

### Added

- Биндінг `killNode` — `mt kill` тепер іде через `mt-core lifecycle::kill` (одна імплементація контракту: вузол без run-історії видаляється, з історією — архів у `.history/`)

## [0.2.1] - 2026-07-14

### Changed

- feat(mt): Rust-порт run-оркестрації до паритету; run.mjs — тонкий клієнт mt-core (#47)

## [0.2.0] - 2026-07-14

### Removed

- Модельні ключі (model_map / claude_model / audit_model) видалені з дефолтів .mt.json — конфігурація виконавців іде з user-level ENV (ADR 260713-2110)

## [0.1.5] - 2026-07-11

### Changed

- ⬆️ chore: @nitra/cursor ^14.25.1 (авто-оновлення pre-commit hook)
- 📝 chore: mt-napi changelog entry

## [0.1.4] - 2026-07-11

### Changed

- ⬆️ chore: @nitra/cursor ^14.25.1 (авто-оновлення pre-commit hook)
- 📝 chore: mt-napi changelog entry

## [0.1.3] - 2026-07-08

### Changed

- 📝 docs: файлові доки для 47 кодових файлів (doc-files беклог) (#16)

## [0.1.2] - 2026-07-08

### Changed

- release: @7n/mt@0.8.0

## [0.1.1] - 2026-07-07

### Changed

- release: @7n/mt@0.8.0
- chore: package.json — type module + engines (js.mdc канон); vitest/stryker/jsconfig конфіги (T0-автофікси) + файлові доки

## [0.1.0] - 2026-07-04

### Added

- Новий napi v3 addon: биндинги mt-core для Node/Bun (darwin-arm64, linux-x64)
