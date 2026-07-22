---
type: ADR
title: Аудит-черга через pending-audit_NNN.md
description: Запит на аудит зберігається як numbered immutable файл, який mt watch підхоплює асинхронно.
---

**Status:** Accepted
**Date:** 2026-06-07

## Context and Problem Statement

Після того як агент завершує роботу над вузлом DAG, потрібен механізм аудиту якості за критеріями з `task.md`. Старий підхід запускав аудитора синхронно через wrapper або команду, але нова архітектура `npm/docs/mt.md` базується на файловому стані `tasks/<node>/` і черзі, яку сканує `mt watch`. Потрібно визначити, як позначати запит на аудит і як пов'язувати його з конкретною версією `outputs_NNN.md`.

## Considered Options

- Синхронний запуск аудитора wrapper-скриптом.
- Асинхронна черга через файл `pending-audit_NNN.md`.
- Порожній sentinel `.pending-audit` без прив'язки до версії.
- Overwrite-файл `.pending-audit` з `ref:` полем.

## Decision Outcome

Chosen option: "Асинхронна черга через `pending-audit_NNN.md`", because transcript фіксує принцип «стан = файли», а NNN в імені `pending-audit_NNN.md` однозначно посилається на відповідний `outputs_NNN.md` без окремого `ref:` поля.

### Consequences

- Good, because `mt watch` отримує єдину точку відповідальності за dispatch аудиту без синхронного блокування wrapper-процесу.
- Good, because `pending-audit_003.md` однозначно відповідає `outputs_003.md`; нумерація не губиться між output, запитом аудиту і обробкою аудитором.
- Good, because запит на аудит стає immutable файловим фактом поруч з `run_NNN.md` і `outputs_NNN.md`.
- Bad, because transcript не містить підтверджених негативних наслідків.

## More Information

Факти з transcript:

- файл запиту: `tasks/<node>/pending-audit_NNN.md`;
- NNN у `pending-audit_NNN.md` дорівнює NNN відповідного `outputs_NNN.md`;
- при повторній доробці агент пише новий `outputs_002.md` і створює `pending-audit_002.md`;
- `mt watch` сканує вузли зі станом `pending-audit` і запускає auditor-агента;
- auditor пише `run_NNN.md` з `actor: auditor` і результатом `success|failed`;
- `run_NNN.md` має незалежний лічильник для всіх акторів, а `outputs_NNN.md` і `pending-audit_NNN.md` мають спільний ключ NNN;
- стан `pending-audit` додається до таблиці станів вузла поруч з `waiting`, `running`, `resolved`, `failed`, `invalidated`.

## Update 2026-06-07

Додано уточнення з паралельного драфта:

- `pending-audit_NNN.md` обрано як numbered immutable варіант замість порожнього sentinel або overwrite-файлу з `ref:`.
- `pending-audit_003.md` є посиланням на `outputs_003.md` самим ім'ям файлу.
- Auditor-агент обробляє запит асинхронно і пише окремий `run_NNN.md` з `actor: auditor, result: success|failed`.
- Таблиця станів вузла включає `pending-audit` поруч із `waiting`, `running`, `resolved`, `failed`, `invalidated`.

Цей драфт також повторює рішення про дворівневий `flow`, `mt plan`, явний `mt spawn` і видалення Фасаду B; вони вже покриті існуючим ADR про переформатування `flow` під архітектуру mt.
