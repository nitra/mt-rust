//! Ed25519-підпис акта transfer ownership (access.md, «Membership API»).
//!
//! Передача власності задачі — криптографічний факт, а не лише право
//! device_token-а: пристрій поточного owner-а підписує canonical-акт,
//! relay перевіряє підпис проти pubkey пристрою (relay/lib/signing.mjs —
//! байт-у-байт той самий формат повідомлення). Механіка віддзеркалює
//! approvals: доменний префікс + NUL-роздільник полів.

use ed25519_dalek::Signer;
use uuid::Uuid;

pub use crate::approvals::ApprovalError;
use crate::approvals::{Signature, SigningKey, VerifyingKey};

/// Доменний префікс canonical-акта transfer — підпис не переноситься
/// в контекст approvals і навпаки.
const DOMAIN: &[u8] = b"mt-transfer-v4";

/// Акт передачі власності кореневої задачі.
#[derive(Debug, Clone, PartialEq)]
pub struct TransferPayload {
    /// Кореневий вузол задачі (кімната relay).
    pub root_node_hash: String,
    /// Поточний owner (акаунт-ініціатор).
    pub from_account: Uuid,
    /// Новий owner (мусить бути учасником задачі).
    pub to_account: Uuid,
}

impl TransferPayload {
    /// Canonical-байти акта: `DOMAIN \0 root \0 from(hyphenated) \0
    /// to(hyphenated)` — дзеркало `transferMessage` у relay (JS).
    pub fn message(&self) -> Vec<u8> {
        let from = self.from_account.hyphenated().to_string();
        let to = self.to_account.hyphenated().to_string();
        let mut message = Vec::with_capacity(
            DOMAIN.len() + self.root_node_hash.len() + from.len() + to.len() + 3,
        );
        message.extend_from_slice(DOMAIN);
        message.push(0);
        message.extend_from_slice(self.root_node_hash.as_bytes());
        message.push(0);
        message.extend_from_slice(from.as_bytes());
        message.push(0);
        message.extend_from_slice(to.as_bytes());
        message
    }
}

/// Підписати акт transfer приватним ключем пристрою owner-а.
/// У WS-кадр `transfer_ownership` підпис їде як base64 від `to_bytes()`.
pub fn sign_transfer(key: &SigningKey, payload: &TransferPayload) -> Signature {
    key.sign(&payload.message())
}

/// Перевірити підпис акта проти pubkey пристрою-ініціатора.
pub fn verify_transfer(
    pubkey: &VerifyingKey,
    payload: &TransferPayload,
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

    fn payload() -> TransferPayload {
        TransferPayload {
            root_node_hash: "r".repeat(20),
            from_account: Uuid::from_u128(1),
            to_account: Uuid::from_u128(2),
        }
    }

    fn key() -> SigningKey {
        SigningKey::from_bytes(&[7u8; 32])
    }

    #[test]
    fn sign_then_verify_ok() {
        let signature = sign_transfer(&key(), &payload());
        assert_eq!(
            verify_transfer(&key().verifying_key(), &payload(), &signature.to_bytes()),
            Ok(())
        );
    }

    /// Підміна отримувача інвалідовує підпис — transfer не «переадресувати».
    #[test]
    fn tampered_recipient_fails() {
        let bytes = sign_transfer(&key(), &payload()).to_bytes();
        let tampered = TransferPayload {
            to_account: Uuid::from_u128(3),
            ..payload()
        };
        assert_eq!(
            verify_transfer(&key().verifying_key(), &tampered, &bytes),
            Err(ApprovalError::VerificationFailed)
        );
    }

    /// Домен відрізняє transfer від approval: підпис approval-повідомлення
    /// тим самим ключем не проходить як transfer.
    #[test]
    fn approval_domain_signature_is_rejected() {
        let approval = crate::approvals::ApprovalPayload {
            request_id: "req".into(),
            approved: true,
            node_hash: payload().root_node_hash,
            run_token: Uuid::from_u128(9),
        };
        let bytes = crate::approvals::sign_approval(&key(), &approval).to_bytes();
        assert_eq!(
            verify_transfer(&key().verifying_key(), &payload(), &bytes),
            Err(ApprovalError::VerificationFailed)
        );
    }

    /// Формат повідомлення — стабільний контракт із relay (signing.mjs):
    /// domain і поля через NUL, uuid — hyphenated lowercase.
    #[test]
    fn message_matches_relay_canonical_format() {
        let expected = format!(
            "mt-transfer-v4\0{}\0{}\0{}",
            "r".repeat(20),
            Uuid::from_u128(1).hyphenated(),
            Uuid::from_u128(2).hyphenated()
        );
        assert_eq!(payload().message(), expected.as_bytes());
    }
}
