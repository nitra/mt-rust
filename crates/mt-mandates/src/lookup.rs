//! `effective_owner` — маршрутизатор ескалацій, «крок 1: lookup»
//! (mandates.md, «Маршрутизатор ескалацій»).
//!
//! Приймає вже провалідований [`MandatesFile`] ([`crate::parse::parse_mandates`])
//! — тут немає повторної структурної валідації, лише lookup і побудова
//! ланцюга ескалації.
//!
//! **Рішення, не покрите буквою контракту:**
//! - **Специфічність match-у.** Коли кілька мандатів покривають один і той
//!   самий `(node_ref, decision_type)`, контракт не задає формулу
//!   пріоритету. Тут — найдовший (у символах) `refs`-патерн, що зійшовся;
//!   найпоширеніша евристика «специфічніший патерн — довший рядок»
//!   (CODEOWNERS: last-match-wins на довжині патерну). Нічия за довжиною —
//!   `LookupError::Ambiguous` (fail-closed, не тиха відповідь навмання).
//! - **`facets` у сигнатурі napi-контракту** — тут `DecisionFacets`: фактичні
//!   значення рішення (бюджет/ризик/незворотність), які звіряються зі
//!   `thresholds` кандидата як стеля (мандат покриває рішення лише в межах
//!   своїх порогів).

use globset::Glob;

use crate::types::{DecisionFacets, MandateKind, MandatesFile};

/// Результат lookup — обчислений власник, його `kind` і повний ланцюг
/// ескалації до кореня (включно з обома кінцями) для автопідйому по SLA.
#[derive(Debug, Clone, PartialEq)]
pub struct EffectiveOwner {
    pub owner: String,
    pub kind: MandateKind,
    pub escalation_chain: Vec<String>,
}

/// Помилка lookup-у.
#[derive(Debug, Clone, PartialEq)]
pub enum LookupError {
    /// Жоден мандат не покриває `(node_ref, decision_type)` у межах порогів.
    NoMatch,
    /// Кілька мандатів однаково специфічні — навмання не вирішуємо.
    Ambiguous(Vec<String>),
    /// `node_ref` не є валідним значенням для жодного скомпільованого
    /// glob-патерну (порожній рядок тощо).
    InvalidRef(String),
}

impl std::fmt::Display for LookupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LookupError::NoMatch => write!(f, "жоден мандат не покриває цей ref/decision_type"),
            LookupError::Ambiguous(owners) => {
                write!(f, "неоднозначний lookup: кандидати {owners:?}")
            }
            LookupError::InvalidRef(r) => write!(f, "невалідний node_ref: {r}"),
        }
    }
}

impl std::error::Error for LookupError {}

/// Крок 1 маршрутизатора: детермінований lookup власника за
/// `refs × decision_types × thresholds`.
pub fn effective_owner(
    file: &MandatesFile,
    node_ref: &str,
    decision_type: &str,
    facets: &DecisionFacets,
) -> Result<EffectiveOwner, LookupError> {
    if node_ref.is_empty() {
        return Err(LookupError::InvalidRef(node_ref.to_string()));
    }

    let mut best: Vec<(usize, &str)> = Vec::new();
    let mut best_specificity = 0usize;

    for mandate in &file.mandates {
        if !mandate.scope.covers_all_decision_types()
            && !mandate
                .scope
                .decision_types
                .iter()
                .any(|t| t == decision_type)
        {
            continue;
        }

        let Some(specificity) = matching_ref_specificity(&mandate.scope.refs, node_ref) else {
            continue;
        };

        if !facets_fit_thresholds(facets, &mandate.thresholds) {
            continue;
        }

        match specificity.cmp(&best_specificity) {
            std::cmp::Ordering::Greater => {
                best_specificity = specificity;
                best = vec![(specificity, mandate.owner.as_str())];
            }
            std::cmp::Ordering::Equal if !best.is_empty() => {
                best.push((specificity, mandate.owner.as_str()));
            }
            _ => {}
        }
    }

    match best.len() {
        0 => Err(LookupError::NoMatch),
        1 => {
            let owner = best[0].1;
            let kind = file
                .mandates
                .iter()
                .find(|m| m.owner == owner)
                .map(|m| m.kind)
                .unwrap_or_default();
            Ok(EffectiveOwner {
                owner: owner.to_string(),
                kind,
                escalation_chain: escalation_chain_from(file, owner),
            })
        }
        _ => Err(LookupError::Ambiguous(
            best.into_iter().map(|(_, o)| o.to_string()).collect(),
        )),
    }
}

