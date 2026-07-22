---
type: ADR
title: Teleport як SSH gateway для доступу розробників до Kubernetes task-node
description: Розробники підключаються до dev pods через Teleport, без прямого kubectl-доступу до кластера.
---

**Status:** Accepted
**Date:** 2026-06-07

## Context and Problem Statement

Система `mt` запускатиме task-вузли у Kubernetes. Розробникам потрібен SSH-доступ до dev pods, зокрема для Zed Remote, інспекції та патчингу конкретних task-node. Водночас transcript фіксує вимогу: розробники не мають і не повинні мати прямого `kubectl`-доступу до кластера. Авторизацію того, хто до якого вузла може підключитись, має контролювати backend-застосунок або identity-aware gateway, а не ручна видача Kubernetes credentials.

## Considered Options

- Teleport як identity-aware SSH gateway з label-based RBAC.
- `kubectl port-forward` для прямого SSH-доступу до pod.

## Decision Outcome

Chosen option: "Teleport як identity-aware SSH gateway з label-based RBAC", because `kubectl port-forward` вимагає `kubectl`-доступу в розробника, а transcript явно відхиляє таку модель доступу. Teleport дозволяє підключатися через стандартний SSH flow і контролювати доступ через identity, short-lived certificates та labels pod/node.

### Consequences

- Good, because backend може створювати dev pod з labels на кшталт `owner: <email>`, `task: <node-name>`, `project: nitra-cursor`, а gateway надає доступ лише дозволеному користувачу.
- Good, because Zed Remote не потребує спеціальної інтеграції: transcript описує підключення через `~/.ssh/config` і `ProxyCommand tsh proxy ssh`.
- Good, because Teleport використовує short-lived SSH-сертифікати замість довгоживучих статичних ключів.
- Bad, because потрібно задеплоїти й підтримувати Teleport Auth Server і Proxy як додаткову операційну залежність у кластері.
- Neutral, because transcript не містить підтвердження, що конкретна Helm/Operator-конфігурація вже реалізована.

## More Information

Transcript facts:
- UI/backend застосунок для задач згадано як `nitra/task` у `/Users/vitaliytv/www/nitra/task`.
- Dev pod label-схема: `task: <node-name>`, `owner: <email>`, `project: nitra-cursor`.
- Teleport Role може використовувати динамічний шаблон `{{internal.logins}}` для привʼязки owner/email.
- Teleport Operator через Kubernetes CRD розглядався як спосіб декларативної реєстрації dev pods.
- SSO варіанти в transcript: GitHub OAuth або Google.
- Приклад SSH proxy з transcript: `ProxyCommand tsh proxy ssh --cluster=nitra %h:%p`.

## Update 2026-06-07

Перед вибором Teleport transcript зафіксував архітектурний принцип: розробники не повинні отримувати прямий `kubectl`-доступ до кластера для роботи із dev pods.

Додаткові уточнення:
- `kubectl port-forward` відхилено як базовий механізм, бо він вимагає Kubernetes credentials у розробника;
- доступ має контролювати backend/gateway, який перевіряє права перед SSH-зʼєднанням;
- рівень доступу до task-node може бути read або rw залежно від ролі;
- вибір між Teleport і власним gateway у цьому ранньому драфті ще не був зафіксований.
