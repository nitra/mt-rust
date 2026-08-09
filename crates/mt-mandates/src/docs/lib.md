---
type: Rust Module
title: lib.rs
resource: crates/mt-mandates/src/lib.rs
docgen:
  crc: b7f6967f
---

## Огляд

Кореневий модуль crate `mt-mandates` — реалізація нормативного контракту «M6 фаза 0» (mandates.md): карта мандатів `.mt/mandates.yaml`, маршрутизатор ескалацій, квіз-гейт. Реекспортує чотири napi-API функції контракту (`parse_mandates`, `effective_owner`, `validate_mandate_change`, `validate_approval`) і всі супутні типи з підмодулів `types`, `parse`, `lookup`, `change`, `approval`.

## Поведінка

Модуль лише декларує підмодулі та реекспортує їхній публічний API — власної логіки не містить.

## Публічний API

parse_mandates, parse_mandates_str, MandatesError — розбір і валідація `.mt/mandates.yaml`.
effective_owner, EffectiveOwner, LookupError — маршрутизатор ескалацій, крок 1.
validate_mandate_change, MandateChangePayload, MandateChangeSignature, MandateSigner, SignerRole, sign_mandate_change, verify_mandate_change_signature — підписаний diff мандатів.
validate_approval, DecisionApproval, ApprovalContext, Quiz, QuizParseError, parse_quiz, depth_for_facets, SignatureError — квіз-гейт.
MandatesFile, Mandate, MandateKind, Scope, Thresholds, RiskLevel, AudacityLevel, BlastRadius, Divergence, LeverageFacets, DecisionFacets, QuizDepth, Verdict — спільні типи.

## Гарантії поведінки

- Read-only: не виконує операцій запису (ФС/БД).
- Чистий реекспорт — без побічної логіки.
