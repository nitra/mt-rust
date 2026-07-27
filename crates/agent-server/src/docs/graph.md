---
type: Rust Module
title: graph.rs
resource: crates/agent-server/src/graph.rs
docgen:
  crc: b05874f3
  model: openai-codex/gpt-5.4-mini
  tier: cloud-min
  score: 100
  issues: judge-refine:kept-original,judge:inaccurate:0.99
  judgeModel: openai-codex/gpt-5.4-mini
---

## Огляд

Міст до графа для інтерактивного run вузла за контрактом `runtime.md` і `git.md`: сесія робить attach через CAS claim, працює в detached worktree, веде `session.jsonl` у run ref, підтримує handoff і завершується через fenced publish або переходить у release. Це не перевизначає граф: усі дії делегуються `mt-core` — тій самій реалізації, яку `@7n/mt` використовує через napi.

Публічний обсяг цього модуля — `GraphConfig`, `new`, `InteractiveRun`, `HandoffTicket`, `attach`, `attach_resume`, `generation`, `add_approval`, `commit_turn`, `renew`, `done`, `release`, `handoff`.

Виклики працюють fail-safe: помилки перехоплюються, назовні винятки не кидаються, а для окремих збоїв повертається порожнє значення замість exception.

`.nitra/` існує лише в run ref під час сесії, не потрапляє в `main` і прибирається перед publish.

## Поведінка

GraphConfig задає спільні параметри життєвого циклу вузла: шлях до tasks-дерева, тривалість lease і початкового актора. new створює цей контрактний стан із дефолтним lease та actor для подальших операцій.

InteractiveRun описує живу сесію, у якій claim уже утримується, а worktree матеріалізовано. Усі наступні дії працюють у межах цього стану й або продовжують той самий run ref, або переводять його в нову фазу.

HandoffTicket переносить мінімум, потрібний для кооперативного переходу між хостами: старий run ref і покоління claim-а, від якого треба продовжити. attach_resume використовує цей тікет, щоб відновити worktree не з main, а зі стану попереднього run ref, зберігши журнал і проміжні зміни.

attach запускає сесію з CAS-claimом, створює detached worktree від базового стану й прив’язує його до run ref. Якщо вузол уже зайнято, відмова є явною, без переходу в напівживий стан.

generation повертає fencing token поточного claim-а, який використовується як межа для side effects і для перевірки, що сесія ще належить цьому хосту.

add_approval доповнює сесію вже верифікованим підтвердженням, яке потім потрапляє в синтезовані артефакти run-а.

commit_turn зберігає хід у журнал сесії та фіксує зміни в run ref, щоб відновлення, handoff і подальший publish бачили повну послідовність дій. Порожній хід не створює зайвих артефактів.

renew продовжує lease для того самого claim-а, зберігаючи ownership лише поки CAS ще не втрачено. Якщо claim уже перехоплено, сесію треба зупинити.

handoff переводить живу сесію в кооперативний режим передачі: зберігає повний журнал у run ref, знімає claim і повертає тікет для attach_resume на новому хості. Це зберігає безперервність серії й не викидає проміжний стан.

release завершує активне утримання вузла без publish: claim знімається, worktree прибирається, а run ref лишається як база для відновлення або подальшого handoff.

done завершує сесію тільки після успішного Check-гейту. Перед publish він синтезує контрактні артефакти run і fact, прибирає `.nitra/` з індексу та виконує fenced publish у main; при невдачі сесія лишається живою.

`HandoffTicket` і `attach_resume` разом реалізують міграцію сесії між хостами без втрати журналу та мідфлайт-змін, а `done` завершує той самий ланцюг публікацією без розриву в нумерації серії.

## Публічний API

- GraphConfig — Конфігурація моста.
- InteractiveRun — Живий інтерактивний run: claim утримується, worktree матеріалізований.
- HandoffTicket — Тікет кооперативного handoff (runtime.md, «Міграція сесії між хостами»): ідентифікує старий run ref, з якого новий хост відновлює worktree, і generation, від якої продовжує лічильник claim-а (git.md: «новий хост: create, generation+1» — попри те, що механічно це create-only CAS, бо старий claim уже видалено). Serialize/Deserialize — тікет піде через relay `HandoffRequest`-відповідь у наступній задачі.
- attach — Attach вузла: CAS claim → detached worktree від `base_sha` → run ref. `accepted: false` CAS-у → явна помилка claim-lost (вузол уже зайнято).
- attach_resume — Відновлення на новому хості після кооперативного `handoff` (runtime.md, кроки 2-3): CAS-create claim (generation = `ticket` + 1) → worktree ЗІ СТАНУ старого run ref (не `origin/main`) — журнал і мідфлайт-правки успадковані → push нового run ref. Недоступний старий run ref (втрачено/типо у тікеті) → явна помилка, не паніка.
- generation — Поточна генерація claim-а (fencing token для side effects).
- add_approval — Додає верифікований approval-рядок (пише ws-обробник після успішної перевірки підпису гейтом).
- commit_turn — Коміт ходу: журнал сесії (`.nitra/session.jsonl`) + правки файлів → push run ref (recovery/handoff, спека git.md: «кожен хід = коміт + негайний push run ref»). Порожній хід (нічого не змінилось) — no-op.
- renew — Renewal lease: той самий token/generation, CAS від поточного claim SHA. `Ok(false)` — claim втрачено (takeover-ом), сесію слід зупинити.
- done — `mt done`: гейт `## Check` (контракт graph.md — fail → відмова сигналу, run лишається живим) → синтез `run_NNN.md`/`fact_NNN.md` → стрип `.nitra/` з індексу (інваріант git.md) → fenced publish (rebase на origin/main + atomic push main / видалення claim+run ref). Успіх → worktree прибирається.
- release — Пауза/відпустити: CAS-delete claim + прибрати worktree; run ref лишається (журнал сесії — база відновлення наступного attach).
- handoff — Кооперативний handoff (git.md, claim-операція `handoff`; runtime.md, «Міграція сесії між хостами», крок 2): синтезує `run_NNN.md (result: handoff)` → коміт → push run ref БЕЗ стрипу `.nitra/` — повний журнал розмови їде разом (checkpoint-режим із дистильованим summary — окрема задача) → CAS-delete claim. Повертає тікет для `attach_resume` на новому хості.
- new — створює новий інстанс модуля для роботи з NAPI-інтеграцією.

## Гарантії поведінки

- Перехоплює помилки і не пропускає винятків назовні (fail-safe).
- За певних помилок повертає порожнє значення (напр. `null`) замість винятку.
