---
type: Rust Module
title: handoff_ws.rs
resource: crates/agent-server/tests/handoff_ws.rs
docgen:
  crc: bf0ca276
  model: openai-codex/gpt-5.4-mini
  tier: cloud-min
  score: 80
---

## Огляд

Кооперативний handoff на рівні AppState/session (runtime.md, «Міграція сесії між хостами», кроки 2-3): дві незалежні `AppState` (окремі `state_dir` — симуляція двох хостів), той самий git-репозиторій. Хід на хості 1 → `handoff_node` → `resume_node` на хості 2 з тим самим тікетом → журнал успадкований, наступний хід продовжує seq без розривів.

## Гарантії поведінки

- (специфічних машинно-виведених гарантій немає)
