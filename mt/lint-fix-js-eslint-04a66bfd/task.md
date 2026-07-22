---
schema_version: 1
created_at: 2026-07-17T11:29:02.512Z
budget_sec: 1800
audit: required
hint: atomic
---

## Task

Виправити порушення правила `js` (concern `eslint`), які не закрила інлайн fix-драбина.

## Done when

- `js` не повідомляє порушень у target-файлах (див. ## Check).

## Check

npx @7n/rules lint --no-fix --cwd ../.. js

## Inputs

Target-файли:

- `relay/lib/push.mjs`
- `relay/lib/signing.mjs`
- `relay/lib/tests/relay.test.mjs`
- `relay/lib/tests/server.test.mjs`
