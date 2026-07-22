# Видалення перевірок version/CHANGELOG з package_structure.mjs

**Status:** Accepted
**Date:** 2026-06-02

## Context and Problem Statement
`package_structure.mjs` (npm/rules/npm-module) містив перевірки, що вимагали відповідності `version` у `package.json` та наявності свіжого запису у `CHANGELOG`. Ці перевірки конфліктували з правилом `n-changelog.mdc`, яке забороняє ручний bump — єдиний дозволений артефакт зміни є change-файл (`npx @nitra/cursor change …`). Результат: `npx @nitra/cursor fix changelog npm-module` давав ❌ навіть за коректно складеної гілки.

## Considered Options
- Видалити перевірки `version`/`CHANGELOG` з `package_structure.mjs`, лишивши `changelog/consistency.mjs` єдиним валідатором узгодженості.
- Інші варіанти в transcript не обговорювалися.

## Decision Outcome
Chosen option: "Видалити суперечливі перевірки з `package_structure.mjs`", because єдиний легальний артефакт змін — change-файл; узгодженість version/CHANGELOG вже валідує `changelog/consistency.mjs`, тому дублювання лише примушувало до ручного bump, що `n-npm-module.mdc` явно забороняє.

### Consequences
- Good, because `npx @nitra/cursor fix changelog npm-module` проходить без ❌ по version/CHANGELOG; ручний bump більше не вимагається.
- Bad, because transcript не містить підтверджених негативних наслідків.

## More Information
Змінені файли: `npm/rules/npm-module/js/package_structure.mjs`, `npm/tests/integration-repo-checks.test.mjs`. Коміт `fe08579`. Change-файл — `npm/.changes/` (bump: patch, section: Fixed). Перша спроба помістила change-файл у кореневий `.changes/` через відсутній `--ws npm` — виправлено повторним `mt done --ws npm`. У цій же сесії розв'язано конфлікт merge гілки `feat/coverage-changed-gate` у `main` (коміт `c091708`): для `reviewer.mjs`/`flow.mdc` обрано бік feat (`DEFAULT_GATES = [lint, coverage --changed]`), для `rust/coverage.mjs` — бік HEAD (новіший, містить `diffPath`/`baseline:skip`). Валідація: `node --check` по 3 файлах, `grep -rl '<<<'` — маркерів нема, 330/330 тестів зелені.
