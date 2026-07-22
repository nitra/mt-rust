## ADR Single Publish Owner як гарантія замість Mutual Exclusion

**Status:** Accepted
**Date:** 2026-06-13

## Context and Problem Statement

`npm/docs/mt.md` використовував термін "mutual exclusion" для claim-гарантій fencing-протоколу. Після рев'ю виявлено: fencing через Git refs зупиняє лише push у `main`, але не зупиняє виконання процесу. Zombie-runner після lease takeover може продовжувати роботу і генерувати зовнішні side effects: повторна оплата, повторний API-запит, зміна database, deployment, відправлення повідомлення.

## Considered Options

* Залишити термін "mutual exclusion" без змін
* Перейменувати гарантію на "single publish owner" і додати документацію щодо side effects та вимог до задач з non-idempotent операціями

## Decision Outcome

Chosen option: "single publish owner", because термін точно описує реальну гарантію протоколу: лише один runner може публікувати Git-результат у `main` у даний момент. Mutual exclusion виконання не забезпечується fencing-механізмом, тому вживання цього терміну вводило в оману авторів задач.

### Consequences

* Good, because документ точно описує межі гарантії; автори задач розуміють що потрібен idempotency key або передача fencing `generation` у зовнішню систему для non-idempotent side effects; задачі без idempotent side effects явно виключаються з auto-takeover.
* Bad, because transcript не містить підтверджених негативних наслідків.

## More Information

Змінено: `npm/docs/mt.md` — секція Fencing (доданий абзац "Межа fencing — лише Git publish"), рядок "Protocol гарантує..." (mutual exclusion → single Git publisher), таблиця summary (рядок перейменовано з "Mutual exclusion" на "Single publish owner").

## Update 2026-06-13

### Heartbeat loop для виявлення zombie-runner

Для довготривалих задач рекомендовано periodic heartbeat loop: runner re-reads і верифікує claim ref кожні N секунд. Реакція на `claim-lost`: негайно скасувати задачу і зупинити зовнішні side effects. Абзац додано між кроком renewal і секцією Межа fencing у Direct publish protocol.

### Зовнішній fencing key: {node-hash, generation}

Рекомендується передавати комбінований ключ `{node-hash, generation}` або `claim_id = node-hash + token` у зовнішні системи замість лише `generation`. `generation` є монотонним лічильником лише в межах одного вузла: два різних вузли можуть мати однаковий `generation`. `token` — uuid4 унікальний per-claim; `node-hash` унікально ідентифікує вузол — комбінація є глобально унікальним ключем. Оновлено рядок 190 у `npm/docs/mt.md`.

### Scope ізоляції claim: node-level, не runner-level

Claim isolation діє на рівні вузла, а не runner-рівні. Multi-tasking runner (тримає кілька claim-ів одночасно) не порушує протокол; кожен вузол ізольований незалежно. Для runner-level обмежень потрібен окремий registry поза MT. Оновлено рядок таблиці summary: `Claim isolation` → `Claim isolation (node-level)`.
