//! Гейт approvals хоста (спека access.md, «Approvals: три гейти, один
//! механізм»): хост шле `ApprovalRequest` у кімнату → пристрій учасника
//! approver+ підписує `(request_id, approved, node_hash, run_token)` →
//! хост звіряє підпис із pubkey-кешем relay; підпис поза списком → відмова.
//!
//! Кеш наповнює relay-міст (`pubkeys`-кадр); разом із ним вмикається
//! `require_signed`. Без relay (локальний dev) порожній підпис приймається —
//! довіра локальному транспорту з discovery-токеном. Матеріалізація у
//! `## Approvals` файлів вузла — окрема задача (потребує синтезу
//! `run_NNN.md` в інтерактивному done).

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use agent_protocol::{verify_approval, ApprovalPayload, VerifyingKey};
use tokio::sync::oneshot;
use uuid::Uuid;

/// Очікуваний approval: адресація для канонічного повідомлення підпису.
struct PendingApproval {
    node_hash: String,
    run_token: Uuid,
    sender: oneshot::Sender<bool>,
}

/// Стан гейту: pending-запити + pubkey-кеш пристроїв approver+.
#[derive(Default)]
pub struct ApprovalGate {
    pending: Mutex<HashMap<String, PendingApproval>>,
    pubkeys: Mutex<HashMap<Uuid, VerifyingKey>>,
    /// Вмикається разом із pubkey-кешем (relay-міст): підпис обовʼязковий.
    require_signed: AtomicBool,
}

impl ApprovalGate {
    /// Реєструє pending-запит; емісію `ApprovalRequest` у сесію робить
    /// викликач. Повертає one-shot із вердиктом (true = approved).
    pub fn register(
        &self,
        request_id: &str,
        node_hash: &str,
        run_token: Uuid,
    ) -> oneshot::Receiver<bool> {
        let (sender, receiver) = oneshot::channel();
        self.pending.lock().unwrap().insert(
            request_id.to_string(),
            PendingApproval {
                node_hash: node_hash.to_string(),
                run_token,
                sender,
            },
        );
        receiver
    }

    /// Оновлює pubkey-кеш (кадр `pubkeys` від relay) і вмикає
    /// обовʼязковість підпису.
    pub fn set_pubkeys(&self, keys: Vec<(Uuid, VerifyingKey)>) {
        let mut pubkeys = self.pubkeys.lock().unwrap();
        pubkeys.clear();
        pubkeys.extend(keys);
        self.require_signed.store(true, Ordering::Relaxed);
    }

    /// Обробляє `ApprovalResponse`. Успіх → pending завершується вердиктом.
    /// Помилка (невідомий request_id / підпис поза списком / зіпсований) →
    /// `Err` із поясненням; pending ЛИШАЄТЬСЯ — інший пристрій може
    /// відповісти валідним підписом.
    pub fn resolve(
        &self,
        request_id: &str,
        approved: bool,
        signature: &[u8],
        device_id: Option<Uuid>,
    ) -> Result<bool, String> {
        let mut pending = self.pending.lock().unwrap();
        let entry = pending
            .get(request_id)
            .ok_or_else(|| format!("approval: невідомий request_id {request_id}"))?;

        if signature.is_empty() {
            if self.require_signed.load(Ordering::Relaxed) {
                return Err("approval: підпис обовʼязковий (require_signed)".into());
            }
        } else {
            let device_id =
                device_id.ok_or_else(|| "approval: підпис без device_id".to_string())?;
            let pubkeys = self.pubkeys.lock().unwrap();
            let key = pubkeys.get(&device_id).ok_or_else(|| {
                format!("approval: пристрій {device_id} поза pubkey-списком — відмова")
            })?;
            let payload = ApprovalPayload {
                request_id: request_id.to_string(),
                approved,
                node_hash: entry.node_hash.clone(),
                run_token: entry.run_token,
            };
            verify_approval(key, &payload, signature)
                .map_err(|error| format!("approval: підпис не пройшов перевірку — {error}"))?;
        }

        let entry = pending.remove(request_id).expect("щойно перевірено");
        let _ = entry.sender.send(approved);
        Ok(approved)
    }
}

