## ADR Bot переходить з GitHub Merge API на fenced atomic push

## Context and Problem Statement
Bot для protected main використовував GitHub Merge API для публікації результатів, що створювало TOCTOU-гонку: перевірка claim і merge не є атомарними, тому між кроками claim міг змінитись.

## Considered Options
* Залишити GitHub Merge API (поточний підхід)
* Bot виконує той самий `git push --atomic` з `--force-with-lease` що й direct publisher

## Decision Outcome
Chosen option: "Bot виконує `git push --atomic` з `--force-with-lease`", because race condition між перевіркою claim і GitHub Merge API усувається через той самий fenced push що й direct publish — перевірка і запис відбуваються в одній атомарній операції.

### Consequences
* Good, because TOCTOU race між check і merge повністю усунуто — `git push --atomic` є атомарним по трьом refs (main, claim, run).
* Bad, because bot потребує bypass-дозволу в branch protection rules щоб пушити напряму в protected main.

## More Information
PR залишається виключно як approval interface (review + CI). Після approval bot формує commit і виконує `git push --atomic --force-with-lease` на refs: `refs/heads/main`, `refs/mt/claims/<hash>`, `refs/mt/runs/<hash>/<token>`. Файл: `npm/docs/mt.md`.
