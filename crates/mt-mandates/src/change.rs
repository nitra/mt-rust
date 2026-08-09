//! `validate_mandate_change` — підписаний акт зміни `.mt/mandates.yaml`
//! (mandates.md, «Нормативний контракт (M6 фаза 0)» → таблиця схеми,
//! «Зміна `escalates_to` вимагає ПОДВІЙНОГО підпису» — follow-up-правка,
//! реалізована тут одразу).
//!
//! Crypto — той самий патерн, що `agent_protocol::approvals`/`transfers`:
//! доменний префікс + NUL-роздільник полів, `ed25519_dalek::verify_strict`,
//! спільний [`agent_protocol::ApprovalError`] (не дублюємо тип помилки
//! верифікації підпису).
//!
//! **Рішення, не покриті буквою контракту** (діф не є формальним типом у
//! спеці — інтерфейс `validate_mandate_change(old, new, signatures)` із
//! розділу «Обсяг» задачі, конкретніший за одну фразу napi-поверхні):
//! - **`old`/`new` — повні знімки файлу**, не структурований diff; ця
//!   функція сама обчислює, що саме змінилось на рівні owner-мандатів.
//! - **«Делегатор рівня вище»** для мандата = owner, на якого цей мандат
//!   **зараз** (у `old`) вказує через `escalates_to`. Новий мандат (owner
//!   з'явився вперше) не має `old`-запису — делегатором вважається
//!   `escalates_to`-адресат у `new`.
//! - **Розширення/звуження порівнюються по кожній осі окремо**; якщо в
//!   одному diff-і одна вісь звужується, а інша розширюється — весь diff
//!   трактується як розширення (не можна протягнути розширення під
//!   виглядом самопідписаного звуження).
//! - **Видалення мандата** трактується як звуження «до нуля» — самопідпис
//!   владника, що видаляється.
//! - **Підпис перевіряється криптографічно** (Ed25519 проти заявленого
//!   `pubkey`); чи `pubkey` справді належить заявленому `handle`/ролі —
//!   відповідальність викликача (pubkey-кеш relay, поза межами цього
//!   crate — контракт прямо каже «проти pubkey-кешу»).

use ed25519_dalek::Signer;
use sha2::{Digest, Sha256};

use agent_protocol::{ApprovalError, Signature, SigningKey, VerifyingKey};

use crate::parse;
use crate::types::{MandateKind, MandatesFile, Verdict};

/// Доменний префікс canonical-акта зміни мандатів — окремий від approvals і
/// transfer (кожен клас акту має власний домен, agent-protocol conventions).
const DOMAIN: &[u8] = b"mt-mandate-change-v1";

/// Хто підписує акт зміни. Класифікація ключа (людина/модель) — довіра до
/// викликача (relay/pubkey-кеш уже це знає); crate лише криптографічно
/// звіряє підпис і застосовує політику «остання константа» до заявленої
/// ролі.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignerRole {
    Human,
    Model,
}

/// Підписант акта — заявлений handle (owner, від імені якого підписано) і
/// роль ключа.
#[derive(Debug, Clone)]
pub struct MandateSigner {
    pub handle: String,
    pub role: SignerRole,
    pub pubkey: VerifyingKey,
}

/// Один підпис під диф-ом `mandates.yaml`.
#[derive(Debug, Clone)]
pub struct MandateChangeSignature {
    pub signer: MandateSigner,
    pub signature: Vec<u8>,
}

/// Canonical-акт зміни: старе/нове покоління файлу + хеш нового вмісту.
/// Підписант підтверджує саме ЦЮ пару (old_generation, new_generation) і
/// САМЕ ЦЕЙ новий вміст — підміна будь-якого поля інвалідовує підпис
/// (той самий принцип, що `ApprovalPayload`/`TransferPayload`).
#[derive(Debug, Clone, PartialEq)]
pub struct MandateChangePayload {
    pub old_generation: u64,
    pub new_generation: u64,
    pub new_content_hash: [u8; 32],
}

