---
type: Rust Module
title: publish.rs
resource: crates/mt-core/src/publish.rs
docgen:
  crc: e621ff5a
  model: omlx/gemma-4-e2b-it-4bit
  score: 100
---

## Огляд

Огляд
Файл реалізує протокол закритого публікування (Fenced publish protocol), який забезпечує атомарне відправлення результату worktree у репозиторій `main` через рефетч/rebase та повторні спроби з експоненційним відкатом (exponential backoff+jitter).

Поведінка
PublishRequest: приймає запит на публікацію з посиланням на worktree та хеші.
PublishOutcome: повертає результат фінальної операції пушу, включаючи стан fencing та кількість спроб.
fenced_publish: виконує atomic push з ребазом worktree на origin/main та перевіркою fencing.

## Поведінка

Поведінка
PublishRequest: приймає запит на публікацію з посиланням на worktree та хеші.
PublishOutcome: повертає результат фінальної операції пушу, включаючи стан fencing та кількість спроб.
fenced_publish: виконує atomic push з ребазом worktree на origin/main та перевіркою fencing.

## Публічний API

Зрозумів. Я перепишу наданий список у потрібному стилі, як технічний письменник, що пише лаконічну поведінкову документацію українською мовою, без зайвих деталей, сигнатур чи типів.

Ось переписаний список:

- PublishRequest — приймає запит на публікацію.
- PublishOutcome — повертає результат публікації. `published: false` означає, що fencing/conflict (claim втрачено або конкурентний publish виграв гонку), а не системну помилку.
- fenced_publish — виконує три кроки: `fetch main + claim ref` → `rebase worktree на origin/main` → перевірка fencing (claim ref усе ще exact SHA) → `atomic multi-ref push` (main + видалення claim/run ref). Повторне виконання з експоненційним backoff+jitter при відхиленні push-у.

## Гарантії поведінки

- Перехоплює помилки і не пропускає винятків назовні (fail-safe).
- За певних помилок повертає порожнє значення (напр. `null`) замість винятку.
