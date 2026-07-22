# Підписочні CLI-виконавці вузла (`agent_cli`) замість власного provider-шляху в першому кільці

**Status:** Accepted
**Date:** 2026-07-13

## Context and Problem Statement

Вбудований agent-шлях `mt run` був жорстко закодований на `claude` CLI, а цільовий стек передбачав власний provider-шар (`agent-core`: `async-openai` + LiteLLM-профіль для хмарних моделей) як критичний шлях. Паралельно розглядались агентні рантайми (pi, pi_agent_rust, codex-core, Goose — останній відхилено окремим ADR) як «мозок» вузла. Питання: чий agent loop і чиї ключі потрібні MT для першого кільця dogfooding-у, якщо цільова аудиторія вже має підписки на вендорські coding-CLI (Claude Code, Codex, Cursor)?

## Considered Options

* Підписочні CLI (claude / codex / cursor) як вбудований agent-шлях; вибір per-node.
* Власний provider-шар (`agent-core` + async-openai + LiteLLM) як критичний шлях першого кільця.
* Прийняти зовнішній агентний рантайм (pi / pi_agent_rust / codex-core) як embedded «мозок».
* Статус-кво: жорстко закодований `claude`-шлях.

## Decision Outcome

Chosen option: «Підписочні CLI як вбудований agent-шлях; вибір per-node», because:

- **Найбільше спрощення першого кільця:** agent loop, tools, sandbox, вибір моделі й білінг привозить вендорський CLI, авторизований користувачем локально під власною підпискою. MT не тримає API-ключів, не білінгує токени, не потребує LiteLLM-прокладки; власний provider-шар (`agent-core`) зсувається у друге кільце (локальні моделі omlx/Ollama, headless без підписки) і зникає з критичного шляху.
- **Юридично чиста форма:** run виконується на хості, де owner вузла сам авторизував CLI, для його задач — штатне використання підписки. Нормативне правило: підписки не пулюються і не проксюються через relay/сервер; relay передає лише події та approvals.
- **Крос-програмковий вимір vision.md стає реальним уже зараз:** `agent_cli` — per-node прапор `a.md` (секція `## Agent cli`) з user-level дефолтом (env `MT_AGENT_CLI`, ADR `260713-2110`) — спеціалізований тул на вузол. Гранулярність осей різна: `node_executor` (чий harness виконує граф) лишається глобальним; `agent_cli` (який CLI всередині вбудованого шляху) — per-node.
- **MT зберігає всю унікальну цінність:** claim/lease, worktree-ізоляція, budget/timeout, retry ladder, `## Check`, fenced publish — оркестрація не делегується. Логіка Goose-ADR («не своп, а адаптер на межі») застосована і тут.
- Побічне вирівнювання: `## Check`-гейт тепер спільний для обох шляхів (вбудованого CLI і `node_executor`) — success вбудованого шляху = fact існує **і** Check пройдено (раніше вбудований шлях мержив без Check).

Реалізація: таблиця `AGENT_CLIS` у `npm/lib/commands/run.mjs` (claude → `--model`; codex → `codex exec -m … --full-auto`; cursor → `cursor-agent --model … --print --force`), env `MT_AGENT_CLI`, fail-fast на невідомому значенні до створення worktree, спільний генералізований читач прапор-секцій `a.md` (`## Model tier` / `## Retry ladder` / `## Agent cli`).

**Тир → конкретна модель per-CLI.** Канон MIN/AVG/MAX не делегується CLI «на розсуд»: мапа «CLI → тир → модель» резолвить тир у конкретну модель обраного CLI (напр. codex: MIN→`gpt-5.6-luna`, AVG→`gpt-5.6-terra`, MAX→`gpt-5.6-sola`), тож retry ladder ескалює не лише тир, а й фактичну модель. CLI без мапінгу резолвить модель сам (тир — hint env `MT_MODEL_TIER`). Правило спільне для headless-викликів і ACP-сесій (`resolveModelForCli` у `npm/lib/core/config.mjs`); механіка конфігурації — user-level ENV (`MT_AGENT_CLI` / `MT_CLOUD_AGENT_CLIS` / `MT_AGENT_CLI_MODEL_MAP`), ADR `260713-2110`.

### Consequences

* Good, because перше кільце dogfooding їде без добудови `provider_openai.rs`/LiteLLM — менша поверхня коду і нуль секретів у MT.
* Good, because vendor-нейтральність підтверджується практикою: три взаємозамінні CLI за одним контрактом вузла.
* Bad, because телеметрія tokens/cost — best-effort (що віддає CLI), бюджети підписочного шляху — soft-alert (hard-межа — `budget_hard_sec` kill); rate limits підписки — зовнішній ресурс, оркестратор має робити backoff.
* Bad, because headless-режими не паузяться на mid-run approval — вузли з approval-гейтами вимагають сесійного транспорту; цільове рішення — ACP (Agent Client Protocol): один ACP-клієнт в agent-server, `permission-request` → `ApprovalRequest` (Ed25519). Окремий ADR після спайку.
* Bad, because матриця сумісності: headless-прапори трьох вендорських CLI змінюються швидко — потрібна `mt doctor`-перевірка наявності/версій.

## More Information

- `npm/docs/architecture/runtime.md` — розділ «Підписочні CLI-виконавці (`agent_cli`)»: таблиця CLI, правило підписки, ACP-намір, телеметрія.
- `npm/docs/architecture/stack.md` — «LLM-провайдери»: перше кільце (підписочні CLI) / друге кільце (власний provider-транспорт).
- `npm/docs/architecture/graph.md` — `a.md`: поле `agent_cli`, гранулярність осей `node_executor` vs `agent_cli`.
- `npm/lib/commands/run.mjs` + `npm/lib/tests/run.test.mjs` — реалізація і тести (codex-диспатч, per-node override, fail-fast, спільний `## Check`-гейт).
- `docs/adr/не-приймати-goose-block-агентний-фреймворк.md` — споріднене рішення про межу з чужими агентними фреймворками.
- Переглянути, якщо вендорські ToS обмежать headless-використання підписок або з'явиться стабільний ACP-адаптер у всіх трьох CLI (тоді headless-таблиця може зʼїхати на ACP цілком).
