---
type: Rust Module
title: change.rs
resource: crates/mt-mandates/src/change.rs
docgen:
  crc: fdc91170
---

## Огляд

Модуль реалізує `validate_mandate_change` — вердикт на підписаний diff `.mt/mandates.yaml` (mandates.md, «Нормативний контракт (M6 фаза 0)», включно з follow-up-правкою «зміна `escalates_to` вимагає ПОДВІЙНОГО підпису», реалізованою одразу). Crypto повторює патерн `agent_protocol::approvals`/`transfers`: доменний префікс + NUL-роздільник полів, `ed25519_dalek::verify_strict`, спільний `agent_protocol::ApprovalError` (без дублювання типу помилки). Перевіряє: `generation` зростає рівно на 1; розширення scope/thresholds підписане делегатором рівня вище (звуження — самопідпис); розширення ШІ-мандата (`kind: model`, включно з підняттям `audacity`) підписує лише людський ключ («остання константа», модельний підпис відхиляється безумовно); зміна `escalates_to` вимагає підпису і нового адресата, і поточного делегатора.

## Поведінка

`MandateChangePayload` — canonical-акт (старе/нове покоління + хеш SHA-256 нового вмісту), підписаний Ed25519 (`sign_mandate_change`/`verify_mandate_change_signature`). `validate_mandate_change` спершу перевіряє generation fencing і структурну валідність нового файлу (`parse::validate`), потім класифікує зміну кожного owner-мандата (`Added`/`Removed`/`KindChanged`/`EscalatesToChanged`/`Widened`/`Narrowed`) і вимагає відповідний підпис із заздалегідь криптографічно перевіреного списку `signatures`.

## Публічний API

validate_mandate_change — `(old, new, signatures) → Verdict`.
MandateChangePayload — canonical-акт для підпису/перевірки.
sign_mandate_change, verify_mandate_change_signature — Ed25519 підпис/перевірка акта.
MandateSigner, SignerRole, MandateChangeSignature — підписант (handle + роль ключа) і його підпис.

## Гарантії поведінки

- Read-only: не виконує операцій запису (ФС/БД).
- Fail-closed: підпис, що не пройшов криптографічну перевірку, не рахується жодним із required підписів.
