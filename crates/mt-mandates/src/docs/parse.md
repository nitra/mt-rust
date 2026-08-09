---
type: Rust Module
title: parse.rs
resource: crates/mt-mandates/src/parse.rs
docgen:
  crc: c587e17c
---

## Огляд

Модуль реалізує `parse_mandates`/`parse_mandates_str` — розбір `.mt/mandates.yaml` через `serde_norway` і повну структурну валідацію за нормативним контрактом «M6 фаза 0» (mandates.md), яку serde сам по собі виразити не може: рівно один кореневий мандат (`escalates_to: null`), досяжність кореня з кожного owner скінченним ланцюгом `escalates_to` без циклів і висячих handle, непорожній `scope.refs`/`scope.decision_types`, `audacity` лише для `kind: model`, `generation ≥ 1`. `owner` трактується як унікальний ключ запису — інакше ланцюг ескалації неможливо однозначно пройти.

## Поведінка

`parse_mandates` читає файл за шляхом і делегує `parse_mandates_str`, яка розбирає YAML і викликає `validate`. `validate` — `pub(crate)`, бо `change::validate_mandate_change` перевіряє нею структурну коректність нового стану файлу окремо від криптографічних/підписних правил. `validate_mandate_shape` перевіряє один запис: непорожній scope, `audacity` лише для моделей.

## Публічний API

parse_mandates — читає файл за шляхом, повертає `MandatesFile` або `MandatesError`.
parse_mandates_str — той самий розбір з рядка (для тестів і викликачів, що вже тримають вміст).
MandatesError — `Io` | `Yaml` | `Validation(String)`.

## Гарантії поведінки

- Read-only: не виконує операцій запису (ФС/БД), лише читання шляху в `parse_mandates`.
- Fail-closed: будь-яке порушення контракту — явна `Validation`-помилка, не мовчазне прийняття.
