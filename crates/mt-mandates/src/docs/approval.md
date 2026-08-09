---
type: Rust Module
title: approval.rs
resource: crates/mt-mandates/src/approval.rs
docgen:
  crc: b3e0b1a3
---

## Огляд

Модуль реалізує `validate_approval` — квіз-гейт на `decision-request` (mandates.md, «Формат квіз-файлів», «Розширення `ApprovalResponse`»). Frontmatter квіз-файлу (`NNNN-quiz.md`) розбирається через `mt_core::frontmatter` (той самий парсер, що всі інші mt-файли); підпис `ApprovalResponse` перевіряється через `agent_protocol::{ApprovalPayload, verify_approval}` — базовий підписаний кортеж лишається незмінним, `chosen_option`/`quiz_ref` — супровідні непідписані поля того самого JSON-об'єкта.

## Поведінка

`parse_quiz` розбирає frontmatter (`schema_version`, `decision_ref`, `depth`, `generated_by`, `iterations`, `time_to_understanding_sec`). `Quiz::is_complete` перевіряє зафіксованість проходження. `validate_approval` перевіряє послідовно: Ed25519-підпис `ApprovalResponse`, наявність `quiz_ref`, завершеність квізу, відповідність `quiz.decision_ref` вузлу, `quiz.generated_by ≠ recommended_by` (конфлікт інтересів), відповідність `depth` мапінгу `leverage_facets` (`depth_for_facets`).

## Публічний API

parse_quiz — розбір frontmatter квіз-файлу.
Quiz, QuizParseError — розібраний квіз і помилка розбору.
DecisionApproval, ApprovalContext — `ApprovalResponse` decision-request-гейту і контекст перевірки.
validate_approval — `(response, quiz, pubkey, ctx) → Verdict`.
depth_for_facets — детермінований мапінг `leverage_facets → depth`.

## Гарантії поведінки

- Read-only: не виконує операцій запису (ФС/БД).
- Fail-closed: відсутній/незавершений квіз, невідповідний `decision_ref`/`depth`, конфлікт інтересів чи невалідний підпис — явний `Verdict::Invalid`.