impl MandateChangePayload {
    /// Хеш канонічного JSON нового файлу (детермінований — порядок полів
    /// фіксований оголошенням структур, не HashMap).
    pub fn for_file(old_generation: u64, new: &MandatesFile) -> Self {
        let bytes = serde_json::to_vec(new).expect("MandatesFile серіалізується без помилок");
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        let new_content_hash: [u8; 32] = hasher.finalize().into();
        MandateChangePayload {
            old_generation,
            new_generation: new.generation,
            new_content_hash,
        }
    }

    /// Canonical-байти: `DOMAIN \0 old_generation \0 new_generation \0 hash(hex)`.
    pub fn message(&self) -> Vec<u8> {
        let hash_hex = hex_lower(&self.new_content_hash);
        let mut message = Vec::with_capacity(DOMAIN.len() + hash_hex.len() + 42);
        message.extend_from_slice(DOMAIN);
        message.push(0);
        message.extend_from_slice(self.old_generation.to_string().as_bytes());
        message.push(0);
        message.extend_from_slice(self.new_generation.to_string().as_bytes());
        message.push(0);
        message.extend_from_slice(hash_hex.as_bytes());
        message
    }
}

fn hex_lower(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        write!(s, "{b:02x}").expect("запис у String не падає");
    }
    s
}

/// Підписати акт зміни приватним ключем підписанта.
pub fn sign_mandate_change(key: &SigningKey, payload: &MandateChangePayload) -> Signature {
    key.sign(&payload.message())
}

/// Перевірити підпис акта проти pubkey підписанта (той самий шлях, що
/// `agent_protocol::verify_approval` — `verify_strict`, відхиляє malleable
/// підписи).
pub fn verify_mandate_change_signature(
    pubkey: &VerifyingKey,
    payload: &MandateChangePayload,
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

/// Один з логічних видів зміни, застосованих до одного owner-мандата між
/// `old` і `new`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChangeKind {
    Added,
    Removed,
    KindChanged,
    EscalatesToChanged,
    Widened,
    Narrowed,
    Unchanged,
}

