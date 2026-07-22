---
type: ADR
title: Один файл task.md для місії та inputs
description: Місія вузла і вхідні дані зберігаються в одному task.md, а зміна inputs після старту виконується через patch-протокол.
---

**Status:** Accepted
**Date:** 2026-06-06

## Context and Problem Statement

Агенту при старті потрібні і формулювання задачі, і вхідні дані або посилання на них. У transcript обговорювалось, чи зберігати місію та inputs у двох файлах (`task.md` і `inputs.md`) або об'єднати їх в одному файлі. Також потрібно було визначити, що робити, якщо EngineerAgent хоче змінити inputs після старту вузла.

## Considered Options

* Два файли: `task.md` для місії та `inputs.md` для вхідних даних.
* Один файл `task.md` із секцією `## Inputs`.

## Decision Outcome

Chosen option: "Один файл `task.md` із секцією `## Inputs`", because transcript фіксує згоду: `task.md` містить і місію, і вхідні дані; якщо інженер хоче змінити inputs, це виконується як patch.

### Consequences

* Good, because агент читає один файл замість двох, а місія та inputs завжди перебувають в одному контексті.
* Good, because spawn створює менше артефактів: `inputs.md` як окремий файл не використовується.
* Neutral, because зміна inputs після старту не є прямим редагуванням окремого файлу, а проходить через `patches/patch-plan-<ts>.md` і `patches/patch-fact-<ts>.md`.
* Bad, because transcript не містить підтверджених негативних наслідків.

## More Information

Файл специфікації: `npm/docs/mt.md`. Фінальна схема `task.md`: YAML frontmatter з `created_at`, опційними `parent` і `deps`, далі обов'язкові секції `## Task`, `## Done when`, `## Inputs`. У `## Inputs` підсекції мають довільні назви; значення можуть бути `ref: tasks/.../outputs.md#section` або inline-текстом. Формат `<ts>` для operation/patch-файлів: `YYYYMMDD-HHMMSS`. Пов'язаний CLI-контракт у transcript: `mt init`, `mt start`, `mt spawn`, `mt done`, `mt fail`, `mt kill`, `mt repair`, `mt status`.

## Update 2026-06-06

Драфт уточнює контракт об'єднаного `task.md` і пов'язаних форматів:

- `task.md` містить обов'язкові англійські секції `## Task`, `## Done when`, `## Inputs`.
- `## Inputs` має підсекції з довільними назвами; значення можуть бути `ref:` або inline-текстом.
- Зміна inputs після старту трактується як patch, а не як окреме редагування `inputs.md`.
- Формат файлів лишається Markdown + YAML frontmatter: frontmatter для машинозчитуваних полів, тіло Markdown для LLM-контексту.
- Додатково драфт фіксує, що секції, які парсить скрипт/оркестратор, мають англійські заголовки; довільні секції можуть бути будь-якою мовою.
