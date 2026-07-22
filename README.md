# mt-rust

Повна реалізація протоколу MT (задачний граф + agent-протокол) на Rust.

- Специфікація протоколу: [github.com/nitra/mt](https://github.com/nitra/mt)
- Тонкий JS-клієнт (npm-пакет `@7n/mt`): [github.com/nitra/mt-js](https://github.com/nitra/mt-js)

## Карта крейтів (`crates/`)

| Крейт | Призначення |
| --- | --- |
| `mt-core` | Core-бібліотека задачного графу `@7n/mt`: сканування, створення, похідні стани |
| `mt-cli` | Transitional CLI-бінарник `mt-scanner` над `mt-core` (JSON pipe) |
| `mt-napi` | napi-rs аддон, що експонує `mt-core` в Node.js/Bun (`@7n/mt`) |
| `agent-protocol` | Протокол подій v4 для `agent-server`: Envelope/Event, хендшейк, Ed25519-підписи approvals |
| `agent-core` | ACP-клієнт (Agent Client Protocol) — єдиний транспорт AI-викликів до зовнішніх підписочних CLI |
| `agent-server` | Хост-процес M1: session host протоколу v4 — Envelope/журнал/broadcast, WS-хендшейк, discovery |
| `agent-cli` | Тонкий клієнт `agent-server`: `serve` (хост-процес) і `attach` (інтерактивна сесія вузла) |

## Інше в репозиторії

- `relay/` — `@7n/relay`, Bun/JS координатор relay-частини протоколу (кімнати Envelope, membership, роздача pubkey)
- `packages/` — платформні npm-підпакети (`@7n/mt-darwin-arm64`, `@7n/mt-linux-x64`) з prebuilt-бінарниками, що збираються в CI цього репозиторію
- `mt/` — dogfood-граф задач цього проєкту (`@7n/mt` on `@7n/mt`)
- `docs/adr/` — інженерна історія архітектурних рішень (ADR)

## Розробка

```sh
bun install
cargo build --workspace
cargo test --workspace
bun test
```
