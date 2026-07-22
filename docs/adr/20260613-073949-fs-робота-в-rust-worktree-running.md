# Виявлення активних worktree та весь FS-доступ переноситься в Rust

**Status:** Accepted
**Date:** 2026-06-13

## Context and Problem Statement

Після рішення викликати Rust-бінарник через JS-шим постало питання: де виконувати логіку визначення стану `running` — виявлення активного git-worktree для задачі. Оригінальний JS-код робив це post-process кроком у `scanTasks`, викликаючи `getActiveWorktrees` (JS `execSync git worktree list`). Потрібно було вирішити: FS-логіка залишається в JS-шимі як post-process чи переноситься в Rust-бінарник.

## Considered Options

* JS post-process у шимі: `mt-scanner scan` повертає JSON без `running`-стану; JS отримує список worktree через `getActiveWorktrees` і підвищує стан самостійно
* Перенести виявлення активних worktree в Rust: `mt-scanner scan` сам виконує `git worktree list --porcelain`, застосовує `sanitize_task_name` для матчингу і повертає `running`-стан у JSON

## Decision Outcome

Chosen option: "Перенести виявлення worktree в Rust", because користувач встановив директиву: «усе, що стосується роботи з файловою системою, повинно бути Rust»; виклик `git worktree list` і читання FS для матчингу — файлова операція, тому JS не повинен цього торкатися.

### Consequences

* Good, because JS-шим стає суто тонким адаптером без FS-операцій — лише `spawnSync`, JSON-парсинг і адаптація контракту.
* Good, because єдина точка відповідальності за стан задачі; принцип «єдина точка FS-логіки» витримано.
* Bad, because `sanitizeTaskName` (конвенція іменування worktree) тепер існує в двох місцях: `scanner/src/lib.rs` (для матчингу при скануванні) і `npm/lib/core/state.mjs` (для створення worktree в `mt run`); без спільних тест-векторів розходження логіки дасть false negative у `running`-детекції.

## More Information

- `scanner/src/lib.rs` — `discover_worktrees(tasks_root)`: виконує `git worktree list --porcelain`, парсить рядки `worktree <path>`, повертає `Vec<String>` (назви — останній компонент шляху).
- `scanner/src/lib.rs` — `sanitize_task_name(s)`: портована логіка з `npm/lib/core/state.mjs`; використовується для матчингу worktree-імені проти task-path при визначенні `running`.
- `scanner/src/main.rs` — прапор `--worktrees w1,w2,...`: дозволяє детерміновані тести без реального `git`; у продакшені пропускається, Rust виконує discovery самостійно.
- `npm/lib/core/state.mjs` — `sanitizeTaskName` збережено; `deriveNodeState`/`isComposite` видалено.
- `getActiveWorktrees`/`parseWorktreeList` залишаються в JS — потрібні `mt run` для *створення* worktree (поза скануванням).
- 30 Rust unit-тестів (`#[cfg(test)]` у `lib.rs`) покривають усі кейси `state.test.mjs`, включно з `worktree→running` і `sanitize`-векторами; `cargo test` зелений.
- Специфікація: `docs/spec-scanner-rust-integration.md §5`.

## Update 2026-06-13

`sanitizeTaskName` в `npm/lib/core/state.mjs` використовується не лише `mt run` (створення worktree), а й `npm/lib/commands/worktree.mjs` — обидва не пов'язані зі скануванням і залишаються в JS. Уточнення щодо прапора `--worktrees`: приймає список через кому і використовується виключно в `#[cfg(test)]`-тестах для детермінізму без реального git.
