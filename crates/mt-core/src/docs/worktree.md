---
type: Rust Module
title: worktree.rs
resource: crates/mt-core/src/worktree.rs
docgen:
  crc: 0e45c74f
  model: omlx/gemma-4-e2b-it-4bit
  score: 100
---

## Огляд

Огляд
Файл керує операціями з іменуванням, пошуком, створення, прив'язки та видалення detached worktree для виконання завдань, використовуючи git-операції та різноманітні хеші.

Поведінка
make_worktree_name Створює ім'я worktree з назви та епохи у форматі `<sanitized-path>-<epoch-sec>`.

find_worktree_match Знаходить перший запис з entries, що починається з префікса або дорівнює префіксу.

create_run_worktree Створює detached worktree від base_sha у `worktrees_dir/<node-hash>-<token>` за допомогою git worktree add --detach.

push_run_ref Публікує локальний run ref у `refs/mt/<node-hash>/<token>` як поточний HEAD worktree.

delete_run_ref Видаляє remote run ref, використовуючи --force-with-lease для безпечного видалення.

remove_run_worktree Видаляє worktree після завершення спроби, використовуючи git worktree remove --force.

## Поведінка

Поведінка

make_worktree_name Створює ім'я worktree з назви та епохи в форматі `<sanitized-path>-<epoch-sec>`.

find_worktree_match Знаходить перший запис з entries, що починається з префікса або дорівнює префіксу.

create_run_worktree Створює detached worktree від base_sha у `worktrees_dir/<node-hash>-<token>` за допомогою git worktree add --detach.

push_run_ref Публікує локальний run ref у `refs/mt/<node_hash>/<token>` як поточний HEAD worktree.

delete_run_ref Видаляє remote run ref, використовуючи --force-with-lease для безпечного видалення.

remove_run_worktree Видаляє worktree після завершення спроби, використовуючи git worktree remove --force.

## Публічний API

**make_worktree_name** — генерує ім'я для worktree: `<sanitized-path>-<epoch-сек>`.
**find_worktree_match** — знаходить перший запис з `entries`, що починається з `<prefix>-`.
**create_run_worktree** — створює detached worktree від `base_sha` у ``worktrees_dir/<node-hash>-<token>`` (спека: `git worktree add --detach .worktrees/<node-hash>-<token> <base_sha>`). Worktree ізольований від живого робочого дерева.
**push_run_ref** — публікує локальний run ref для recovery/handoff (спека, крок 5: `refs/mt/runs/<node-hash>/<token>` ← поточний HEAD worktree).
**delete_run_ref** — видаляє remote run ref (після успішного publish або при cleanup невдалої спроби; `--force-with-lease` — лише якщо ref усе ще на очікуваному SHA).
**remove_run_worktree** — прибирає worktree після завершення спроби (success — завжди; failure — залишається для debug за рішенням викликача, спека «Failure-сімейство»).

## Гарантії поведінки

- Перехоплює помилки і не пропускає винятків назовні (fail-safe).
- За певних помилок повертає порожнє значення (напр. `null`) замість винятку.
