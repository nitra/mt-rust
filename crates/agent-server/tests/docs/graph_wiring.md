---
type: Rust Module
title: graph_wiring.rs
resource: crates/agent-server/tests/graph_wiring.rs
docgen:
  crc: d9f8a134
  model: openai-codex/gpt-5.4-mini
  tier: cloud-min
  score: 100
  judgeModel: openai-codex/gpt-5.4-mini
---

## Огляд

Інтеграція WS-сесій із graph-мостом: attach на першому UserMessage, журнал у run ref, DoneSession → fenced publish, ReleaseSession → пауза. Все герметично: bare-репо як origin, скриптований runner, реальний WS.

## Поведінка

Під час першого `UserMessage` сесія може прив’язати вузол до поточного тримача; якщо вузол уже зайнятий іншим тримачем, для клієнта приходить `Error` із `claim-lost`, і хід не стартує.

Після завершення ходу публікація відбувається через fenced publish у `main`: `refs/mt/claims/` і `refs/mt/runs/` зникають, а службовий журнал не потрапляє в `.nitra/session.jsonl` на `main`.

`DoneSession` завершує сесію коміт-станом із повідомленням про done; окремі контрактні артефакти спроби лишаються в `main`. `ReleaseSession` лише знімає claim і зберігає run ref та журнал, тож той самий вузол можна прив’язати знову.

## Гарантії поведінки

- (специфічних машинно-виведених гарантій немає)
