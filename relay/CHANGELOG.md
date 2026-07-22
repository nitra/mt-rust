# Changelog

## [0.8.1] - 2026-07-21

### Fixed

- eslint (unicorn/prefer-uint8array-base64, unicorn/prefer-iterator-to-array, max-classes-per-file) на релей-модулях v4: DevPushSink винесено у push-sink.mjs, Buffer base64 → Uint8Array.fromBase64/toBase64, regex-літерали тестів — у module-scope, iterator.toArray() замість spread

## [0.8.0] - 2026-07-17

### Added

- Протокол v4 для multi-owner (owner-app, спека 260714): WS-кадри membership (invite/accept/decline/transfer_ownership/bootstrap_owners), Ed25519-підписаний акт transfer (mt-transfer-v4, дзеркальні sign_transfer/verify_transfer у agent-protocol і signing.mjs relay), push-модуль (тип 2 «запрошено», тип 3 «потребує уваги» + адресна Escalation), Event::Escalation у протоколі, directory-модуль mt-core (.mt/directory.json, handle → email поза git), валідація hex-pubkey пристроїв

### Changed

- release: @7n/mt@0.26.1

## [0.7.1] - 2026-07-14

### Changed

- внутрішні константи FRAME_LIMIT/BUFFER_LIMIT/ROLES більше не експортуються (knip: unused exports)

## [0.7.0] - 2026-07-12

### Changed

- Додано RelayCore для управління кімнатами, ролями та membership API
- Додано RelayCore, Rooms та Server для управління кімнатами та членством
- Додано відстеження `from_host` для клієнтських envelope
- Додано обробку pubkeys-кадру у server.mjs та його тест

## [0.6.0] - 2026-07-12

### Changed

- Додано RelayCore для управління кімнатами, ролями та membership API
- Додано RelayCore, Rooms та Server для управління кімнатами та членством
- Додано відстеження `from_host` для клієнтських envelope
- Додано обробку pubkeys-кадру у server.mjs та його тест

## [0.5.0] - 2026-07-12

### Changed

- Додано RelayCore для управління кімнатами, ролями та membership API
- Додано RelayCore, Rooms та Server для управління кімнатами та членством
- Додано відстеження `from_host` для клієнтських envelope
- Додано обробку pubkeys-кадру у server.mjs та його тест

## [0.4.0] - 2026-07-12

### Changed

- Додано RelayCore для управління кімнатами, ролями та membership API
- Додано RelayCore, Rooms та Server для управління кімнатами та членством
- Додано відстеження `from_host` для клієнтських envelope
- Додано обробку pubkeys-кадру у server.mjs та його тест

## [0.3.0] - 2026-07-12

### Changed

- Додано RelayCore для управління кімнатами, ролями та membership API
- Додано RelayCore, Rooms та Server для управління кімнатами та членством
- Додано відстеження `from_host` для клієнтських envelope

## [0.2.0] - 2026-07-12

### Changed

- Додано RelayCore для управління кімнатами, ролями та membership API
- Додано RelayCore, Rooms та Server для управління кімнатами та членством

All notable changes to this project will be documented in this file.

## [0.1.0] - 2026-07-11

### Added

- Initial changelog for `@7n/relay`.
