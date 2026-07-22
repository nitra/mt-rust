---
type: ADR
title: Teleport як SSH gateway для task-нод у Kubernetes
description: Розробники підключаються до dev pods через Teleport без прямого kubectl-доступу.
---

**Status:** Accepted

**Date:** 2026-06-07

## Context and Problem Statement

Система `mt` запускає task-вузли у Kubernetes. Розробникам потрібен Zed Remote SSH-доступ до dev pods для інспекції та патчингу task-нод, але transcript фіксує, що вони не мають і не повинні мати прямого `kubectl`-доступу до кластера. Потрібен gateway, де backend контролює, хто до якого task-вузла може підключитися.

## Considered Options

- Teleport як identity-aware SSH gateway з label-based RBAC.
- `kubectl port-forward` для прямого SSH-доступу до pod.

## Decision Outcome

Chosen option: "Teleport як identity-aware SSH gateway", because `kubectl port-forward` вимагає `kubectl`-доступу у розробника, а transcript явно відхиляє такий доступ; Teleport дозволяє контролювати доступ через labels і short-lived SSH-сертифікати.

### Consequences

- Good, because backend може створювати dev pod з label `owner: email`, а Teleport надає доступ лише відповідному користувачу.
- Good, because Zed підключається через стандартний SSH з `ProxyCommand tsh proxy ssh`, без патчів у Zed.
- Good, because Teleport використовує short-lived SSH certificates замість статичних ключів.
- Bad, because потрібно задеплоїти Teleport Auth Server і Proxy як додаткову операційну залежність.
- Neutral, because transcript не фіксує фінальну реалізацію backend-інтеграції, лише варіанти Teleport Operator і label-based доступу.

## More Information

Transcript facts:

- UI-застосунок для задач: `nitra/task` (`/Users/vitaliytv/www/nitra/task`).
- Dev pod labels: `task: <node-name>`, `owner: <email>`, `project: nitra-cursor`.
- Teleport Role може використовувати `{{internal.logins}}` для привʼязки login до email користувача.
- Teleport Operator через Kubernetes CRD може декларативно реєструвати dev pods.
- SSO варіанти: GitHub OAuth або Google.
- SSH config згадує `ProxyCommand tsh proxy ssh --cluster=nitra %h:%p`.

## Update 2026-06-07

Перед вибором Teleport transcript зафіксував Kubernetes-модель виконання:

- task-вузли живуть у Kubernetes.
- Worker pods і dev pod монтують спільний `tasks-pvc`.
- Dev pod дає Zed Remote доступ до живого файлового стану задач.
- Запропонована тимчасова команда доступу: `kubectl port-forward pod/n-graph-dev 2222:22`, але подальший transcript відхилив прямий `kubectl`-доступ для розробників.

Для UI окремо зафіксовано, що веб-інтерфейс має читати стан через API/REST/SSE поверх `mt scan --json`, а не через пряме монтування `tasks/`.

## Update 2026-06-07

Transcript до фінального вибору Teleport зафіксував архітектурний принцип: розробники не повинні отримувати прямий `kubectl`-доступ, а авторизація SSH-доступу до dev pods має проходити через backend/gateway.

Розглянуті варіанти:
- `kubectl port-forward` + SSH у pod — відхилено, бо потребує `kubectl`-прав у розробника.
- SSH gateway / bastion з backend-авторизацією — прийнято як напрям.

Також згадано, що dev pod монтує `tasks-pvc`, а рівень доступу read/rw має залежати від ролі користувача.
