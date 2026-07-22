---
type: ADR
title: "Open in Editor: URI deep links для dev pods"
description: UI відкриває task dev pod у VS Code і Cursor через URI deep links, а для Zed використовує fallback із копіюванням hostname.
---

**Status:** Accepted
**Date:** 2026-06-07

## Context and Problem Statement

UI `nitra/task` має дозволити розробнику відкрити конкретний task-node у редакторі через SSH-доступ до dev pod. Початковий фокус був на Zed, але transcript фіксує потребу підтримати також VS Code і Cursor.

## Considered Options

- Підтримка тільки Zed із ручним копіюванням SSH hostname.
- Підтримка VS Code і Cursor через URI deep links, Zed — через copy hostname.

## Decision Outcome

Chosen option: "Підтримка VS Code і Cursor через URI deep links, Zed — через copy hostname", because VS Code і Cursor підтримують `vscode-remote` URI-схеми для автоматичного відкриття SSH-сесії, а transcript фіксує, що Zed не має аналогічного URI-протоколу.

### Consequences

- Good, because для VS Code і Cursor користувач отримує one-click UX через browser URI.
- Bad, because Zed потребує ручного кроку: скопіювати hostname і відкрити SSH-підключення в редакторі.
- Neutral, because усі редактори використовують однакову SSH-конфігурацію з Teleport `ProxyCommand`.

## More Information

URI formats із transcript:

```text
vscode://vscode-remote/ssh-remote+<hostname>.teleport.nitra.com/tasks
cursor://vscode-remote/ssh-remote+<hostname>.teleport.nitra.com/tasks
```

Спільний SSH transport очікує Teleport-конфігурацію в `~/.ssh/config` із `ProxyCommand tsh proxy ssh --cluster=nitra %h:%p`.

## Update 2026-06-07

- Transcript уточнює, що задача перейменована з `open-in-zed` на `open-in-editor`, бо scope охоплює VS Code, Cursor і Zed.
- Для VS Code і Cursor використовуються URI-схеми `vscode://vscode-remote/ssh-remote+<hostname>.teleport.nitra.com/tasks` та `cursor://vscode-remote/ssh-remote+<hostname>.teleport.nitra.com/tasks`.
- Для Zed залишається fallback через копіювання hostname, оскільки transcript не фіксує підтримки URI deep link у Zed.

## Update 2026-06-07

- Transcript додає deployment facts для `nitra/task`: dev pod створюється on-demand через backend API після натискання "Open in Editor".
- Потік: backend перевіряє права, застосовує `k8s/dev-pod/template.yaml`, чекає `Pod Ready`, після чого Teleport node-agent реєструє pod і UI повертає connection string або editor URI.
- Dev pod монтує той самий `tasks-pvc`, тому editor відкриває актуальний файловий стан task-графу, а не окремий клон.
