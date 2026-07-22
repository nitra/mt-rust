## ADR mt invalidate зупиняє running процес внутрішньо; mt stop не є окремою CLI-командою

**Status:** Accepted
**Date:** 2026-06-13

## Context and Problem Statement

Patch protocol описував `mt kill` для зупинки successor-вузлів перед патчем цільового вузла. Але `mt kill` виконує `git rm -r` і знищує topology: після kill restart каскаду неможливий без повторної матеріалізації через `mt spawn --approve`. Reviewer запропонував окрему команду `mt stop` (зупинка процесу без знищення topology), яку patch protocol використовував би перед `mt invalidate`.

## Considered Options

* Окрема команда `mt stop` + `mt invalidate` в patch protocol
* `mt invalidate` сам виконує SIGTERM + CAS-delete claim для running-вузла перед архівацією; `mt stop` — не окрема CLI-команда

## Decision Outcome

Chosen option: "`mt invalidate` обробляє stop внутрішньо", because `mt stop` як standalone команда не має самостійного use case: пауза без подальшого `mt invalidate` залишає вузол у невизначеному стані (run без `result:`). Вбудування stop-логіки спрощує протокол і усуває race між `mt stop` і `mt invalidate` (retake claim у вікні між командами).

### Consequences

* Good, because patch protocol використовує одну команду замість двох; неможливий race між зупинкою і архівацією; `mt kill` явно зарезервований тільки для остаточного видалення topology.
* Bad, because transcript не містить підтверджених негативних наслідків.

## More Information

Змінено: `npm/docs/mt.md` — секція `mt invalidate` (додано: "Якщо вузол має активний claim: локальний runner → SIGTERM + CAS-delete claim; remote runner → CAS-delete claim"), engineer protocol (~рядок 1476: `mt kill <dep-node>` → `mt stop + mt invalidate`), engineer permissions (~1483). `mt stop` залишається як CLI-команда для explicit human use (звільнити claim без архівації), але з patch protocol прибрано.
