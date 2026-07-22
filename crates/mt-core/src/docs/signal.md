---
type: Rust Module
title: signal.rs
resource: crates/mt-core/src/signal.rs
docgen:
  crc: dc1e4917
  model: omlx/gemma-4-e2b-it-4bit
  score: 100
---

## Огляд

Файл відповідає за обгортку виконавця для управління станом завершення роботи, включаючи запис фактів, результатів виконання та керування аудитами. Забезпечує послідовне проходження етапів виконання вузла та агрегацію результатів вгору.

CheckResult Результат однієї команди
SignalOutcome Результат сигналу done/audit записані файли та пропагація вгору
next_run_nnn Обчислює NNN наступної спроби count + 1
check_commands Витягує команди секції ## Check з task.md
run_check Проганяє ## Check з task.md повертає `Vec<CheckResult>`
write_fact Пише fact_NNN.md з обов'язковим ## Summary
write_run Пише run_NNN.md
write_run_fm Пише run_NNN.md з додатковими frontmatter-рядками
done Записує run_NNN (success) та створює необхідні компоненти
audit Записує run_NNN (success) та відкриває аудит-цикл pending-audit_NNN.md
failed Пише run_NNN (failed) без fact
propagate_composite Виконує composite-агрегацію вгору якщо всі діти resolved

## Поведінка

Поведінка

CheckResult Результат однієї команди ## Check
SignalOutcome Результат сигналу done/audit записані файли та пропагація вгору
next_run_nnn Обчислює NNN наступної спроби count + 1
check_commands Витягує команди секції ## Check з task.md
run_check Проганяє ## Check з task.md повертає `Vec<CheckResult>`
write_fact Пише fact_NNN.md з обов'язковим ## Summary
write_run Пише run_NNN.md
write_run_fm Пише run_NNN.md з додатковими frontmatter-рядками
done Записує run_NNN (success) та створює необхідні компоненти
audit Записує run_NNN (success) та відкриває аудит-цикл pending-audit_NNN.md
failed Пише run_NNN (failed) без fact
propagate_composite Виконує composite-агрегацію вгору якщо всі діти resolved

## Публічний API

Як технічний письменник, я готовий переписати ваш список відповідно до ваших вимог.

---

CheckResult — результат однієї команди `## Check`.
SignalOutcome — результат сигналу done/audit: записані файли + пропагація вгору.
next_run_nnn — NNN наступної спроби: `count + 1` (спека, «NNN source»).
check_commands — витягує команди секції `## Check` task.md: кожен непорожній рядок — shell-команда, `#` — коментар.
run_check — проганяє `## Check` (cwd = project root — батько tasks_dir). Будь-який ненульовий exit → `Err` з виводом команд; сигнал відхиляється.
write_fact — пише `fact_NNN.md` (NNN наступної спроби) з обов'язковим `## Summary`.
write_run — формулює стисло з наміру файлу.
write_run_fm — як `write_run`, але з додатковими frontmatter-рядками (wall_sec тощо).
done — `mt done`: fact існує → `## Check` → `run_NNN (success)` → агрегація вгору.
audit — `mt audit`: як done, але відкриває аудит-цикл (`pending-audit_NNN.md`).
failed — `mt failed`: `run_NNN (failed)` без fact; секції Completed/Blockers/Next Attempt обов'язкові (інваріант файлу — джерело діагностики ретраїв).

## Гарантії поведінки

- Перехоплює помилки і не пропускає винятків назовні (fail-safe).
- За певних помилок повертає порожнє значення (напр. `null`) замість винятку.
