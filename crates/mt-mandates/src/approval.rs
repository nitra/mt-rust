//! `validate_approval` — квіз-гейт на `decision-request` (mandates.md,
//! «Нормативний контракт (M6 фаза 0)» → «Формат квіз-файлів»,
//! «Розширення `ApprovalResponse`»).
//!
//! Frontmatter квіз-файлу розбирається через `mt_core::frontmatter`
//! (той самий парсер, що всі інші мт-файли — не дублюємо front-matter
//! парсинг); підпис `ApprovalResponse` перевіряється через
//! `agent_protocol::{ApprovalPayload, verify_approval}` — базовий кортеж
//! `(request_id, approved, node_hash, run_token)` лишається БЕЗ ЗМІН,
//! `chosen_option`/`quiz_ref` — непідписані супровідні поля цього ж
//! JSON-об'єкта (контракт розширює JSON-приклад `ApprovalResponse`, не
//! домен підпису approvals).
//!
//! **Рішення, не покриті буквою контракту:**
//! - **Сигнатура функції.** Napi-поверхня описує `validate_approval(response,
//!   quiz) → Verdict` одним реченням; реально потрібні ще `pubkey` (проти
//!   pubkey-кешу — контракт це прямо називає) і контекст
//!   `decision-request`-вузла (`decision_ref` файлу, `recommended_by`,
//!   `leverage_facets`) — без них «quiz.decision_ref відповідає вузлу» і
//!   «depth — мапінгу з leverage_facets» неможливо перевірити. Тут вони
//!   згорнуті в [`ApprovalContext`], а `response`/`quiz` лишаються двома
//!   головними параметрами, як у контракті.
//! - **Мапінг facets → depth** — точна таблиця живе поза цим фрагментом
//!   контракту («підписана політика … один джерело істини в
//!   mandates.yaml», секція вище нормативного розділу); тут —
//!   детермінована реалізація одного речення тексту специфікації
//!   («reversible + низькі фасети → one-tap; середні фасети й лише
//!   reversible → standard; irreversible або широкий blast_radius →
//!   teach-back»), винесена в [`crate::depth_for_facets`] так, щоб
//!   викликач міг підмінити її власною політикою з `mandates.yaml`, коли
//!   та таблиця матеріалізується.

use agent_protocol::{verify_approval, ApprovalError, ApprovalPayload, VerifyingKey};
use mt_core::frontmatter::parse_front_matter;

use crate::types::{BlastRadius, Divergence, LeverageFacets, QuizDepth, Verdict};

/// Квіз-файл `NNNN-quiz.md` — розібраний frontmatter (тіло документа —
/// питання/відповіді/переказ — не потрібне для валідації гейту).
#[derive(Debug, Clone, PartialEq)]
pub struct Quiz {
    pub schema_version: u64,
    pub decision_ref: String,
    pub depth: QuizDepth,
    /// Ідентифікатор агента, що згенерував квіз — МАЄ відрізнятись від
    /// `decision-request.recommended_by` (конфлікт інтересів у промпті).
    pub generated_by: String,
    /// `None` = квіз ще не зафіксовано (проходження не завершене).
    pub iterations: Option<u64>,
    pub time_to_understanding_sec: Option<u64>,
}

/// Помилка розбору квіз-файлу.
#[derive(Debug, Clone, PartialEq)]
pub enum QuizParseError {
    MissingField(&'static str),
    InvalidField { field: &'static str, value: String },
}

impl std::fmt::Display for QuizParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            QuizParseError::MissingField(name) => write!(f, "квіз: відсутнє поле '{name}'"),
            QuizParseError::InvalidField { field, value } => {
                write!(f, "квіз: поле '{field}' має неочікуване значення '{value}'")
            }
        }
    }
}

impl std::error::Error for QuizParseError {}

/// Розбирає frontmatter квіз-файлу (`schema_version: 1`, `type: quiz`, …).
pub fn parse_quiz(text: &str) -> Result<Quiz, QuizParseError> {
    let fm = parse_front_matter(text);

    let schema_version = fm
        .get("schema_version")
        .and_then(|v| v.as_u64())
        .ok_or(QuizParseError::MissingField("schema_version"))?;

    let decision_ref = fm
        .get("decision_ref")
        .and_then(|v| v.as_str())
        .ok_or(QuizParseError::MissingField("decision_ref"))?
        .to_string();

    let depth_raw = fm
        .get("depth")
        .and_then(|v| v.as_str())
        .ok_or(QuizParseError::MissingField("depth"))?;
    let depth = match depth_raw {
        "one-tap" => QuizDepth::OneTap,
        "standard" => QuizDepth::Standard,
        "teach-back" => QuizDepth::TeachBack,
        other => {
            return Err(QuizParseError::InvalidField {
                field: "depth",
                value: other.to_string(),
            })
        }
    };

    let generated_by = fm
        .get("generated_by")
        .and_then(|v| v.as_str())
        .ok_or(QuizParseError::MissingField("generated_by"))?
        .to_string();

    let iterations = fm.get("iterations").and_then(|v| v.as_u64());
    let time_to_understanding_sec = fm.get("time_to_understanding_sec").and_then(|v| v.as_u64());

    Ok(Quiz {
        schema_version,
        decision_ref,
        depth,
        generated_by,
        iterations,
        time_to_understanding_sec,
    })
}

