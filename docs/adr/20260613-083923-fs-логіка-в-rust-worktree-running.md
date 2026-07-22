# ФС-логіка — в Rust; виявлення `worktree→running` переноситься в бінарник

**Status:** Accepted
**Date:** 2026-06-13

## Context and Problem Statement

Після рішення викликати `mt-scanner` через JS-шим постало питання розподілу відповідальності: де виконувати логіку визначення стану `running` для задачі, що виконується в активному git-worktree. JS-реалізація визначала цей стан двома шляхами: `running_*`-sentinel файл на диску та наявність активного git-worktree через JS `execSync`. Перехід на шим вимагав явного рішення — залишити цю логіку у JS чи перенести в Rust.

## Considered Options

* JS post-process у шимі: Rust повертає дерево без `running`-стану; JS отримує список worktree через `getActiveWorktrees` і підвищує стан поверх JSON
* Перенести виявлення активних worktree в Rust: `mt-scanner scan` сам виконує `git worktree list --porcelain` і повертає `running`-стан у JSON

## Decision Outcome

Chosen option: "Перенести виявлення worktree в Rust", because користувач сформулював принцип: «все що стосується роботи з файловою системою повинно бути Rust»; виклик `git worktree list` і читання ФС для матчингу є файловою операцією — JS не повинен цього торкатися.

### Consequences

* Good, because JS-шим стає суто тонким адаптером (запуск бінарника + JSON-парсинг + flatten + мапінг станів); жодних FS-операцій у JS.
* Good, because transcript фіксує очікувану користь: єдина точка відповідальності за стан задачі; принцип «єдина точка ФС-логіки» витримано.
* Bad, because `sanitizeTaskName` (конвенція іменування worktree) потрібно продублювати в Rust для матчингу — JS-копія лишається лише для `mt run` (створення worktree); синхронізація через спільні тест-вектори є необхідною умовою коректності.

## More Information

- `scanner/src/lib.rs` — `discover_worktrees(tasks_root)`: викликає `git worktree list --porcelain`, парсить `worktree <path>` рядки, повертає `Vec<String>` імен (останній компонент шляху).
- `scanner/src/lib.rs` — `sanitize_task_name(s)`: портована логіка з JS `state.mjs`; використовується для матчингу worktree-імені проти task-path при визначенні стану `running`.
- `scanner/src/main.rs` — опція `--worktrees <list>` дозволяє детерміновані тести без реального `git`; у продакшені пропускається — бінарник сам виявляє worktrees.
- `npm/lib/core/state.mjs` — `sanitizeTaskName` збережено для `mt run` (створення worktree); `deriveNodeState`/`isComposite` видаляються.
- 30 Rust unit-тестів (`#[cfg(test)]` у `lib.rs`) відтворюють кейси з `state.test.mjs`, включно з `worktree→running` і `sanitize`-векторами; `cargo test` зелений.
- Специфікація: `docs/spec-scanner-rust-integration.md §5`.
