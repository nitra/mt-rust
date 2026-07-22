---
type: ADR
title: Teleport як SSH gateway для доступу розробників до task-нод у Kubernetes
description: Розробники підключаються до dev pods через Teleport без прямого kubectl-доступу до Kubernetes-кластера.
---

**Status:** Accepted
**Date:** 2026-06-07

## Context and Problem Statement

Система `mt` запускатиме task-вузли у Kubernetes. Розробникам потрібен доступ до dev pods, наприклад через Zed Remote SSH, для інспекції та патчингу конкретних task-нод. Водночас transcript фіксує вимогу: розробники не мають і не повинні мати прямого `kubectl`-доступу до кластера. Авторизацію на доступ до конкретного вузла має контролювати backend-застосунок, а не прямий k8s RBAC для розробників.

## Considered Options

- Teleport як identity-aware SSH gateway з label-based RBAC.
- `kubectl port-forward` для прямого SSH-доступу до pod.

## Decision Outcome

Chosen option: "Teleport як identity-aware SSH gateway з label-based RBAC", because `kubectl port-forward` вимагає наявного `kubectl`-доступу у розробника, що transcript явно відхиляє, а Teleport дозволяє контролювати SSH-доступ через identity, labels і short-lived certificates без видачі kubectl-прав.

### Consequences

- Good, because backend може створювати dev pod з labels на кшталт `owner: <email>`, а Teleport надає доступ лише відповідному користувачу.
- Good, because Teleport використовує short-lived SSH-сертифікати замість статичних ключів.
- Good, because Zed Remote може підключатися через стандартний SSH із `ProxyCommand tsh proxy ssh` без спеціальних патчів UI/IDE.
- Bad, because потрібно задеплоїти Teleport Auth Server і Proxy як додаткову операційну залежність у Kubernetes.
- Neutral, because transcript не містить підтвердження фінальної реалізації Teleport Operator або конкретного identity provider.

## More Information

Transcript facts:

- UI-застосунок для задач згадано як `nitra/task` (`/Users/vitaliytv/www/nitra/task`).
- Структура task-вузлів у сесії: `tasks/ui-task-view/task.md`, `tasks/coverage-skill-test/task.md`, `tasks/skills-orchestrator-migration/task.md`.
- Запропонована label-схема dev pod: `task: <node-name>`, `owner: <email>`, `project: nitra-cursor`.
- Teleport Role може використовувати динамічний шаблон `{{internal.logins}}` для привʼязки `owner` до email користувача.
- Teleport Operator згадано як спосіб декларативно реєструвати dev pods через Kubernetes CRD.
- SSO через GitHub OAuth або Google згадано як достатній identity provider.
- Zed Remote підключається через `~/.ssh/config` і `ProxyCommand tsh proxy ssh --cluster=nitra %h:%p`.

## Update 2026-06-07

Уточнено Kubernetes-контекст для доступу до task-вузлів:

- task-вузли мають виконуватись у Kubernetes.
- Рекомендований dev-доступ: dev pod монтує той самий PVC, що й worker-поди, щоб Zed Remote бачив живий стан `tasks/`.
- Прямий `kubectl port-forward pod/n-graph-dev 2222:22` обговорювався як технічний варіант доступу, але пізніше відхилений для розробників без `kubectl`-прав.
- Для UI окремо зафіксовано, що веб-інтерфейсу не потрібно монтувати `tasks/` напряму: він може читати стан через API-сервер на базі `mt scan --json`, REST або SSE.
- Варіанти назв UI-проєкту (`n-graph`, `graphwatch`, `taskflow`) згадані в transcript, але остаточне підтвердження назви не зафіксовано.

## Update 2026-06-07

Перед вибором Teleport зафіксовано архітектурний принцип: розробники не повинні отримувати прямі `kubectl` credentials, а доступ до dev-середовища має контролювати backend/gateway.

Додаткові transcript facts:
- `kubectl port-forward` відхилено як основний шлях, бо він вимагає `kubectl`-доступу у розробника.
- Gateway має перевіряти права доступу перед відкриттям SSH-зʼєднання до dev pod.
- Рівень доступу до dev pod може бути read або rw залежно від ролі.
- Вибір між Teleport і власним gateway у цьому ранньому фрагменті ще не був завершений; наступний transcript зафіксував Teleport як обраний варіант.
