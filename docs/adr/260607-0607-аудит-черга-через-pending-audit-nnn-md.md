---
type: ADR
title: "Аудит-черга через `pending-audit_NNN.md`"
---

**Status:** Accepted
**Date:** 2026-06-07

## Context and Problem Statement
Потрібно визначити механізм запуску аудитора після того, як агент завершує роботу над вузлом DAG. Існуючий дизайн передбачав синхронний запуск аудитора безпосередньо через wrapper-скрипт, але система мала перейти до нового контракту на основі файлів (tasks/).

## Considered Options
* Синхронний запуск аудитора wrapper-скриптом (старий підхід — `mt audit` → auditor у тому ж worktree одразу)
* Асинхронна черга через файл `pending-audit_NNN.md` (новий підхід — `mt audit` записує файл, `mt watch` підхоплює)

## Decision Outcome
Chosen option: "Асинхронна черга через `pending-audit_NNN.md`", because аудит має бути обробленим чергою (як скан файлів), а не синхронно в wrapper-скрипті — це відповідає загальному принципу «стан = файли» і дає `mt watch` єдину точку відповідальності за dispatch.

### Consequences
* Good, because `mt watch` отримує єдину точку управління чергою аудиту та виконання вузлів — без синхронних блокувань у wrapper.
* Good, because NNN у `pending-audit_NNN.md` дорівнює NNN відповідного `outputs_NNN.md` — ім'я файлу саме по собі є посиланням, без потреби у явному полі `ref:`.

## More Information
Додаткової інформації не зафіксовано.
