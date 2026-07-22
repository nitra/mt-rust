---
schema_version: 1
created_at: 2026-07-12T06:36:14Z
budget_sec: 10800
hint: atomic
---

## Task

M2, підписані approvals у потоці (access.md, «Approvals: три гейти, один механізм»): хост шле `ApprovalRequest` у кімнату → пристрій учасника approver+ підписує `(request_id, approved, node_hash, run_token)` → хост звіряє підпис із pubkey-кешем relay (підпис поза списком → відмова + `Error`) → запит-очікування завершується. Pubkey-кеш наповнюється мостом через новий relay-кадр `pubkeys`.

## Done when

- relay: WS-кадр `{kind:"pubkeys", root}` → `{kind:"pubkeys", root, pubkeys:[{device_id, account_id, pubkey}]}` (ядро `core.pubkeys` вже є); vitest-тест;
- agent-server: `ApprovalGate` — реєстрація pending-запиту (`request_approval` публікує `ApprovalRequest` у сесію, повертає oneshot), `resolve` верифікує Ed25519-підпис (`agent_protocol::verify_approval`) ключем пристрою з кешу за `device_id`; невалідний/невідомий → `Error` у сесію, pending лишається (можна повторити валідним підписом);
- політика: `require_signed` вмикається разом із relay-мостом; без нього (локальний dev) порожній підпис приймається;
- міст: після subscribe запитує `pubkeys`; вхідний `pubkeys`-кадр оновлює кеш і вмикає `require_signed`;
- `handle_client_frame`: гілка `ApprovalResponse` → `ApprovalGate::resolve`;
- тести: unit ApprovalGate (валідний/зіпсований/чужий ключ/непідписаний у двох політиках); інтеграційний із mock-relay: bridge запитує pubkeys → ApprovalRequest у кімнаті → підписаний ApprovalResponse віддаленого пристрою → oneshot true; невалідний підпис → Error-кадр у стрічці;
- `cargo test --workspace` і `npx vitest run relay` зелені.

## Check

cargo test -p agent-server -q
npx vitest run relay

## Inputs

- Нормативні: access.md (потік гейтів, pubkey-кеш із TTL, «підпис поза списком → відмова + Error»; матеріалізація у файли вузла — ОКРЕМА задача: потребує синтезу `run_NNN.md` в інтерактивному done), stack.md (CI-кейс «відхилення підпису пристрою поза pubkey-списком»).
- Готове: `agent_protocol::approvals` (sign/verify, ApprovalPayload), relay-міст (PR #35), `store.pubkeysFor` (PR #34).
- Поза скоупом: TTL-refresh кешу (кеш оновлюється на pubkeys-кадр; періодичний refresh — разом із presence), тригер деструктивного ToolCall (гейт викликається програмно; звʼязка з tool-політикою — окрема задача), plan-review/аудит-вердикт гейти (той самий механізм — після синтезу файлів).
