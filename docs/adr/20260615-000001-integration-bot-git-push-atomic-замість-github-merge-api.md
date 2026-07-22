# Integration Bot: git push --atomic замість GitHub Merge API

**Status:** Accepted
**Date:** 2026-06-15

## Context and Problem Statement

MT Integration Bot виконував merge через GitHub Merge API з подальшим видаленням claim як окремі нетомарні кроки. Це створювало TOCTOU-гонку: між перевіркою claim, викликом Merge API і видаленням claim стан міг змінитися іншим учасником. Потрібно було усунути цю гонку й привести протокол бота у відповідність до прямого publish-протоколу.

## Considered Options

* GitHub Merge API + окреме видалення claim (попередній підхід)
* `git push --atomic --force-with-lease` по трьох рефах: `main`, `refs/mt/claims/<hash>`, `refs/mt/runs/<hash>/<token>`

## Decision Outcome

Chosen option: "`git push --atomic --force-with-lease` по трьох рефах", because атомарний push виключає TOCTOU-гонку — всі три рефи оновлюються в одній транзакції або не оновлюються взагалі; протокол бота стає ідентичним прямому publish-протоколу. PR перетворюється виключно на approval-інтерфейс.

### Consequences

* Good, because TOCTOU-гонка між check → merge → delete усунута: операція атомарна на рівні Git.
* Good, because уніфікація з прямим publish-протоколом — одна кодова гілка для обох сценаріїв.
* Bad, because бот-ідентичність потребує дозволу bypass branch protection для запису в `main` напряму без PR-merge.

## More Information

- `npm/docs/mt.md` — змінено протокол Integration Bot: видалено виклик GitHub Merge API, додано `git push --atomic --force-with-lease` на `main`, `refs/mt/claims/<hash>`, `refs/mt/runs/<hash>/<token>`.
- Три рефи: `main` — цільова гілка; `refs/mt/claims/<hash>` — claim видаляється; `refs/mt/runs/<hash>/<token>` — run-запис публікується.
- Дозвіл `bypass branch protection` — необхідна умова для bot-ідентичності.
