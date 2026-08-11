---
type: Rust Module
title: graph.rs
resource: crates/agent-server/src/graph.rs
docgen:
  crc: 4d27361c
  model: openai-codex/gpt-5.4-mini
  tier: cloud-min
  score: 100
  issues: judge-refine:kept-original,judge:inaccurate:0.99
  judgeModel: openai-codex/gpt-5.4-mini
---

## Огляд

Містить інтерактивний run вузла як lifecycle від `CAS claim` і `run ref` з detached worktree до `session.jsonl`, `attach`/`attach_resume`, `done` або `release`, а також `handoff`. `done` завершує сесію fenced publish, `release` ставить її на паузу без публікації. Усі операції виконуються через `mt-core` через `napi`; графовий контракт не реімплементується, а використовується та сама реалізація, що й у `@7n/mt`. `GraphConfig`, `new`, `InteractiveRun`, `HandoffTicket`, `attach`, `attach_resume`, `generation`, `add_approval`, `commit_turn`, `renew`, `done`, `release`, `handoff`. `.nitra/` живе лише в run ref і прибирається перед publish, тому в `main` не потрапляє. Локальні fail-safe гілки можуть повертати порожнє значення замість винятку; інші помилки можуть поширюватися назовні.

## Поведінка

GraphConfig задає спільні межі життєвого циклу run: звідки беруться tasks і скільки живе claim до renewal або takeover. new створює початковий контекст вузла, з якого далі будуються attach, renew, done, release і handoff.

InteractiveRun тримає активний стан сесії: claim лишається захопленим, а worktree — матеріалізованим, доки вузол не завершить хід або не передасть естафету. Усі подальші дії працюють поверх цього стану і повертають результат або fail-safe помилку без ламання зовнішнього потоку.

attach запускає звичайний старт інтерактивної сесії: бере claim, створює detached worktree від базового стану й піднімає run ref як основу для журналу та наступних комітів. Якщо claim уже зайнято, потік зупиняється явною помилкою, щоб не змішувати два активні вузли в одному графі.

commit_turn є базовою одиницею прогресу: зміни з worktree та журнал сесії накопичуються в run ref, а порожні ходи не створюють зайвих артефактів. add_approval входить у цей самий потік як підтверджений сигнал, що потім потрапляє в синтезовані run-артефакти.

generation і renew разом утримують fencing token для side effects: generation відображає поточну версію claim, а renew продовжує її без зміни власника. Якщо claim уже втрачено, renew повертає ознаку провалу, щоб сесію можна було безпечно зупинити.

handoff переводить активну сесію на інший хост без втрати журналу: run ref зберігається як база відновлення, claim знімається, а результатом стає HandoffTicket. attach_resume споживає цей тікет, відновлює worktree зі стану старого run ref і продовжує generation з наступного значення; якщо старий run ref недоступний, це явна помилка, а не прихований збій.

done завершує хід через перевірку стану, синтез run- і fact-артефактів та fenced publish у main. Тут `.nitra/` має бути прибраний із публічного результату; якщо перевірка не проходить, вузол лишається живим і може бути виправлений перед повторною спробою.

release відпускає вузол без публікації: claim знімається, worktree прибирається, а run ref лишається як база для подальшого attach або відновлення.

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
- new — створює новий об’єкт для публікації, щоб зібрати дані допису перед відправкою

## Гарантії поведінки

- Містить локальні fail-safe гілки; інші помилки можуть поширюватися назовні.
- Деякі локальні fail-safe гілки повертають порожнє значення (напр. `null`) замість винятку.
