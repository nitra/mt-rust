---
type: Rust Module
title: graph.rs
resource: crates/agent-server/src/graph.rs
docgen:
  crc: 85b7be20
  model: openai-codex/gpt-5.4-mini
  score: 100
  issues: judge:inaccurate:0.99
  judgeModel: openai-codex/gpt-5.4-mini
---

## Огляд

Міст для інтерактивного run вузла за контрактом `runtime.md`: «Інтерактивна сесія = run вузла». Бере CAS claim, створює detached worktree, прив’язує run ref і фіксує хід сесії в `session.jsonl` через виклики `mt-core`; реалізація не реімплементує графовий контракт, а спирається на ту саму основу, яку використовує `@7n/mt` через napi. Життєвий цикл: `attach` → `commit_turn` → `renew` → `done`/`release`, де `done` означає fenced publish, `release` — пауза, а `.nitra/` живе лише в run ref і очищається перед publish, щоб ніколи не потрапити в `main` згідно з `git.md`. API працює fail-safe: перехоплює помилки, для частини з них повертає порожнє значення замість винятку.

## Поведінка

- GraphConfig — зберігає конфігурацію для інтерактивного run: tasks-директорію, lease і актора.
- new — створює дефолтну конфігурацію для інтерактивного запуску з коротким lease.
- InteractiveRun — представляє живу інтерактивну сесію вузла з прив’язаним claim, worktree і run ref.
- attach — займає вузол через CAS claim, створює detached worktree від базового стану та публікує run ref.
- commit_turn — фіксує хід сесії через журнал `.nitra/session.jsonl`, а порожній хід лишає без змін.
- renew — поновлює lease поточного claim і повертає ознаку, чи сесію ще утримує цей runner.
- done — готує публікацію: прибирає `.nitra/` з дерева змін, виконує fenced publish і за успіху очищає worktree.
- release — знімає claim, прибирає worktree і лишає run ref як основу для відновлення.

## Публічний API

- GraphConfig — конфіг моста між worktree, claim і run ref.
- new — створює новий claim і прив’язаний стартовий стан run.
- InteractiveRun — тримає claim активним під час живого інтерактивного run і матеріалізує worktree.
- attach — підхоплює claim, створює detached worktree від `base_sha` і прив’язує run ref; якщо claim уже забраний, одразу падає з claim-lost.
- commit_turn — фіксує один хід: додає журнал сесії й зміни файлів, потім пушить run ref; якщо змін немає, нічого не робить.
- renew — подовжує lease для того ж token/generation від поточного SHA claim; якщо claim уже втрачено, зупиняє сесію.
- done — завершує `mt done`: прибирає `.nitra/` з індексу, публікує fenced release через rebase на `origin/main` і атомарний push, далі очищає worktree.
- release — ставить сесію на паузу: видаляє claim і прибирає worktree, залишаючи run ref для наступного attach.

## Гарантії поведінки

- Перехоплює помилки і не пропускає винятків назовні (fail-safe).
- За певних помилок повертає порожнє значення (напр. `null`) замість винятку.
