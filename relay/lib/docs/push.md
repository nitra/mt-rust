---
type: JS Module
title: push.mjs
resource: relay/lib/push.mjs
docgen:
  crc: d6df4c66
  model: openai-codex/gpt-5.5
  score: 100
  issues: judge:inaccurate:0.98
  judgeModel: openai-codex/gpt-5.4-mini
---

## Огляд

Файл задає relay для push-нотифікацій типів `2` («вас запрошено») і `3` («задача потребує уваги»), щоб відокремити маршрутизацію подій від реальної FCM-доставки. `PushRouter` спрямовує push за `event.type` і адресним `event.to_account_id`, спираючись на конфіг `directory.json` для резолву отримувача, а `DevPushSink` як dev-реалізація зберігає події в памʼяті через sink-контракт.

## Поведінка

- `DevPushSink` накопичує dev-доставки push-нотифікацій у памʼяті замість реальної FCM-доставки, щоб relay мав той самий контракт sink-а для сценаріїв «вас запрошено» і «задача потребує уваги».
- `PushRouter` маршрутизує push-нотифікації за даними store та sink-а: надсилає запрошення на наявний акаунт, а attention-події — адресату або учасникам задачі без автора; для адресних подій спирається на резолв отримувача через `directory.json` і не розбирає payload далі роутінгових полів.

## Публічний API

- DevPushSink — накопичує push-доставки в памʼяті для dev-сценаріїв; спирається на `directory.json`.
- PushRouter — спрямовує push-повідомлення через сховище і sink для єдиного маршруту доставки; спирається на `directory.json`.

## Гарантії поведінки

- Read-only: не виконує операцій запису (ФС/БД).
