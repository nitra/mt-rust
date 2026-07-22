---
type: ADR
title: Teleport SSH gateway для Kubernetes task nodes
description: Доступ розробників до dev pods у Kubernetes проходить через Teleport, а не через прямий kubectl-доступ.
---

**Status:** Accepted
**Date:** 2026-06-07

## Context and Problem Statement

Система `mt` запускатиме task-вузли у Kubernetes. Розробникам потрібен SSH-доступ до dev pods, зокрема для Zed remote, інспекції та патчингу конкретних task-нод. Водночас розробники не мають і не повинні мати прямого `kubectl`-доступу до кластера. Авторизацію має контролювати backend-застосунок, а не k8s RBAC напряму для кожного розробника.

## Considered Options

- Teleport як identity-aware SSH gateway з label-based RBAC.
- `kubectl port-forward` для прямого SSH-доступу до pod.

## Decision Outcome

Chosen option: "Teleport як identity-aware SSH gateway з label-based RBAC", because `kubectl port-forward` вимагає наявного `kubectl`-доступу у розробника, а це явно відхилено в transcript. Teleport дозволяє backend-у контролювати доступ через labels без видачі kubectl-прав розробникам.

### Consequences

- Good, because backend при створенні dev pod може ставити labels на кшталт `owner: <email>`, а Teleport надає доступ лише відповідному користувачу.
- Good, because Teleport використовує short-lived SSH-сертифікати замість статичних ключів.
- Good, because Zed може підключатися як до звичайного SSH через `~/.ssh/config` і `ProxyCommand tsh proxy ssh`.
- Bad, because потрібно задеплоїти Teleport Auth Server і Proxy як додаткову операційну залежність у кластері.

## More Information

У transcript згадано UI-застосунок для задач `nitra/task` і task-вузли `tasks/ui-task-view/task.md`, `tasks/coverage-skill-test/task.md`, `tasks/skills-orchestrator-migration/task.md`. Dev pod монтує `tasks-pvc`; worker pods і dev pod бачать той самий файловий стан. Label-схема: `task: <node-name>`, `owner: <email>`, `project: nitra-cursor`. Для SSH-конфігурації згадано `ProxyCommand tsh proxy ssh --cluster=nitra %h:%p`. Як SSO достатні GitHub OAuth або Google.

## Update 2026-06-07

Додатково transcript зафіксував модель on-demand dev pod-ів для доступу до task-node:

- Dev pod створюється бекендом `nitra/task` після UI-запиту розробника.
- Бекенд перевіряє права у власній RBAC/БД, виконує `kubectl apply dev-pod.yaml`, проставляє labels `task=<name>` і `owner=<email>`, чекає `Pod Ready`, після чого Teleport node-agent реєструє pod автоматично.
- Pod монтує `tasks-pvc`, тому розробник бачить актуальні `task.md`, `run_NNN.md`, `outputs_NNN.md`.
- Lifecycle: spawn on request, grace period після закриття SSH, auto-delete по timeout або при переході task-node у `resolved`.
- Transcript згадує очікуваний cold-start приблизно `5–15с`, але не містить підтвердження негативного наслідку цього часу.

## Update 2026-06-07

Transcript уточнює UX доступу через `nitra/task`:

- Проєкт UI для task-графу названо `nitra/task` і розташовано у `/Users/vitaliytv/www/nitra/task`.
- Кнопка "Open in Zed" ініціює backend-controlled flow: перевірка прав → створення dev pod з labels `task=X`, `owner=email` → очікування Ready → Teleport registration → повернення connection string.
- Dev pod монтує `tasks-pvc`; це відрізняє підхід від загального dev-environment, бо pod є точкою доступу до DAG-стану агентів.
- Для Zed transcript фіксує стандартне SSH-підключення через `~/.ssh/config` і `ProxyCommand tsh proxy ssh --cluster=nitra %h:%p`.

## Update 2026-06-07

Transcript додає multi-editor аспект для доступу до dev pod-ів через Teleport:

- VS Code і Cursor підтримуються через URI deep link:
  - `vscode://vscode-remote/ssh-remote+<hostname>.teleport.nitra.com/tasks`
  - `cursor://vscode-remote/ssh-remote+<hostname>.teleport.nitra.com/tasks`
- Zed у transcript не має підтвердженого URI-протоколу, тому для нього лишається fallback: скопіювати hostname і відкрити SSH вручну.
- Усі редактори використовують той самий `~/.ssh/config` з Teleport `ProxyCommand`.
- Transcript фіксує намір перейменувати scope з `open-in-zed` на `open-in-editor`, але не містить підтвердження виконаного перейменування в цьому драфті.

## Update 2026-06-07

Transcript уточнює Kubernetes-реалізацію Teleport/dev-pod доступу:

- У `/Users/vitaliytv/www/nitra/task/` підготовлено k8s manifests: `k8s/teleport/configmap.yaml`, `deployment.yaml`, `service.yaml`, `ingress.yaml`, `pvc.yaml`, `rbac.yaml`, `roles.yaml`, `k8s/dev-pod/template.yaml`, `k8s/dev-pod/rbac.yaml`, `k8s/README.md`.
- Dev pod монтує той самий `tasks-pvc`, що й worker pods `mt`.
- Join method описано як k8s ServiceAccount JWT без статичних токенів.
- RBAC роль `developer` обмежує доступ через `node_labels: {owner: "{{internal.logins}}"}`.
- Transcript фіксує `replicas: 1` для Teleport Auth+Proxy з коментарем про SQLite як обмеження для HA.
