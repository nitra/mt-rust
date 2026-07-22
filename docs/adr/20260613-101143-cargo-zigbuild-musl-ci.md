# `cargo-zigbuild` як CI-інструмент для musl-статичних Linux-збірок

**Status:** Accepted
**Date:** 2026-06-13

## Context and Problem Statement

`npm-publish.yml` потребував способу зібрати статичний musl-бінарник `mt-scanner` для `x86_64-unknown-linux-musl` на GitHub Actions. Вибір підходу впливав на вартість додавання `linux-arm64` у майбутньому. Rust-код вже є path-портабельним (рядок `lib.rs:273` нормалізує бекслеші; `lines()` коректно обробляє CRLF), тому Windows-підтримка вимагатиме лише `.exe`-суфіксу у резолвері й окремого CI-job без змін логіки.

## Considered Options

* `cargo-zigbuild` — один Linux-раннер крос-компілює будь-який musl-таргет через `zig cc`; arm64 пізніше додається як `--target aarch64-unknown-linux-musl` без нових тулчейнів
* `musl-tools` + `rustup target add x86_64-unknown-linux-musl` — стандартний apt-підхід; arm64 потребує окремого aarch64-тулчейна або окремого раннера

## Decision Outcome

Chosen option: "`cargo-zigbuild`", because zigbuild дає дешеве додавання `linux-arm64` пізніше без нових тулчейнів; Windows є ортогональним і потребує нативного `windows-latest` незалежно від вибору tooling.

### Consequences

* Good, because майбутній `linux-arm64` = `+1` пакет + `+1` CI-job без зміни тулчейна; матриця підтвердила роботу в CI (`ubuntu-latest`, job `build-binaries`, ~2m10s).
* Good, because Windows-підтримка закладена наперед у резолвері: `binName(platform)` у `npm/lib/core/scanner-bin.mjs` повертає `'mt-scanner.exe'` для `win32`; додавання `@7n/mt-win32-x64` і CI-job `windows-latest` є add-only.
* Bad, because `cargo-zigbuild` додає залежність від `zig` і є менш зрілим інструментом; для локального тестування musl-збірок необхідно `brew install zig` та `cargo install --locked cargo-zigbuild`.

## More Information

Файл: `.github/workflows/npm-publish.yml`, job `build-binaries`, matrix `os: ubuntu-latest`, команда `cargo zigbuild --release --target x86_64-unknown-linux-musl`. Локальна верифікація: бінарник типу `ELF x86-64 statically linked`; `npm pack --dry-run` у `packages/mt-linux-x64/` підтвердив коректний вміст. Spec §6.5: «Додавання Windows пізніше = add-only»; `lib.rs:273` нормалізує бекслеші, що підтверджує наявну Windows-портабельність.