/// `validate_mandate_change(old, new, signatures) → Verdict` — napi-API
/// поверхня контракту.
pub fn validate_mandate_change(
    old: &MandatesFile,
    new: &MandatesFile,
    signatures: &[MandateChangeSignature],
) -> Verdict {
    if new.generation != old.generation + 1 {
        return Verdict::invalid(format!(
            "generation має зрости рівно на 1: старий {}, новий {}",
            old.generation, new.generation
        ));
    }

    if let Err(reason) = parse::validate(new) {
        return Verdict::invalid(format!("новий стан файлу невалідний: {reason}"));
    }

    let payload = MandateChangePayload::for_file(old.generation, new);

    // Кожен елемент `signatures` має бути криптографічно дійсним підписом
    // над цим самим диф-ом — інакше він не рахується жодним із required
    // підписів нижче (fail-closed: непідтверджений підпис ігнорується, а
    // не приймається «за замовчуванням валідний»).
    let mut verified_handles_human: Vec<&str> = Vec::new();
    let mut verified_handles_any: Vec<&str> = Vec::new();
    for sig in signatures {
        if verify_mandate_change_signature(&sig.signer.pubkey, &payload, &sig.signature).is_ok() {
            verified_handles_any.push(sig.signer.handle.as_str());
            if sig.signer.role == SignerRole::Human {
                verified_handles_human.push(sig.signer.handle.as_str());
            }
        }
    }

    for owner in all_owners(old, new) {
        let old_mandate = old.mandates.iter().find(|m| m.owner == owner);
        let new_mandate = new.mandates.iter().find(|m| m.owner == owner);

        let kind = classify(old_mandate, new_mandate);
        if kind == ChangeKind::Unchanged {
            continue;
        }

        let delegator_above = match (old_mandate, new_mandate) {
            (Some(m), _) => m.escalates_to.as_deref(),
            (None, Some(m)) => m.escalates_to.as_deref(),
            (None, None) => None,
        };

        let is_model_mandate = old_mandate
            .map(|m| m.kind == MandateKind::Model)
            .or_else(|| new_mandate.map(|m| m.kind == MandateKind::Model))
            .unwrap_or(false);

        match kind {
            ChangeKind::EscalatesToChanged => {
                let new_addressee = new_mandate
                    .expect("EscalatesToChanged означає new_mandate існує")
                    .escalates_to
                    .as_deref();
                let old_delegator = old_mandate.and_then(|m| m.escalates_to.as_deref());

                let addressee_signed = new_addressee
                    .map(|a| verified_handles_any.contains(&a))
                    .unwrap_or(true); // нова адреса — корінь (null), приймати нема кого
                let delegator_signed = old_delegator
                    .map(|d| verified_handles_any.contains(&d))
                    .unwrap_or(false);

                if !addressee_signed || !delegator_signed {
                    return Verdict::invalid(format!(
                        "owner '{owner}': зміна escalates_to вимагає ПОДВІЙНОГО підпису — \
                         новий адресат ескалації і делегатор рівня вище (поточний escalates_to)"
                    ));
                }
            }
            ChangeKind::Widened | ChangeKind::Added => {
                if is_model_mandate {
                    // «Остання константа»: розширення ШІ-мандата підписує
                    // ЛИШЕ людина, безумовно — навіть якщо делегатор вище
                    // теж підписав модельним ключем.
                    if verified_handles_human.is_empty() {
                        return Verdict::invalid(format!(
                            "owner '{owner}': розширення ШІ-мандата (kind: model) підписує \
                             лише людський ключ — модельний підпис відхиляється безумовно"
                        ));
                    }
                }
                let Some(delegator) = delegator_above else {
                    return Verdict::invalid(format!(
                        "owner '{owner}': розширення без делегатора рівня вище (немає escalates_to)"
                    ));
                };
                if !verified_handles_any.contains(&delegator) {
                    return Verdict::invalid(format!(
                        "owner '{owner}': розширення scope/thresholds вимагає підпису \
                         делегатора рівня вище ('{delegator}')"
                    ));
                }
            }
            ChangeKind::Narrowed | ChangeKind::Removed => {
                if !verified_handles_any.contains(&owner.as_str()) {
                    return Verdict::invalid(format!(
                        "owner '{owner}': звуження/видалення мандата вимагає самопідпису owner"
                    ));
                }
            }
            ChangeKind::KindChanged => {
                if verified_handles_human.is_empty()
                    || !verified_handles_human.contains(&owner.as_str())
                {
                    return Verdict::invalid(format!(
                        "owner '{owner}': зміна kind (person↔model) — delete+create, \
                         в обох напрямках підписує людина (owner)"
                    ));
                }
            }
            ChangeKind::Unchanged => unreachable!(),
        }
    }

    Verdict::Valid
}

fn all_owners(old: &MandatesFile, new: &MandatesFile) -> Vec<String> {
    let mut owners: Vec<String> = old
        .mandates
        .iter()
        .map(|m| m.owner.clone())
        .chain(new.mandates.iter().map(|m| m.owner.clone()))
        .collect();
    owners.sort();
    owners.dedup();
    owners
}

fn classify(
    old: Option<&crate::types::Mandate>,
    new: Option<&crate::types::Mandate>,
) -> ChangeKind {
    match (old, new) {
        (None, Some(_)) => ChangeKind::Added,
        (Some(_), None) => ChangeKind::Removed,
        (None, None) => ChangeKind::Unchanged,
        (Some(o), Some(n)) => {
            if o.kind != n.kind {
                return ChangeKind::KindChanged;
            }
            if o.escalates_to != n.escalates_to {
                return ChangeKind::EscalatesToChanged;
            }
            let widened = scope_widened(o, n) || thresholds_widened(o, n);
            let narrowed = scope_narrowed(o, n) || thresholds_narrowed(o, n);
            match (widened, narrowed) {
                (false, false) => ChangeKind::Unchanged,
                (true, _) => ChangeKind::Widened,
                (false, true) => ChangeKind::Narrowed,
            }
        }
    }
}