/// Квіз завершений: обидва поля фіксації присутні (контракт: «мутабельний
/// до моменту, коли `iterations` фіксується підписаним `ApprovalResponse`»).
impl Quiz {
    pub fn is_complete(&self) -> bool {
        self.iterations.is_some() && self.time_to_understanding_sec.is_some()
    }
}

/// `ApprovalResponse` на гейті `decision-request` — базовий підписаний
/// кортеж [`ApprovalPayload`] + `chosen_option`/`quiz_ref` (JSON-приклад
/// контракту, «Розширення `ApprovalResponse`»).
#[derive(Debug, Clone, PartialEq)]
pub struct DecisionApproval {
    pub payload: ApprovalPayload,
    pub signature: Vec<u8>,
    pub chosen_option: Option<String>,
    pub quiz_ref: Option<String>,
}

/// Контекст `decision-request`-вузла, потрібний для перевірок, які
/// napi-речення контракту називає, але не вміщує у два позиційні параметри.
#[derive(Debug, Clone, PartialEq)]
pub struct ApprovalContext {
    /// Ім'я файлу квіза, очікуване для цього `decision-request`
    /// (`decisions/NNNN-quiz.md`) — звіряється з `quiz.decision_ref`
    /// навпаки: контракт каже «quiz.decision_ref відповідає» решті пари,
    /// тобто самому `decision-request`-файлу, не `response.request_id`.
    pub decision_request_ref: String,
    pub recommended_by: String,
    pub leverage_facets: LeverageFacets,
}

/// `validate_approval(response, quiz) → Verdict` — napi-API поверхня
/// контракту. `pubkey`/`ctx` — див. module doc, «Рішення …».
pub fn validate_approval(
    response: &DecisionApproval,
    quiz: Option<&Quiz>,
    pubkey: &VerifyingKey,
    ctx: &ApprovalContext,
) -> Verdict {
    if let Err(e) = verify_approval(pubkey, &response.payload, &response.signature) {
        return Verdict::invalid(format!("підпис ApprovalResponse невалідний: {e}"));
    }

    let Some(quiz_ref) = response.quiz_ref.as_deref() else {
        return Verdict::invalid(
            "decision-request-гейт вимагає quiz_ref — ApprovalResponse без нього невалідний",
        );
    };

    let Some(quiz) = quiz else {
        return Verdict::invalid(format!(
            "quiz_ref '{quiz_ref}' вказує на квіз, який не вдалося завантажити"
        ));
    };

    if !quiz.is_complete() {
        return Verdict::invalid(
            "квіз не завершений — немає зафіксованих iterations/time_to_understanding_sec",
        );
    }

    if quiz.decision_ref != ctx.decision_request_ref {
        return Verdict::invalid(format!(
            "quiz.decision_ref '{}' не відповідає decision-request '{}'",
            quiz.decision_ref, ctx.decision_request_ref
        ));
    }

    if quiz.generated_by == ctx.recommended_by {
        return Verdict::invalid(
            "quiz.generated_by збігається з decision-request.recommended_by — \
             конфлікт інтересів (квіз має генерувати ІНШИЙ агент)",
        );
    }

    let expected_depth = depth_for_facets(&ctx.leverage_facets);
    if quiz.depth != expected_depth {
        return Verdict::invalid(format!(
            "quiz.depth {:?} не відповідає мапінгу leverage_facets (очікувано {:?})",
            quiz.depth, expected_depth
        ));
    }

    Verdict::Valid
}

/// Мапінг leverage-фасетів на глибину квізу — див. module doc, «Рішення …
/// Мапінг facets → depth».
pub fn depth_for_facets(facets: &LeverageFacets) -> QuizDepth {
    if facets.irreversible
        || matches!(
            facets.blast_radius,
            BlastRadius::Repo | BlastRadius::Company
        )
    {
        return QuizDepth::TeachBack;
    }
    if facets.divergence == Divergence::High || facets.blast_radius == BlastRadius::Subtree {
        return QuizDepth::Standard;
    }
    QuizDepth::OneTap
}

