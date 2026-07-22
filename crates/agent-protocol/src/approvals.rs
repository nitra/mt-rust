//! Ed25519-підписи approvals (спека access.md, «Approvals: три гейти, один
//! механізм»).
//!
//! Пристрій учасника з роллю approver+ підписує кортеж
//! `(request_id, approved, node_hash, run_token)` власним ключем; хост
//! звіряє підпис із pubkey-кешем relay і матеріалізує у файл вузла.
//! Канонічне повідомлення — доменний префікс + поля через NUL-роздільник
//! (див. [`ApprovalPayload::message`]), однакове для всіх трьох гейтів.

use ed25519_dalek::Signer;
pub use ed25519_dalek::{Signature, SigningKey, VerifyingKey};
use uuid::Uuid;

/// Доменний префікс канонічного повідомлення — захист від повторного
/// використання підпису в іншому контексті.
const DOMAIN: &[u8] = b"mt-approval-v4";

/// Кортеж, який підписує пристрій. Той самий для mid-run tool approval,
/// plan-review і аудит-вердикту людини.
#[derive(Debug, Clone, PartialEq)]
pub struct ApprovalPayload {
    pub request_id: String,
    pub approved: bool,
    /// Кімната/адреса вузла.
    pub node_hash: String,
    /// = token claim-а (ідентифікатор сесії).
    pub run_token: Uuid,
}

impl ApprovalPayload {
    /// Канонічні байти для підпису: `DOMAIN \0 request_id \0 approved-байт
    /// (0x01/0x00) \0 node_hash \0 run_token(hyphenated)`. NUL-роздільник
    /// унеможливлює склейку сусідніх полів.
    pub fn message(&self) -> Vec<u8> {
        let mut message = Vec::with_capacity(
            DOMAIN.len() + self.request_id.len() + self.node_hash.len() + 36 + 5,
        );
        message.extend_from_slice(DOMAIN);
        message.push(0);
        message.extend_from_slice(self.request_id.as_bytes());
        message.push(0);
        message.push(u8::from(self.approved));
        message.push(0);
        message.extend_from_slice(self.node_hash.as_bytes());
        message.push(0);
        message.extend_from_slice(self.run_token.hyphenated().to_string().as_bytes());
        message
    }
}

/// Помилка перевірки підпису approval-а.
#[derive(Debug, Clone, PartialEq)]
pub enum ApprovalError {
    /// Ed25519-підпис — рівно 64 байти.
    BadSignatureLength { actual: usize },
    /// Підпис не сходиться з payload-ом чи pubkey-ем (зіпсований, чужий
    /// ключ або підмінене поле кортежу).
    VerificationFailed,
}

impl std::fmt::Display for ApprovalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApprovalError::BadSignatureLength { actual } => {
                write!(f, "approval signature must be 64 bytes, got {actual}")
            }
            ApprovalError::VerificationFailed => {
                write!(f, "approval signature verification failed")
            }
        }
    }
}

impl std::error::Error for ApprovalError {}

/// Підписати approval приватним ключем пристрою. Байти для
/// `ApprovalResponse.signature` — `signature.to_bytes()`.
pub fn sign_approval(key: &SigningKey, payload: &ApprovalPayload) -> Signature {
    key.sign(&payload.message())
}

/// Перевірити підпис проти pubkey пристрою (з pubkey-кешу relay).
/// Використовує `verify_strict` — відхиляє нестандартні (malleable) підписи.
pub fn verify_approval(
    pubkey: &VerifyingKey,
    payload: &ApprovalPayload,
    signature: &[u8],
) -> Result<(), ApprovalError> {
    let bytes: [u8; 64] = signature
        .try_into()
        .map_err(|_| ApprovalError::BadSignatureLength {
            actual: signature.len(),
        })?;
    pubkey
        .verify_strict(&payload.message(), &Signature::from_bytes(&bytes))
        .map_err(|_| ApprovalError::VerificationFailed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload() -> ApprovalPayload {
        ApprovalPayload {
            request_id: "req-1".into(),
            approved: true,
            node_hash: "d".repeat(20),
            run_token: Uuid::from_u128(42),
        }
    }

    fn key() -> SigningKey {
        SigningKey::from_bytes(&[7u8; 32])
    }

    #[test]
    fn sign_then_verify_ok() {
        let signature = sign_approval(&key(), &payload());
        assert_eq!(
            verify_approval(&key().verifying_key(), &payload(), &signature.to_bytes()),
            Ok(())
        );
    }

    /// Зіпсований підпис (один перевернутий байт) — відмова.
    #[test]
    fn corrupted_signature_fails() {
        let mut bytes = sign_approval(&key(), &payload()).to_bytes();
        bytes[10] ^= 0xFF;
        assert_eq!(
            verify_approval(&key().verifying_key(), &payload(), &bytes),
            Err(ApprovalError::VerificationFailed)
        );
    }

    /// Підміна будь-якого поля кортежу (тут `approved`) інвалідовує підпис.
    #[test]
    fn tampered_payload_fails() {
        let bytes = sign_approval(&key(), &payload()).to_bytes();
        let tampered = ApprovalPayload {
            approved: false,
            ..payload()
        };
        assert_eq!(
            verify_approval(&key().verifying_key(), &tampered, &bytes),
            Err(ApprovalError::VerificationFailed)
        );
    }

    /// Ключ поза pubkey-списком — відмова.
    #[test]
    fn foreign_key_fails() {
        let bytes = sign_approval(&SigningKey::from_bytes(&[9u8; 32]), &payload()).to_bytes();
        assert_eq!(
            verify_approval(&key().verifying_key(), &payload(), &bytes),
            Err(ApprovalError::VerificationFailed)
        );
    }

    #[test]
    fn wrong_length_is_explicit_error() {
        assert_eq!(
            verify_approval(&key().verifying_key(), &payload(), &[1, 2, 3]),
            Err(ApprovalError::BadSignatureLength { actual: 3 })
        );
    }

    /// NUL-роздільник: зсув межі полів дає ІНШЕ повідомлення.
    #[test]
    fn message_field_boundaries_are_unambiguous() {
        let a = ApprovalPayload {
            request_id: "ab".into(),
            node_hash: "c".into(),
            ..payload()
        };
        let b = ApprovalPayload {
            request_id: "a".into(),
            node_hash: "bc".into(),
            ..payload()
        };
        assert_ne!(a.message(), b.message());
    }
}
