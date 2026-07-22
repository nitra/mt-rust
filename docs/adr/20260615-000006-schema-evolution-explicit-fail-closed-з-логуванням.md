# Schema evolution: explicit fail-closed з логуванням

**Status:** Accepted
**Date:** 2026-06-15

## Context and Problem Statement

MT-схема версіонується. Потрібно було визначити поведінку системи при зустрічі документа з версією, вищою за підтриману (`version > max_known`), та при наявності невідомих полів. Тиха ігнорація несумісних версій могла б призводити до некоректної обробки даних без сигналу про проблему.

## Considered Options

* Silent ignore: невідома версія та невідомі поля — просто ігноруються
* Explicit fail-closed: `version > max_known` → FATAL; невідомі поля → WARN у `--verbose`

## Decision Outcome

Chosen option: "Explicit fail-closed з логуванням", because тиха ігнорація несумісної версії схеми є потенційно небезпечною — система може обробляти дані некоректно без будь-якого сигналу. Явний FATAL гарантує, що несумісна версія не буде оброблена мовчки.

### Consequences

* Good, because несумісна версія одразу видима: FATAL до stderr з path, actual version, max supported.
* Good, because чіткий інваріант: `--verbose` ЗАВЖДИ логує WARN для невідомих полів; ніколи не ігнорує мовчки при `--verbose`.
* Bad, because вузол переходить у стан `stalled` при зустрічі несумісної версії — потребує ручного втручання або оновлення інструменту.

## More Information

- `npm/docs/mt.md` ~line 145 — додано специфікацію schema evolution: FATAL для `version > max_known`, WARN для невідомих полів у `--verbose`.
- FATAL-повідомлення містить: path документа, actual version, max supported version — виводиться до stderr.
- Невідомі поля в normal mode: ignored; у `--verbose` mode: WARN (не допускається тиха ігнорація).
- Стан вузла при FATAL: `stalled`.
