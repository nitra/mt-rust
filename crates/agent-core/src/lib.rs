//! `agent-core` — ACP-клієнт (Agent Client Protocol).
//!
//! ACP — **єдиний транспорт AI-викликів** (ADR `260713-2110`): виконавці —
//! зовнішні підписочні CLI (claude / codex / cursor / pi для локальних
//! omlx-моделей), кожен підключається своїм ACP-адаптером;
//! `session/request_permission` мапиться на `ApprovalRequest` протоколу
//! (Ed25519). Власного agent loop, реєстру tools і provider-шару тут НЕМАЄ —
//! це свідомо видалені відхилення від ACP-норми.

pub mod acp;

pub use acp::{AcpClient, AcpError, PermissionHandler};
