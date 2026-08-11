-- Схема relay (access.md, «Схема даних») у діалекті SQLite.
--
-- Персистентне у relay — ЛИШЕ акаунти/пристрої/membership/запрошення:
-- журнали сесій, git і lease relay не тримає, тож таблиць під них тут
-- немає і бути не має.
--
-- UUID зберігаються як TEXT: SQLite не має власного типу, а генерація
-- лишається на боці JS (`randomUUID`) — той самий формат, що бачить решта
-- relay і pubkey-кеш хоста.
--
-- Ідемпотентно: застосовується на старті кожного інстансу.

PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS accounts (
  account_id   TEXT PRIMARY KEY,
  email        TEXT NOT NULL UNIQUE,
  display_name TEXT NOT NULL DEFAULT ''
);

CREATE TABLE IF NOT EXISTS devices (
  device_id    TEXT PRIMARY KEY,
  account_id   TEXT NOT NULL REFERENCES accounts(account_id) ON DELETE CASCADE,
  name         TEXT NOT NULL DEFAULT '',
  role         TEXT NOT NULL,
  -- hex Ed25519 (32 байти) — той самий формат, що очікує pubkey-кеш хоста.
  pubkey       TEXT NOT NULL,
  device_token TEXT NOT NULL UNIQUE,
  last_seen    TEXT
);

CREATE INDEX IF NOT EXISTS devices_account_idx ON devices(account_id);

CREATE TABLE IF NOT EXISTS tasks (
  root_node_hash TEXT PRIMARY KEY,
  owner_account  TEXT NOT NULL REFERENCES accounts(account_id),
  project_name   TEXT NOT NULL DEFAULT '',
  remote_url     TEXT NOT NULL DEFAULT '',
  created_at     TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS task_members (
  root_node_hash TEXT NOT NULL REFERENCES tasks(root_node_hash) ON DELETE CASCADE,
  account_id     TEXT NOT NULL REFERENCES accounts(account_id) ON DELETE CASCADE,
  -- owner | host | approver | viewer
  role           TEXT NOT NULL,
  invited_by     TEXT REFERENCES accounts(account_id),
  joined_at      TEXT NOT NULL,
  PRIMARY KEY (root_node_hash, account_id)
);

CREATE TABLE IF NOT EXISTS invitations (
  invitation_id  TEXT PRIMARY KEY,
  root_node_hash TEXT NOT NULL REFERENCES tasks(root_node_hash) ON DELETE CASCADE,
  from_account   TEXT NOT NULL REFERENCES accounts(account_id),
  to_email       TEXT NOT NULL,
  role           TEXT NOT NULL,
  -- pending | accepted | declined | revoked
  status         TEXT NOT NULL DEFAULT 'pending',
  created_at     TEXT NOT NULL
);

-- Ідемпотентність bootstrap-у запрошень: одне відкрите запрошення на
-- (задача, email). Часткова унікальність — щоб історія accepted/declined
-- не заважала запросити людину повторно.
CREATE UNIQUE INDEX IF NOT EXISTS invitations_pending_unique
  ON invitations(root_node_hash, to_email)
  WHERE status = 'pending';