/// Mid-run approval-запит (access.md, перший гейт): публікує
/// `ApprovalRequest` у кімнату вузла і повертає one-shot із верифікованим
/// вердиктом. Вільна функція — щоб runner-фабрика могла гейтити тули без
/// циклу залежностей із AppState.
pub fn request_approval(
    sessions: &crate::session::SessionHost,
    gate: &ApprovalGate,
    node: &str,
    action: String,
    diff: Option<String>,
) -> std::io::Result<oneshot::Receiver<bool>> {
    let session = sessions.get_or_open(node)?;
    let request_id = Uuid::new_v4().to_string();
    let receiver = gate.register(&request_id, node, session.run_token);
    sessions.publish(
        &session,
        agent_protocol::Event::ApprovalRequest {
            request_id,
            action,
            diff,
        },
        None,
        None,
    );
    Ok(receiver)
}

#[cfg(test)]
mod tests {
    use agent_protocol::{sign_approval, SigningKey};

    use super::*;

    fn key() -> SigningKey {
        SigningKey::from_bytes(&[3u8; 32])
    }

    fn signed(
        gate: &ApprovalGate,
        request_id: &str,
        approved: bool,
        signer: &SigningKey,
    ) -> Vec<u8> {
        let _ = gate; // підпис не залежить від гейту — лише від payload
        sign_approval(
            signer,
            &ApprovalPayload {
                request_id: request_id.to_string(),
                approved,
                node_hash: "room-1".into(),
                run_token: Uuid::from_u128(9),
            },
        )
        .to_bytes()
        .to_vec()
    }

    /// Валідний підпис пристрою з кешу завершує pending вердиктом.
    #[tokio::test]
    async fn valid_signature_resolves_pending() {
        let gate = ApprovalGate::default();
        let device = Uuid::from_u128(1);
        gate.set_pubkeys(vec![(device, key().verifying_key())]);
        let receiver = gate.register("req-1", "room-1", Uuid::from_u128(9));

        let signature = signed(&gate, "req-1", true, &key());
        assert_eq!(
            gate.resolve("req-1", true, &signature, Some(device)),
            Ok(true)
        );
        assert_eq!(receiver.await, Ok(true));
    }

    /// Чужий ключ (поза pubkey-списком) і зіпсований підпис — відмова;
    /// pending лишається і приймає наступну валідну відповідь.
    #[tokio::test]
    async fn invalid_signatures_are_rejected_but_pending_survives() {
        let gate = ApprovalGate::default();
        let device = Uuid::from_u128(1);
        gate.set_pubkeys(vec![(device, key().verifying_key())]);
        let receiver = gate.register("req-1", "room-1", Uuid::from_u128(9));

        // Невідомий пристрій.
        let foreign = signed(&gate, "req-1", true, &SigningKey::from_bytes(&[7u8; 32]));
        let error = gate
            .resolve("req-1", true, &foreign, Some(Uuid::from_u128(2)))
            .unwrap_err();
        assert!(error.contains("поза pubkey-списком"), "{error}");

        // Зіпсований підпис відомого пристрою.
        let mut corrupted = signed(&gate, "req-1", true, &key());
        corrupted[5] ^= 0xFF;
        let error = gate
            .resolve("req-1", true, &corrupted, Some(device))
            .unwrap_err();
        assert!(error.contains("не пройшов"), "{error}");

        // Валідна відповідь після відмов досі можлива.
        let signature = signed(&gate, "req-1", false, &key());
        assert_eq!(
            gate.resolve("req-1", false, &signature, Some(device)),
            Ok(false)
        );
        assert_eq!(receiver.await, Ok(false));
    }

    /// Політики непідписаної відповіді: dev (без relay) приймає, із
    /// pubkey-кешем — відмова.
    #[tokio::test]
    async fn unsigned_policy_depends_on_require_signed() {
        let gate = ApprovalGate::default();
        let receiver = gate.register("req-1", "room-1", Uuid::from_u128(9));
        assert_eq!(gate.resolve("req-1", true, &[], None), Ok(true));
        assert_eq!(receiver.await, Ok(true));

        gate.set_pubkeys(vec![]);
        let _receiver = gate.register("req-2", "room-1", Uuid::from_u128(9));
        let error = gate.resolve("req-2", true, &[], None).unwrap_err();
        assert!(error.contains("обовʼязковий"), "{error}");
    }
}
