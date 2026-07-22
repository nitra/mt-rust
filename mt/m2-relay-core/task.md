---
schema_version: 1
created_at: 2026-07-11T17:50:50Z
budget_sec: 10800
hint: atomic
---

## Task

Старт M2 (mission control): ядро relay — Bun-сервіс `relay/` (plain JS + JSDoc, БЕЗ TypeScript) за access.md/stack.md. Обовʼязки: auth акаунтів/пристроїв (інтерфейс `verifySession`, dev — magic tokens), membership задач + запрошення (invite → accept → MemberChanged), кімнати з пересилкою Envelope (не парсить payload далі роутінгових полів), буфер останніх ~200 Envelope на run, роздача pubkey-ів `approver+`. НЕ робить: журнали сесій, git-проксі, lease (істина — git claim), виконання агентів.

## Done when

- `relay/lib`: store-інтерфейс (accounts/devices/tasks/task_members/invitations за схемою access.md) з in-memory реалізацією (PostgreSQL — окрема задача за тим самим інтерфейсом);
- auth: `verifySession(token) → {account_id}` інтерфейс + dev-реалізація magic tokens; реєстрація пристрою `{name, role, pubkey} → device_token`;
- кімнати: підписка лише пристроям учасників кореневого вузла задачі; broadcast Envelope підписникам; буфер ≤200 Envelope на кімнату, реплей при підписці;
- ролі: viewer-клієнт НЕ шле клієнтські події (relay відхиляє, включно з CancelTurn); host+ шле; `GET pubkeys` — pubkey-и пристроїв учасників approver+ (доступ лише учасникам);
- membership API: invite (owner) → accept/decline → broadcast MemberChanged; transfer ownership;
- WS-сервер (пакет `ws` — працює під vitest/node і bun): hello з device_token → subscribe/envelope-кадри; ліміт кадру 2 МБ;
- vitest-тести: membership-роутінг кімнат, viewer не шле клієнтські події, invite→accept→MemberChanged, transfer ownership, буфер/реплей, відмова підписки не-учаснику.

## Check

npx vitest run relay

## Inputs

- Нормативні: npm/docs/architecture/access.md (обовʼязки/межі relay, схема даних, ролі, membership API), stack.md («Relay-інфраструктура»: Bun+Postgres, auth-інтерфейс, ліміти: кадр ≤2 MB, буфер ≤200 Envelope/run), runtime.md (транспорт (в) relay-клієнт).
- Поза скоупом: PostgreSQL-реалізація store, FCM push (інтерфейс-заглушка), Ory Kratos, deploy (Dockerfile/k8s), інтеграція agent-server як relay-клієнта, E2E-шифрування (свідомо не 0.3.0).