/// Найбільша специфічність (довжина патерну в символах) серед `refs`, що
/// збіглися з `node_ref`. `None`, якщо жоден не збігся.
fn matching_ref_specificity(refs: &[String], node_ref: &str) -> Option<usize> {
    refs.iter()
        .filter_map(|pattern| {
            let glob = Glob::new(pattern).ok()?;
            glob.compile_matcher()
                .is_match(node_ref)
                .then_some(pattern.chars().count())
        })
        .max()
}

/// Мандат покриває рішення лише в межах власних порогів: відсутній поріг =
/// без обмеження на цій осі (застосовується стеля, не рівність).
fn facets_fit_thresholds(facets: &DecisionFacets, thresholds: &crate::types::Thresholds) -> bool {
    if let (Some(cap), Some(actual)) = (thresholds.budget_eur, facets.budget_eur) {
        if actual > cap {
            return false;
        }
    }
    if let (Some(cap), Some(actual)) = (thresholds.risk, facets.risk) {
        if actual > cap {
            return false;
        }
    }
    if facets.irreversible && !thresholds.irreversible_or_default() {
        return false;
    }
    true
}

/// Повний ланцюг `escalates_to` від `owner` до кореня (включно з обома
/// кінцями). Викликач гарантує, що `file` пройшов [`crate::parse::validate`]
/// (без циклів/висячих handle) — цикл-guard тут лише захист від паніки на
/// непровалідованому вводі, не заміна валідації.
fn escalation_chain_from(file: &MandatesFile, owner: &str) -> Vec<String> {
    let mut chain = vec![owner.to_string()];
    let mut current = owner;
    let max_steps = file.mandates.len() + 1;

    for _ in 0..max_steps {
        let Some(mandate) = file.mandates.iter().find(|m| m.owner == current) else {
            break;
        };
        match &mandate.escalates_to {
            Some(next) => {
                chain.push(next.clone());
                current = next.as_str();
            }
            None => break,
        }
    }
    chain
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Mandate, MandateKind, RiskLevel, Scope, Thresholds};

    fn scope(refs: &[&str], decision_types: &[&str]) -> Scope {
        Scope {
            refs: refs.iter().map(|s| s.to_string()).collect(),
            decision_types: decision_types.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn fixture() -> MandatesFile {
        MandatesFile {
            generation: 1,
            mandates: vec![
                Mandate {
                    owner: "olena".into(),
                    kind: MandateKind::Person,
                    scope: scope(&["refs/mt/tasks/design/**"], &["architecture", "ux"]),
                    thresholds: Thresholds {
                        budget_eur: Some(2000.0),
                        risk: Some(RiskLevel::Medium),
                        irreversible: Some(false),
                        audacity: None,
                    },
                    escalates_to: Some("vitalii".into()),
                },
                Mandate {
                    owner: "fable-5".into(),
                    kind: MandateKind::Model,
                    scope: scope(&["refs/mt/tasks/routine/**"], &["ops"]),
                    thresholds: Thresholds {
                        budget_eur: Some(200.0),
                        risk: Some(RiskLevel::Low),
                        irreversible: Some(false),
                        audacity: None,
                    },
                    escalates_to: Some("olena".into()),
                },
                Mandate {
                    owner: "vitalii".into(),
                    kind: MandateKind::Person,
                    scope: scope(&["refs/mt/**"], &["*"]),
                    thresholds: Thresholds::default(),
                    escalates_to: None,
                },
            ],
        }
    }

    fn facets(budget: Option<f64>, risk: Option<RiskLevel>, irreversible: bool) -> DecisionFacets {
        DecisionFacets {
            budget_eur: budget,
            risk,
            irreversible,
        }
    }

    #[test]
    fn specific_mandate_wins_over_root() {
        let result = effective_owner(
            &fixture(),
            "refs/mt/tasks/design/0001",
            "architecture",
            &facets(Some(500.0), Some(RiskLevel::Low), false),
        )
        .expect("match");
        assert_eq!(result.owner, "olena");
        assert_eq!(result.escalation_chain, vec!["olena", "vitalii"]);
    }

    #[test]
    fn wildcard_decision_type_matches_root() {
        let result = effective_owner(
            &fixture(),
            "refs/mt/tasks/whatever/0001",
            "anything",
            &facets(None, None, false),
        )
        .expect("match via root wildcard");
        assert_eq!(result.owner, "vitalii");
        assert_eq!(result.escalation_chain, vec!["vitalii"]);
    }

    #[test]
    fn threshold_exceeded_falls_back_to_root() {
        // Бюджет перевищує поріг olena (2000) → кандидат відсіюється,
        // лишається лише корінь (без обмежень).
        let result = effective_owner(
            &fixture(),
            "refs/mt/tasks/design/0001",
            "architecture",
            &facets(Some(5000.0), Some(RiskLevel::Medium), false),
        )
        .expect("root still matches");
        assert_eq!(result.owner, "vitalii");
    }

    #[test]
    fn irreversible_decision_without_covering_mandate_is_no_match() {
        // Контракт: відсутнє thresholds.irreversible = false за замовчуванням
        // для БУДЬ-ЯКОГО мандата, включно з кореневим (`thresholds: {}`) —
        // незворотне рішення без явного `irreversible: true` десь у ланцюгу
        // не покривається жодним мандатом цієї фікстури.
        let err = effective_owner(
            &fixture(),
            "refs/mt/tasks/design/0001",
            "architecture",
            &facets(Some(100.0), Some(RiskLevel::Low), true),
        )
        .unwrap_err();
        assert_eq!(err, LookupError::NoMatch);
    }

    #[test]
    fn irreversible_decision_matches_mandate_with_explicit_cap() {
        let mut file = fixture();
        file.mandates[2].thresholds.irreversible = Some(true); // root
        let result = effective_owner(
            &file,
            "refs/mt/tasks/design/0001",
            "architecture",
            &facets(Some(100.0), Some(RiskLevel::Low), true),
        )
        .expect("root now covers irreversible decisions");
        assert_eq!(result.owner, "vitalii");
    }

    #[test]
    fn no_match_outside_scope() {
        let file = MandatesFile {
            generation: 1,
            mandates: vec![Mandate {
                owner: "a".into(),
                kind: MandateKind::Person,
                scope: scope(&["refs/mt/tasks/design/**"], &["architecture"]),
                thresholds: Thresholds::default(),
                escalates_to: None,
            }],
        };
        let err = effective_owner(
            &file,
            "refs/mt/tasks/routine/0001",
            "ops",
            &facets(None, None, false),
        )
        .unwrap_err();
        assert_eq!(err, LookupError::NoMatch);
    }

    #[test]
    fn ambiguous_equal_specificity_is_reported() {
        // Два мандати з тотожним за довжиною патерном покривають той самий
        // ref/decision_type — детермінований пріоритет неможливий.
        let file = MandatesFile {
            generation: 1,
            mandates: vec![
                Mandate {
                    owner: "a".into(),
                    kind: MandateKind::Person,
                    scope: scope(&["refs/mt/tasks/**"], &["architecture"]),
                    thresholds: Thresholds::default(),
                    escalates_to: None,
                },
                Mandate {
                    owner: "b".into(),
                    kind: MandateKind::Person,
                    scope: scope(&["refs/mt/tasks/**"], &["architecture"]),
                    thresholds: Thresholds::default(),
                    escalates_to: Some("a".into()),
                },
            ],
        };
        let err = effective_owner(
            &file,
            "refs/mt/tasks/design/1",
            "architecture",
            &facets(None, None, false),
        )
        .unwrap_err();
        assert!(matches!(err, LookupError::Ambiguous(_)));
    }

    #[test]
    fn empty_node_ref_is_invalid() {
        let err = effective_owner(&fixture(), "", "architecture", &facets(None, None, false))
            .unwrap_err();
        assert_eq!(err, LookupError::InvalidRef(String::new()));
    }
}
