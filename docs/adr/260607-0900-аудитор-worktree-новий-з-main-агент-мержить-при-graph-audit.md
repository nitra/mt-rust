## ADR Аудитор отримує новий worktree з main — агент мержить при `mt audit`

## Context and Problem Statement
Після того як агент викликає `mt audit`, аудитор має отримати доступ до артефактів вузла (`outputs_NNN.md`, `pending-audit_NNN.md`). Потрібно було визначити: аудитор працює в існуючому worktree агента, чи у новому worktree з main-гілки?

## Considered Options
* Аудитор у тому ж worktree агента — агент не мержить при `mt audit`, worktree залишається для аудитора
* Аудитор у новому worktree з main — агент мержить при `mt audit`, видаляє worktree; watch диспатчить аудитора у свіжий worktree з main
* Аудитор без worktree — read-only доступ до main без checkout

## Decision Outcome
Chosen option: "Аудитор у новому worktree з main", because це усуває конфлікт між atomic mkdir-lock (worktree вже існує = EEXIST → skip) і необхідністю аудитора запуститися; агент мержить свої зміни перед виходом, файли потрапляють у main, аудитор стартує у чистому worktree де artifacts вже присутні.

### Consequences
* Good, because аудитор запускається через той самий механізм `mt run --actor auditor` з тим самим mkdir-lock що й звичайний вузол — жодного спецкейсу.
* Good, because після `mt audit` стан main актуальний: `outputs_NNN.md` і `pending-audit_NNN.md` доступні для інших вузлів і для git history.
* Bad, because якщо аудитор повертає `result: failed`, агент стартує новий worktree з main (де вже є `audit-result_NNN.md`), читає зауваження і починає заново — один більший цикл ніж у варіанті "той самий worktree".

## More Information
Потік:
```
agent writes outputs_NNN.md
agent calls: mt audit <path>
  → wrapper: creates pending-audit_NNN.md (NNN = NNN outputs)
  → wrapper: git merge + delete worktree   (файли тепер у main)

mt watch: pending-audit_NNN.md без audit-result_NNN.md → mt run --actor auditor
  → wrapper: git worktree add .worktrees/<node>-audit-<epoch> main
  → auditor reads: task.md + plan_NNN.md + outputs_NNN.md + pending-audit_NNN.md
  → auditor writes: audit-result_NNN.md (NNN = NNN pending-audit)
  → success → merge + delete audit worktree + touch .n-cursor/wake
  → failed  → merge audit-result → agent starts new worktree, reads audit-result_NNN.md
```
- Лічильник failed-циклів: wrapper рахує `audit-result_*.md (result: failed)` у main (файли на диску, без shared state між процесами)
- Після 3 failed → worktree залишається, `mt watch` ескалює через Telegram
- Зафіксовано у `npm/docs/mt.md` (секції «Async Audit Queue» і «Wrapper-скрипт»)

## Update 2026-06-06

- `audit: true` у frontmatter `task.md` вмикає перевірку вузла.
- Після `result: success` агента wrapper запускає аудитора без окремого worktree, у read-only режимі.
- Аудитор пише наступний `run_(NNN+1).md` з `actor: auditor` і `result: success | failed`.
- Якщо аудитор повертає `result: failed`, вузол не мержиться, а агент перезапускається з feedback.
- Конфіг може містити `audit_model` для дешевшої моделі аудитора.
- У тому ж transcript повторно зафіксовано вже прийняті рішення про `run_NNN.md`, англійські імена файлів, злиття `inputs.md` у `task.md`, межу immutability по worktree та post-merge hook; окремого нового ADR для них не потрібно.
