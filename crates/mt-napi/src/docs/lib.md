---
type: Rust Module
title: lib.rs
resource: crates/mt-napi/src/lib.rs
docgen:
  crc: b07b821c
  model: omlx/gemma-4-e2b-it-4bit
  score: 95
---

## Огляд

Файл є біндінгом до `mt-core` для використання з `@7n/mt`. Він забезпечує конвертацію типів між JavaScript та Rust та мапінг помилок у `napi::Error`. Уся доменна логіка знаходиться в `mt-core`, а JS-обгортки знаходяться в `npm/lib/core/native.mjs`.

## Поведінка

scan_tasks Сканує директорію tasks і повертає дерево вузлів.
create_task Створює вузол задачі з ім'ям та опціями.
find_workspaces Виявляє workspace-и з заданих директорій.
discover_worktrees Виявляє workspace-и з початкової директорії.
pad_nnn Форматує число у формат NNN-рядок.
next_run_nnn Розраховує наступне NNN для run_файлів.
next_plan_nnn Розраховує наступне NNN для plan_файлів.
latest_fact_nnn Повертає найвищий NNN з fact-файлів.
latest_pending_audit_nnn Повертає найвищий NNN з pending-audit-файлів.
latest_audit_result_nnn Повертає найвищий NNN з audit-result-файлів.
latest_build_markdown Повертає згенерований markdown-файл.
parse_front_matter Парсить YAML front-matter з markdown-тексту.
get_body Отримує тіло документа без frontmatter.
serialize_yaml Серіалізує об'єкт у формат YAML.
build_markdown Будує markdown-файл із frontmatter та тілом.
sanitize_task_name Санітизує ім'я задачі для worktree.
validate_task_name Валідує ім'я задачі відповідно до специфікації.
sanitize_branch Нормалізує ім'я гілки до безпечного імені директорії.
config_defaults Повертає дефолтну конфігурацію.
merge_config Зливає сирий текст `.mt.json` з дефолтними значеннями.
effective_config Створює ефективну конфігурацію з різних джерел.
make_worktree_name Генерує ім'я worktree з шляху та epoch.
find_worktree_match Знаходить відповідність worktree з даним шляхом.

## Публічний API

Я готовий. Надайте мені код, який потрібно переписати у вигляді лаконічної поведінкової документації, дотримуючись усіх ваших інструкцій.

## Гарантії поведінки

- Read-only: не виконує операцій запису (ФС/БД).
- Перехоплює помилки і не пропускає винятків назовні (fail-safe).
- За певних помилок повертає порожнє значення (напр. `null`) замість винятку.
