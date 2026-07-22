## ADR Integration bot виконує fenced git push замість GitHub Merge API

**Status:** Accepted
**Date:** 2026-06-13

## Context and Problem Statement

Protected-main fallback використовував GitHub Merge API: bot перевіряв claim, викликав GitHub merge, потім CAS-видаляв claim. Між перевіркою claim (крок 1) та GitHub merge (крок 2) виникав TOCTOU race: claim міг бути renewed або takeover-нутий. У разі renewal — bot не міг CAS-видалити claim (SHA вже інший), залишаючи "висячий" claim. У разі takeover — PR старого runner потрапляв у `main` під ownership нового runner.

## Considered Options

* Залишити GitHub Merge API з додатковою синхронізацією між перевіркою і merge
* Bot виконує той самий `git push --atomic` з трьома `--force-with-lease` що й direct publisher; PR залишається approval interface

## Decision Outcome

Chosen option: "bot виконує fenced git push", because `git push --atomic` з `--force-with-lease` на `main`, claim ref і run ref усуває TOCTOU повністю: перевірка claim і запис у `main` відбуваються в одній атомарній операції. Якщо claim змінився між approval і push — операція відхиляється цілком.

### Consequences

* Good, because TOCTOU race усунено; protected-main шлях отримує ті самі атомарні гарантії що й direct publish; GitHub Merge API як залежність прибрано; PR залишається для review/CI/human sign-off без участі в механізмі commit.
* Bad, because transcript не містить підтверджених негативних наслідків.

## More Information

Змінено: `npm/docs/mt.md` — секція про protected main fallback (~рядки 1165–1174), рядок про "integration branch + PR + bot", рядок 874, таблиця "Lifecycle у main". Bot потребує "bypass branch protection" permission в GitHub branch protection rules. Аналогія: GitHub Merge Queue або Dependabot-style direct push.