fn scope_widened(old: &crate::types::Mandate, new: &crate::types::Mandate) -> bool {
    set_widened(&old.scope.refs, &new.scope.refs)
        || decision_types_widened(&old.scope.decision_types, &new.scope.decision_types)
}

fn scope_narrowed(old: &crate::types::Mandate, new: &crate::types::Mandate) -> bool {
    set_widened(&new.scope.refs, &old.scope.refs)
        || decision_types_widened(&new.scope.decision_types, &old.scope.decision_types)
}

fn set_widened(old: &[String], new: &[String]) -> bool {
    new.iter().any(|v| !old.contains(v))
}

fn decision_types_widened(old: &[String], new: &[String]) -> bool {
    if old.iter().any(|t| t == "*") {
        return false; // старий scope вже покриває все — розширити нікуди
    }
    if new.iter().any(|t| t == "*") {
        return true;
    }
    set_widened(old, new)
}

fn thresholds_widened(old: &crate::types::Mandate, new: &crate::types::Mandate) -> bool {
    let o = &old.thresholds;
    let n = &new.thresholds;
    numeric_widened(o.budget_eur, n.budget_eur)
        || ord_widened(o.risk, n.risk)
        || (!o.irreversible_or_default() && n.irreversible_or_default())
        || (old.kind == MandateKind::Model
            && ord_widened(Some(o.audacity_or_default()), Some(n.audacity_or_default())))
}

fn thresholds_narrowed(old: &crate::types::Mandate, new: &crate::types::Mandate) -> bool {
    let o = &old.thresholds;
    let n = &new.thresholds;
    numeric_widened(n.budget_eur, o.budget_eur)
        || ord_widened(n.risk, o.risk)
        || (o.irreversible_or_default() && !n.irreversible_or_default())
        || (old.kind == MandateKind::Model
            && ord_widened(Some(n.audacity_or_default()), Some(o.audacity_or_default())))
}

/// `None` = без стелі (найширше значення). Розширено, якщо стеля піднялась
/// або зникла зовсім.
fn numeric_widened(old: Option<f64>, new: Option<f64>) -> bool {
    match (old, new) {
        (Some(_), None) => true,
        (None, _) => false,
        (Some(o), Some(n)) => n > o,
    }
}

