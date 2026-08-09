//! `mt-mandates` — карта мандатів `.mt/mandates.yaml`, маршрутизатор
//! ескалацій і квіз-гейт (спека `mt`, `docs/architecture/mandates.md`,
//! «Нормативний контракт (M6 фаза 0)»).
//!
//! **Контракт-перший**: цей crate — одна з двох незалежних реалізацій
//! нормативної схеми (друга — застосунок `delta`, репо `task`, мокає за
//! тими самими сигнатурами). Чотири napi-API функції контракту:
//!
//! - [`parse_mandates`]/[`parse_mandates_str`] — розбір + повна валідація
//!   `.mt/mandates.yaml`.
//! - [`effective_owner`] — крок 1 маршрутизатора ескалацій (lookup за
//!   `refs × decision_types × thresholds`).
//! - [`validate_mandate_change`] — вердикт на підписаний diff мандатів
//!   (generation fencing, delegation-підписи, «остання константа» для
//!   ШІ-мандатів, подвійний підпис `escalates_to`).
//! - [`validate_approval`] — квіз-гейт на `ApprovalResponse` гейту
//!   `decision-request`.
//!
//! Ed25519 crypto скрізь реюзає типи й патерни `agent-protocol`
//! (`Signature`/`SigningKey`/`VerifyingKey`, `verify_strict`,
//! доменно-префіксовані NUL-роздільні повідомлення) — не дублює.
//! YAML-документ мандатів — повноцінна вкладена структура (масив об'єктів),
//! тому парситься через `serde_norway` (підтримуваний форк `serde_yaml`),
//! а не через рестрикт-парсер front-matter `mt-core`; квіз-файли — навпаки,
//! плоский frontmatter, і повторно використовують `mt_core::frontmatter`.

pub mod approval;
pub mod change;
pub mod lookup;
pub mod parse;
pub mod types;

pub use approval::{
    depth_for_facets, parse_quiz, validate_approval, ApprovalContext, DecisionApproval, Quiz,
    QuizParseError, SignatureError,
};
pub use change::{
    sign_mandate_change, validate_mandate_change, verify_mandate_change_signature,
    MandateChangePayload, MandateChangeSignature, MandateSigner, SignerRole,
};
pub use lookup::{effective_owner, EffectiveOwner, LookupError};
pub use parse::{parse_mandates, parse_mandates_str, MandatesError};
pub use types::{
    AudacityLevel, BlastRadius, DecisionFacets, Divergence, LeverageFacets, Mandate, MandateKind,
    MandatesFile, QuizDepth, RiskLevel, Scope, Thresholds, Verdict,
};
