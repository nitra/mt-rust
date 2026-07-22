# Не приймати Goose (Block) як заміну agent-core стеку

**Status:** Accepted
**Date:** 2026-07-12

## Context and Problem Statement

Власний Rust-стек агента (`crates/agent-core`, `agent-cli`, `agent-server`, `agent-protocol`) ще в розробці (M2/M3, статус «планується» за `npm/docs/architecture/stack.md`). Goose (Block, тепер Linux Foundation Agentic AI, Rust, Apache-2.0) — зрілий open-source агентний фреймворк (крейти `goose`/`goose-cli`/`goose-server`/`goose-mcp`/`goose-acp`) з готовим MCP-registry, 15+ провайдерами і recipe-системою (YAML: prompt, extensions, provider, retry-політика). Питання: чи варто прийняти Goose як залежність замість власного agent-core стеку, чи закривати прогалини (насамперед MCP) точковими рішеннями.

## Considered Options

* Повний swap на Goose — замінити `agent-core`+`agent-cli`+`agent-server` фреймворком Goose цілком.
* Goose лише як «мозок» усередині кастомного `agent-server`/`agent-protocol` — Goose відповідає за agent loop/providers, наш `agent-server` лишається хостом протоколу.
* Лишити власний стек, закрити MCP-прогалину крейтом `rmcp` напряму в `agent-core`.
* Статус-кво без змін.

## Decision Outcome

Chosen option: «Лишити власний стек, закрити MCP-прогалину крейтом `rmcp` напряму в `agent-core`», because:

- `crates/agent-protocol` (Envelope/Event, protocol v4, Ed25519-підписи approvals) — контракт між `agent-server` (host-процес) і тонкими клієнтами (desktop/mobile Tauri, `agent-cli attach`). Повний swap на Goose вимагав би або переписати ці клієнти під Goose-івську сесійну модель, або будувати адаптер `Envelope`/`Event` ↔ Goose — велика площа змін заради фреймворка, що вже й так лише «референс» для нас.
- `npm/docs/architecture/stack.md` уже фіксує Goose (`aaif-goose/goose`) як референсну кодову базу «для рішень, не для копіювання» — структуру (core/cli/server/mcp, sessions/providers/config) уже запозичено при проєктуванні власного стеку. Це свідомий вибір, а не прогалина.
- Головний практичний аргумент за Goose — готовий MCP — закривається дешевше й без ризику для протоколу: підключити крейт `rmcp` (той самий SDK, яким користується сам Goose) напряму в `agent-core::tools.rs` (`register_external(...)` заділ уже існує), не чіпаючи agent loop і `agent-protocol`.
- Retry ladder (`MT_ATTEMPT`/`MT_RETRY_STRATEGY`) уже реалізований у JS-оркестраторі (`npm/lib/commands/run.mjs`) і не залежить від вибору Rust-агента — Goose не дав би тут додаткового виграшу.
- `crates/mt-napi` агентного стеку не торкається (Rust-агент запускається окремим підпроцесом через WS, не embedded) — це не аргумент ні за, ні проти Goose, лише знімає один з розглянутих ризиків.

### Consequences

* Good, because повний контроль над протоколом host↔клієнт (Envelope/Event, approval-модель) залишається в нас, без залежності від чужої еволюції протоколу.
* Good, because MCP-прогалина закривається малою, ізольованою зміною (`rmcp` у `tools.rs`), сумісною з CI-межею «`agent-core` без стороннього agent-фреймворку».
* Good, because архітектура вже узгоджена з цим рішенням (`stack.md` явно позначає Goose як «для рішень, не для копіювання») — нема потреби переглядати вже прийняте.
* Bad, because self-maintenance agent loop, provider-абстракції (`provider.rs`/`provider_openai.rs`) і подальші провайдер-фічі (напр. нативний Anthropic API замість LiteLLM-проксі) лишаються на нас, а не «безкоштовні» через Goose.
* Bad, because втрачаємо 70+ MCP-інтеграцій «з коробки», які Goose постачає як bundled extensions — доведеться підключати чи писати MCP-сервери окремо (хоча вони сумісні з чистим `rmcp`, підключення все одно ручне).

## More Information

- `npm/docs/architecture/stack.md` — розділ «LLM-провайдери» (OpenAI-сумісний Chat Completions як спільний знаменник, LiteLLM-профіль для хмарних моделей) і розділ «Референсні кодові бази» (Goose, `openai/codex`, `pi_agent_rust`).
- `crates/agent-protocol` — `Envelope`/`Event`, `ClientHello`/`ServerHello`, Ed25519 approvals (protocol v4).
- `crates/agent-core/src/tools.rs` — `register_external(...)` заділ під MCP, закоментований намір на `rmcp`.
- `npm/lib/commands/run.mjs` — реалізація retry ladder (`MT_ATTEMPT`/`MT_RETRY_STRATEGY`), незалежна від вибору Rust-агентного фреймворку.
- Окреме (менше за обсягом) питання — заміна `provider_openai.rs` на крейт `genai` (jeremychone/rust-genai) для прямих нативних викликів провайдерів без LiteLLM-хопа — не заборонене цим ADR, розглядається окремо як точковий рефакторинг провайдер-шару в межах `Provider`-трейта.
- Переглянути це рішення, якщо зʼявиться новий аргумент, якого не було на момент запису (напр. вартість власної MCP/multi-provider реалізації виявиться суттєво вищою за очікувану).
