# Branch protection — обов'язкова передумова деплойменту MT

**Status:** Accepted
**Date:** 2026-06-13

## Context and Problem Statement

Fencing через Git CAS гарантується лише для compliant MT runners. Будь-який актор з прямим доступом до репозиторію може push-нути в `main` і обійти механізм. Документ `npm/docs/mt.md` формулював branch protection як умовну рекомендацію ("щоб fencing було security boundary"), а не hard prerequisite.

## Considered Options

* Branch protection як best-practice з поясненням наслідків відсутності
* Branch protection як mandatory prerequisite; `mt setup` fail closed без неї

## Decision Outcome

Chosen option: "mandatory prerequisite з fail-closed у `mt setup`", because без branch protection fencing не є security boundary за визначенням — один non-compliant writer руйнує гарантію для всіх runners. Fail-closed підхід робить порушення явним замість мовчазної деградації.

### Consequences

* Good, because fencing стає реальним security boundary, а не рекомендацією; документ явно вказує що MT не надає гарантій без branch protection.
* Bad, because вимагає налаштування "bypass required pull requests" для MT runner і integration bot identities — додаткові операційні вимоги при першому розгортанні.

## More Information

Файл `npm/docs/mt.md`, секція Bootstrap (крок 0), рядок ~1275. GitHub: Settings → Branches → "Allow specified actors to bypass required pull requests". MT runner identity та integration bot identity мають бути явно додані до bypass list.
