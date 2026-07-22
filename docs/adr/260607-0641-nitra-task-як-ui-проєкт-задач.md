---
type: ADR
title: nitra/task як окремий UI-проєкт для task-графу
description: UI для візуалізації та керування task-графом розміщується в окремому проєкті `nitra/task`.
---

**Status:** Accepted
**Date:** 2026-06-07

## Context and Problem Statement

Потрібен окремий веб-проєкт для візуалізації стану task-графу з `npm/docs/mt.md` та керування доступом розробників до task-node середовищ. У transcript обговорювалися назва і розташування такого проєкту.

## Considered Options

- `n-graph`
- `graphwatch`
- `taskflow`
- `nitra/task`

## Decision Outcome

Chosen option: "nitra/task", because користувач явно визначив назву і розташування проєкту як `/Users/vitaliytv/www/nitra/task`.

### Consequences

- Good, because назва вписується у namespace `nitra/*` і не привʼязує UI до конкретної реалізації CLI.
- Bad, because transcript не містить підтвердження негативних наслідків цього вибору.
- Neutral, because transcript фіксує, що проєкт уже існує з `app`, `package.json`, `bun.lock`, `bunfig.toml` та `eslint.config.js`.

## More Information

Transcript facts:

- Шлях проєкту: `/Users/vitaliytv/www/nitra/task`.
- Перший task-node у проєкті повʼязаний із доступом через editor/Teleport.
- Альтернативи `n-graph`, `graphwatch` і `taskflow` були відхилені на користь явно названого користувачем `nitra/task`.
