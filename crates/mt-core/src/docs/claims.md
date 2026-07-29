---
type: Rust Module
title: claims.rs
resource: crates/mt-core/src/claims.rs
docgen:
  crc: e001c819
  model: omlx/gemma-4-e2b-it-4bit
  score: 95
---

## Огляд

Файл надає інструменти для роботи з remote execution claims, забезпечуючи механізми для визначення та керування правами володіння вузлами через git refs.

## Поведінка

node_hash: генерує 20-символьний хеш SHA-256 з поєднання `tasks_root` та `node_path`.
discover_repo_root: знаходить кореневий каталог репозиторію через native `gix` discovery.
tasks_root_relative: обчислює канонічний шлях `tasks_dir` відносно `repo_root`, нормалізуючи POSIX-роздільники.
RemoteClaimRef: структура для зберігання хешу вузла та SHA.
parse_ls_remote: парсить вивід `git ls-remote` для вилучення `RemoteClaimRef`.
ClaimInfo: структура для зберігання розпарсених даних з `.mt-claim.yml` включаючи стан прострочення.
lease_expired: перевіряє, чи прострочений термін дії ліцензії з урахуванням простроки (grace period).
parse_claim: будує `ClaimInfo` з YAML-вмісту `.mt-claim.yml`.
ClaimFields: структура для зберігання полів, які контролює runner, включаючи бейз-хеш та посилання на першу коміт.
ClaimPush: структура для збереження результату CAS-push, включаючи статус прийняття.
acquire_claim: створює claim-коміт через native `gix`, а exact CAS-публікацію делегує вузькому `git::compat`.
renew_or_takeover_claim: створює новий claim-коміт на основі попереднього, використовуючи `old_claim_sha` для авторизації.
release_claim: намагається видалити (delete) claim-референс, якщо він належить поточному власнику.
fetch_remote_claims: зчитує remote claims через native `gix` fetch та парсить YAML для генерації `ClaimInfo`.

## Публічний API

**node_hash** — 20-символьний хеш SHA-256 з `<tasks-root>\0<node-path>`.
**discover_repo_root** — кореневий каталог репозиторію через native `gix` discovery.
**RemoteClaimRef**, **ClaimInfo**, **ClaimFields**, **ClaimPush** — структури для представлення claim-даних.
**parse_ls_remote**, **parse_claim** — парсинг `git ls-remote` і `.mt-claim.yml` у `ClaimInfo`.
**lease_expired** — прострочення lease з урахуванням grace period.
**acquire_claim**, **renew_or_takeover_claim**, **release_claim** — CAS-цикл claim-коміту: створення, поновлення/перехоплення за `old_claim_sha`, видалення за `claim_sha`.
**fetch_remote_claims** — читає всі remote claims native `gix` fetch-операцією та парсить кожен claim-коміт.

## Гарантії поведінки

- Перехоплює помилки і не пропускає винятків назовні (fail-safe).
- За певних помилок повертає порожнє значення (напр. `null`) замість винятку.