/// Помилка криптографічної перевірки — реекспорт [`ApprovalError`] для
/// зручності викликачів, що не хочуть тягнути `agent-protocol` напряму.
pub type SignatureError = ApprovalError;

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    use uuid::Uuid;

    const QUIZ_TEXT: &str = r#"---
schema_version: 1
type: quiz
decision_ref: 0001-decision-request.md
depth: one-tap
generated_by: quiz-gen-fable-5
time_to_understanding_sec: 47
iterations: 1
---

## Питання 1
Що станеться з бюджетом задачі, якщо обереш варіант B?
"#;

    #[test]
    fn parses_quiz_frontmatter() {
        let quiz = parse_quiz(QUIZ_TEXT).expect("valid quiz");
        assert_eq!(quiz.schema_version, 1);
        assert_eq!(quiz.decision_ref, "0001-decision-request.md");
        assert_eq!(quiz.depth, QuizDepth::OneTap);
        assert_eq!(quiz.generated_by, "quiz-gen-fable-5");
        assert_eq!(quiz.iterations, Some(1));
        assert_eq!(quiz.time_to_understanding_sec, Some(47));
        assert!(quiz.is_complete());
    }

    #[test]
    fn incomplete_quiz_missing_iterations() {
        let text = QUIZ_TEXT.replace("iterations: 1\n", "");
        let quiz = parse_quiz(&text).expect("still parses");
        assert!(!quiz.is_complete());
    }

    #[test]
    fn missing_required_field_is_error() {
        let text = QUIZ_TEXT.replace("decision_ref: 0001-decision-request.md\n", "");
        let err = parse_quiz(&text).unwrap_err();
        assert_eq!(err, QuizParseError::MissingField("decision_ref"));
    }

    #[test]
    fn invalid_depth_value_is_error() {
        let text = QUIZ_TEXT.replace("depth: one-tap", "depth: extreme");
        let err = parse_quiz(&text).unwrap_err();
        assert!(matches!(
            err,
            QuizParseError::InvalidField { field: "depth", .. }
        ));
    }

    fn signer_key() -> SigningKey {
        SigningKey::from_bytes(&[3u8; 32])
    }

    fn payload() -> ApprovalPayload {
        ApprovalPayload {
            request_id: "req-1".into(),
            approved: true,
            node_hash: "n".repeat(20),
            run_token: Uuid::from_u128(1),
        }
    }

    fn signed_response(quiz_ref: Option<&str>, chosen_option: Option<&str>) -> DecisionApproval {
        let sig = agent_protocol::sign_approval(&signer_key(), &payload());
        DecisionApproval {
            payload: payload(),
            signature: sig.to_bytes().to_vec(),
            chosen_option: chosen_option.map(str::to_string),
            quiz_ref: quiz_ref.map(str::to_string),
        }
    }

    fn one_tap_ctx() -> ApprovalContext {
        ApprovalContext {
            decision_request_ref: "0001-decision-request.md".into(),
            recommended_by: "escalation-intake-fable-5".into(),
            leverage_facets: LeverageFacets {
                irreversible: false,
                blast_radius: BlastRadius::Node,
                divergence: Divergence::Low,
                est_cost_eur: None,
            },
        }
    }

    #[test]
    fn missing_quiz_ref_is_invalid() {
        let response = signed_response(None, Some("B"));
        let pubkey = signer_key().verifying_key();
        let verdict = validate_approval(&response, None, &pubkey, &one_tap_ctx());
        assert!(matches!(verdict, Verdict::Invalid(r) if r.contains("quiz_ref")));
    }

    #[test]
    fn incomplete_quiz_is_invalid() {
        let response = signed_response(Some("0001-quiz.md"), Some("B"));
        let quiz = Quiz {
            schema_version: 1,
            decision_ref: "0001-decision-request.md".into(),
            depth: QuizDepth::OneTap,
            generated_by: "quiz-gen-fable-5".into(),
            iterations: None,
            time_to_understanding_sec: None,
        };
        let pubkey = signer_key().verifying_key();
        let verdict = validate_approval(&response, Some(&quiz), &pubkey, &one_tap_ctx());
        assert!(matches!(verdict, Verdict::Invalid(r) if r.contains("не завершений")));
    }

    #[test]
    fn generated_by_equal_to_recommended_by_is_invalid() {
        let response = signed_response(Some("0001-quiz.md"), Some("B"));
        let mut ctx = one_tap_ctx();
        ctx.recommended_by = "quiz-gen-fable-5".into();
        let quiz = Quiz {
            schema_version: 1,
            decision_ref: "0001-decision-request.md".into(),
            depth: QuizDepth::OneTap,
            generated_by: "quiz-gen-fable-5".into(),
            iterations: Some(1),
            time_to_understanding_sec: Some(47),
        };
        let pubkey = signer_key().verifying_key();
        let verdict = validate_approval(&response, Some(&quiz), &pubkey, &ctx);
        assert!(matches!(verdict, Verdict::Invalid(r) if r.contains("конфлікт інтересів")));
    }

    #[test]
    fn decision_ref_mismatch_is_invalid() {
        let response = signed_response(Some("0001-quiz.md"), Some("B"));
        let quiz = Quiz {
            schema_version: 1,
            decision_ref: "0002-decision-request.md".into(),
            depth: QuizDepth::OneTap,
            generated_by: "quiz-gen-fable-5".into(),
            iterations: Some(1),
            time_to_understanding_sec: Some(47),
        };
        let pubkey = signer_key().verifying_key();
        let verdict = validate_approval(&response, Some(&quiz), &pubkey, &one_tap_ctx());
        assert!(matches!(verdict, Verdict::Invalid(r) if r.contains("decision_ref")));
    }

    #[test]
    fn depth_mismatch_is_invalid() {
        let response = signed_response(Some("0001-quiz.md"), Some("B"));
        let quiz = Quiz {
            schema_version: 1,
            decision_ref: "0001-decision-request.md".into(),
            depth: QuizDepth::TeachBack, // очікувано one-tap для low-facets ctx
            generated_by: "quiz-gen-fable-5".into(),
            iterations: Some(1),
            time_to_understanding_sec: Some(47),
        };
        let pubkey = signer_key().verifying_key();
        let verdict = validate_approval(&response, Some(&quiz), &pubkey, &one_tap_ctx());
        assert!(matches!(verdict, Verdict::Invalid(r) if r.contains("depth")));
    }

    #[test]
    fn fully_valid_approval_is_valid() {
        let response = signed_response(Some("0001-quiz.md"), Some("B"));
        let quiz = Quiz {
            schema_version: 1,
            decision_ref: "0001-decision-request.md".into(),
            depth: QuizDepth::OneTap,
            generated_by: "quiz-gen-fable-5".into(),
            iterations: Some(1),
            time_to_understanding_sec: Some(47),
        };
        let pubkey = signer_key().verifying_key();
        let verdict = validate_approval(&response, Some(&quiz), &pubkey, &one_tap_ctx());
        assert_eq!(verdict, Verdict::Valid);
    }

    #[test]
    fn tampered_signature_is_invalid() {
        let mut response = signed_response(Some("0001-quiz.md"), Some("B"));
        response.signature[0] ^= 0xFF;
        let quiz = Quiz {
            schema_version: 1,
            decision_ref: "0001-decision-request.md".into(),
            depth: QuizDepth::OneTap,
            generated_by: "quiz-gen-fable-5".into(),
            iterations: Some(1),
            time_to_understanding_sec: Some(47),
        };
        let pubkey = signer_key().verifying_key();
        let verdict = validate_approval(&response, Some(&quiz), &pubkey, &one_tap_ctx());
        assert!(matches!(verdict, Verdict::Invalid(r) if r.contains("підпис")));
    }

    #[test]
    fn depth_mapping_matches_prose_table() {
        let low = LeverageFacets {
            irreversible: false,
            blast_radius: BlastRadius::Node,
            divergence: Divergence::Low,
            est_cost_eur: None,
        };
        assert_eq!(depth_for_facets(&low), QuizDepth::OneTap);

        let medium = LeverageFacets {
            irreversible: false,
            blast_radius: BlastRadius::Subtree,
            divergence: Divergence::Low,
            est_cost_eur: None,
        };
        assert_eq!(depth_for_facets(&medium), QuizDepth::Standard);

        let irreversible = LeverageFacets {
            irreversible: true,
            blast_radius: BlastRadius::Node,
            divergence: Divergence::Low,
            est_cost_eur: None,
        };
        assert_eq!(depth_for_facets(&irreversible), QuizDepth::TeachBack);

        let wide = LeverageFacets {
            irreversible: false,
            blast_radius: BlastRadius::Company,
            divergence: Divergence::Low,
            est_cost_eur: None,
        };
        assert_eq!(depth_for_facets(&wide), QuizDepth::TeachBack);
    }
}
