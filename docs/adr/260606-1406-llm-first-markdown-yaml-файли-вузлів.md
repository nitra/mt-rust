---
type: ADR
title: LLM-first Markdown з YAML-frontmatter для файлів вузлів
description: Файли вузлів графу зберігаються як Markdown з YAML-frontmatter, щоб бути одночасно зручними для LLM-агентів і скриптів.
---

**Status:** Accepted
**Date:** 2026-06-06

## Context and Problem Statement

Кожен вузол графу задач зберігає стан у файловій системі. Потрібно обрати формат файлів, який зручний для LLM-агентів, що читають і дописують контекст, і водночас придатний для машинного парсингу оркестратором.

## Considered Options

- JSON для всіх файлів стану.
- Markdown з YAML-frontmatter як LLM-first формат.

## Decision Outcome

Chosen option: "Markdown з YAML-frontmatter", because LLM-агент читає Markdown природно і може продовжувати текст без реконструкції JSON-структури, а YAML-frontmatter надає машинозчитувані поля для оркестратора.

### Consequences

- Good, because `repair_history.md` або інші журнальні файли можна читати й дописувати як природний Markdown-контекст.
- Good, because frontmatter відокремлює machine-readable metadata від довільного людського або LLM-контенту.
- Bad, because transcript не містить підтверджених негативних наслідків.
- Neutral, because секції, які парсить скрипт, мають бути стандартизовані англійськими заголовками.

## More Information

Файли вузла з transcript: `task.md`, `outputs.md`, `error.md`, `repair_context.md`, `repair_history.md`, `ops/*`, `patches/*`.

Правила контракту:

- `created_at` — перше поле frontmatter у всіх файлах.
- Імена файлів і директорій — англійською.
- Атрибути frontmatter — англійською у `snake_case`.
- Секції, які парсить скрипт або оркестратор, мають англійські заголовки.
- Секції з довільними даними можуть бути будь-якою мовою.
- Якщо дані вже існують у файлі, потрібно використовувати `ref:` замість копіювання.
