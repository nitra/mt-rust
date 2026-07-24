# mt-rust

Повна реалізація протоколу MT (задачний граф + agent-протокол) на Rust.

- Специфікація протоколу: [github.com/nitra/mt](https://github.com/nitra/mt) — публікується в npm як `@7n/mt` (з 0.29.0; версії ≤0.28.0 — це старий CLI)
- JS-обгортка над Rust-бінарником `mt` (без власної command-логіки): [github.com/nitra/mt-js](https://github.com/nitra/mt-js)
- CLI `mt` — реалізовано в цьому репозиторії (`crates/mt`), дистрибуція без npm: `cargo install --path crates/mt` або GitHub Releases

## Специфікація в цьому репозиторії

Пакет `@7n/mt` (сама специфікація, не CLI) підключений як `devDependency` — після
`bun install` вона читабельна на диску без окремого клонування:

```sh
cat node_modules/@7n/mt/docs/index.md
```

Оновлюється звичайним бампом версії в кореневому `package.json`.

## Карта крейтів (`crates/`)

| Крейт | Призначення |
| --- | --- |
| `mt-core` | Core-бібліотека задачного графу `@7n/mt`: сканування, створення, lifecycle, claims/publish, run-wrapper |
| `mt` | CLI-бінарник `mt` — тонкий шар над `mt-core` (clap), без napi/subprocess-посередників |
| `agent-protocol` | Протокол подій v4 для `agent-server`: Envelope/Event, хендшейк, Ed25519-підписи approvals |
| `agent-core` | ACP-клієнт (Agent Client Protocol) — єдиний транспорт AI-викликів до зовнішніх підписочних CLI |
| `agent-server` | Хост-процес M1: session host протоколу v4 — Envelope/журнал/broadcast, WS-хендшейк, discovery |
| `agent-cli` | Тонкий клієнт `agent-server`: `serve` (хост-процес) і `attach` (інтерактивна сесія вузла) |

## Інше в репозиторії

- `relay/` — `@7n/relay`, Bun/JS координатор relay-частини протоколу (кімнати Envelope, membership, роздача pubkey)
- `layers/` — `@7n/layers`, рушій шарової документації (подвійний CRC, LLM-генерація верхніх шарів, derived-переклади); корпус доків, який він обробляє, живе в [nitra/mt](https://github.com/nitra/mt) (`docs/`), тут — лише сам рушій. Запуск проти sibling-checkout: `bun ./layers/lib/cli.mjs status ../mt/docs`
- `mt/` — dogfood-граф задач цього проєкту (`@7n/mt` on `@7n/mt`)
- `docs/adr/` — інженерна історія архітектурних рішень (ADR)

## Розробка

```sh
bun install
cargo build --workspace
cargo test --workspace
bun test
```