fn ord_widened<T: PartialOrd>(old: Option<T>, new: Option<T>) -> bool {
    match (old, new) {
        (Some(_), None) => true,
        (None, _) => false,
        (Some(o), Some(n)) => n > o,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Mandate, MandateKind, RiskLevel, Scope, Thresholds};

    fn key(seed: u8) -> SigningKey {
        SigningKey::from_bytes(&[seed; 32])
    }

    fn signer(handle: &str, role: SignerRole, seed: u8) -> MandateSigner {
        MandateSigner {
            handle: handle.to_string(),
            role,
            pubkey: key(seed).verifying_key(),
        }
    }

    fn sign_for(new: &MandatesFile, old_generation: u64, seed: u8) -> Vec<u8> {
        let payload = MandateChangePayload::for_file(old_generation, new);
        sign_mandate_change(&key(seed), &payload)
            .to_bytes()
            .to_vec()
    }

    fn base_file(generation: u64) -> MandatesFile {
        MandatesFile {
            generation,
            mandates: vec![
                Mandate {
                    owner: "olena".into(),
                    kind: MandateKind::Person,
                    scope: Scope {
                        refs: vec!["refs/mt/tasks/design/**".into()],
                        decision_types: vec!["architecture".into()],
                    },
                    thresholds: Thresholds {
                        budget_eur: Some(2000.0),
                        risk: Some(RiskLevel::Medium),
                        irreversible: Some(false),
                        audacity: None,
                    },
                    escalates_to: Some("vitalii".into()),
                },
                Mandate {
                    owner: "vitalii".into(),
                    kind: MandateKind::Person,
                    scope: Scope {
                        refs: vec!["refs/mt/**".into()],
                        decision_types: vec!["*".into()],
                    },
                    thresholds: Thresholds::default(),
                    escalates_to: None,
                },
            ],
        }
    }

    #[test]
    fn generation_must_increase_by_exactly_one() {
        let old = base_file(3);
        let mut new = base_file(3);
        new.generation = 5;
        let verdict = validate_mandate_change(&old, &new, &[]);
        assert!(matches!(verdict, Verdict::Invalid(r) if r.contains("generation")));
    }

    #[test]
    fn narrowing_without_self_signature_is_invalid() {
        let old = base_file(1);
        let mut new = base_file(2);
        new.mandates[0].thresholds.budget_eur = Some(1000.0); // звуження
        let verdict = validate_mandate_change(&old, &new, &[]);
        assert!(matches!(verdict, Verdict::Invalid(r) if r.contains("самопідпис")));
    }

    #[test]
    fn narrowing_with_self_signature_is_valid() {
        let old = base_file(1);
        let mut new = base_file(2);
        new.mandates[0].thresholds.budget_eur = Some(1000.0);
        let sig = sign_for(&new, 1, 42);
        let verdict = validate_mandate_change(
            &old,
            &new,
            &[MandateChangeSignature {
                signer: signer("olena", SignerRole::Human, 42),
                signature: sig,
            }],
        );
        assert_eq!(verdict, Verdict::Valid);
    }

    #[test]
    fn widening_without_delegator_signature_is_invalid() {
        let old = base_file(1);
        let mut new = base_file(2);
        new.mandates[0].thresholds.budget_eur = Some(3000.0); // розширення
        let sig = sign_for(&new, 1, 42); // підписав сам olena, не делегатор
        let verdict = validate_mandate_change(
            &old,
            &new,
            &[MandateChangeSignature {
                signer: signer("olena", SignerRole::Human, 42),
                signature: sig,
            }],
        );
        assert!(matches!(verdict, Verdict::Invalid(r) if r.contains("делегатора")));
    }

    #[test]
    fn widening_with_delegator_signature_is_valid() {
        let old = base_file(1);
        let mut new = base_file(2);
        new.mandates[0].thresholds.budget_eur = Some(3000.0);
        let sig = sign_for(&new, 1, 7); // vitalii — делегатор olena
        let verdict = validate_mandate_change(
            &old,
            &new,
            &[MandateChangeSignature {
                signer: signer("vitalii", SignerRole::Human, 7),
                signature: sig,
            }],
        );
        assert_eq!(verdict, Verdict::Valid);
    }

    #[test]
    fn model_mandate_widening_rejects_model_signature_even_from_delegator() {
        let mut old = base_file(1);
        old.mandates.push(Mandate {
            owner: "fable-5".into(),
            kind: MandateKind::Model,
            scope: Scope {
                refs: vec!["refs/mt/tasks/routine/**".into()],
                decision_types: vec!["ops".into()],
            },
            thresholds: Thresholds {
                budget_eur: Some(200.0),
                risk: Some(RiskLevel::Low),
                irreversible: Some(false),
                audacity: Some(crate::types::AudacityLevel::Low),
            },
            escalates_to: Some("olena".into()),
        });
        let mut new = old.clone();
        new.generation = 2;
        new.mandates[2].thresholds.audacity = Some(crate::types::AudacityLevel::High);

        let sig = sign_for(&new, 1, 99);
        // "olena" — делегатор fable-5, але підписує МОДЕЛЬНИМ ключем.
        let verdict = validate_mandate_change(
            &old,
            &new,
            &[MandateChangeSignature {
                signer: signer("olena", SignerRole::Model, 99),
                signature: sig,
            }],
        );
        assert!(matches!(verdict, Verdict::Invalid(r) if r.contains("людський")));
    }

    #[test]
    fn model_mandate_widening_with_human_delegator_signature_is_valid() {
        let mut old = base_file(1);
        old.mandates.push(Mandate {
            owner: "fable-5".into(),
            kind: MandateKind::Model,
            scope: Scope {
                refs: vec!["refs/mt/tasks/routine/**".into()],
                decision_types: vec!["ops".into()],
            },
            thresholds: Thresholds {
                budget_eur: Some(200.0),
                risk: Some(RiskLevel::Low),
                irreversible: Some(false),
                audacity: Some(crate::types::AudacityLevel::Low),
            },
            escalates_to: Some("olena".into()),
        });
        let mut new = old.clone();
        new.generation = 2;
        new.mandates[2].thresholds.audacity = Some(crate::types::AudacityLevel::High);

        let sig = sign_for(&new, 1, 99);
        let verdict = validate_mandate_change(
            &old,
            &new,
            &[MandateChangeSignature {
                signer: signer("olena", SignerRole::Human, 99),
                signature: sig,
            }],
        );
        assert_eq!(verdict, Verdict::Valid);
    }

    #[test]
    fn escalates_to_change_requires_double_signature() {
        let mut old = base_file(1);
        old.mandates.push(Mandate {
            owner: "petro".into(),
            kind: MandateKind::Person,
            scope: Scope {
                refs: vec!["refs/mt/tasks/ops/**".into()],
                decision_types: vec!["ops".into()],
            },
            thresholds: Thresholds::default(),
            escalates_to: Some("olena".into()),
        });
        let mut new = old.clone();
        new.generation = 2;
        new.mandates[2].escalates_to = Some("vitalii".into()); // petro тепер під vitalii

        // Лише новий адресат підписав — делегатора (olena) немає.
        let sig = sign_for(&new, 1, 7);
        let verdict = validate_mandate_change(
            &old,
            &new,
            &[MandateChangeSignature {
                signer: signer("vitalii", SignerRole::Human, 7),
                signature: sig.clone(),
            }],
        );
        assert!(matches!(verdict, Verdict::Invalid(r) if r.contains("ПОДВІЙНОГО")));

        // Обидва підписали — валідно.
        let sig_olena = sign_for(&new, 1, 55);
        let verdict = validate_mandate_change(
            &old,
            &new,
            &[
                MandateChangeSignature {
                    signer: signer("vitalii", SignerRole::Human, 7),
                    signature: sig,
                },
                MandateChangeSignature {
                    signer: signer("olena", SignerRole::Human, 55),
                    signature: sig_olena,
                },
            ],
        );
        assert_eq!(verdict, Verdict::Valid);
    }

    #[test]
    fn invalid_signature_bytes_do_not_count() {
        let old = base_file(1);
        let mut new = base_file(2);
        new.mandates[0].thresholds.budget_eur = Some(1000.0);
        let verdict = validate_mandate_change(
            &old,
            &new,
            &[MandateChangeSignature {
                signer: signer("olena", SignerRole::Human, 42),
                signature: vec![0u8; 64], // випадкові байти, не дійсний підпис
            }],
        );
        assert!(matches!(verdict, Verdict::Invalid(_)));
    }

    #[test]
    fn new_file_failing_structural_validation_is_invalid() {
        let old = base_file(1);
        let mut new = base_file(2);
        new.mandates[1].escalates_to = Some("olena".into()); // ламає корінь
        let verdict = validate_mandate_change(&old, &new, &[]);
        assert!(matches!(verdict, Verdict::Invalid(r) if r.contains("невалідний")));
    }
}
