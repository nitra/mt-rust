---
type: Rust Module
title: types.rs
resource: crates/mt-mandates/src/types.rs
docgen:
  crc: 4b166235
---

## Огляд

Модуль визначає serde-модель `.mt/mandates.yaml` за нормативним контрактом «M6 фаза 0» (mandates.md): корінь файлу (`MandatesFile`), запис делегування (`Mandate`), межі повноважень (`Scope`, `Thresholds`) і супутні перелічувані типи. Типи навмисно приймають ширшу множину значень, ніж дозволяє контракт (наприклад, `audacity` присутнє в `Thresholds` для будь-якого `kind`) — структурну валідність перевіряє окремо `parse::parse_mandates`, а не serde.

## Поведінка

`MandatesFile` тримає `generation` і масив `mandates`. `Mandate` — один запис делегування: owner, kind (person/model), scope, thresholds, адресат ескалації. `Scope::covers_all_decision_types` перевіряє wildcard `"*"`. `Thresholds::irreversible_or_default`/`audacity_or_default` застосовують дефолти контракту (`false`/`low`) до відсутніх полів. `LeverageFacets`/`QuizDepth`/`DecisionFacets` — допоміжні типи для маршрутизатора ескалацій і квіз-гейту. `Verdict` — спільний тип відповіді `validate_mandate_change`/`validate_approval`.

## Публічний API

MandatesFile — корінь `.mt/mandates.yaml` (generation + mandates[]).
Mandate — один запис делегування (owner, kind, scope, thresholds, escalates_to).
MandateKind — `person` | `model`.
Scope, Thresholds — межі мандата за refs/decision_types і за бюджетом/ризиком/незворотністю/зухвалістю.
RiskLevel, AudacityLevel, BlastRadius, Divergence — впорядковані перелічувані осі.
LeverageFacets, QuizDepth, DecisionFacets — вхід маршрутизатора ескалацій і квіз-гейту.
Verdict — `Valid` | `Invalid(reason)`.

## Гарантії поведінки

- Read-only: не виконує операцій запису (ФС/БД).
- Чиста serde-модель без бізнес-валідації (валідація — в `parse`/`change`/`approval`).
