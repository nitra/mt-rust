//! Протокол подій v4 — контракт клієнт↔хост agent-server (спека
//! npm/docs/architecture/runtime.md, «Протокол подій»).
//!
//! Крейт свідомо БЕЗ tokio/tauri — чистий контракт (фізична межа зі
//! stack.md): типи `Envelope`/`Event`, хендшейк `ClientHello`/`ServerHello`
//! з перевіркою сумісності версії та Ed25519-підписи approvals за
//! npm/docs/architecture/access.md.

pub mod approvals;
pub mod envelope;
pub mod handshake;
pub mod transfers;

pub use approvals::{
    sign_approval, verify_approval, ApprovalError, ApprovalPayload, Signature, SigningKey,
    VerifyingKey,
};
pub use envelope::{ClaimInfo, Envelope, Event, Rect};
pub use handshake::{check_protocol_version, ClientHello, ProtocolError, ServerHello, SessionInfo};
pub use transfers::{sign_transfer, verify_transfer, TransferPayload};

/// Поточна версія протоколу подій. v1/v2 — історія scaffold-spec; v3 —
/// проміжний draft без `lang`. Несумісна версія відхиляється на хендшейку
/// з явною помилкою і підказкою оновитись (runtime.md).
pub const PROTOCOL_VERSION: u32 = 4;
