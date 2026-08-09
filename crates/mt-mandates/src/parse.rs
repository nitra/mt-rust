//! `parse_mandates` — розбір і повна валідація `.mt/mandates.yaml`
//! (mandates.md, «Нормативний контракт (M6 фаза 0)»).
//!
//! Serde сам по собі не може виразити частину контракту (рівно один
//! корінь, досяжність, непорожній scope, `audacity` лише для `kind: model`)
//! — ці перевірки живуть тут, окремим проходом після структурного
//! десеріалізування, і повертають людинозрозумілу причину замість
//! serde-помилки.
//!
//! **Рішення, не покрите буквою контракту:** `owner` трактується як
//! унікальний ключ запису (одна активна делегація на владника) — інакше
//! ланцюг `escalates_to` було б неможливо однозначно пройти, коли той самий
//! `owner` фігурує у кількох рядках з різними адресатами ескалації.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::Path;

use crate::types::{Mandate, MandateKind, MandatesFile};

/// Помилка `parse_mandates` — читання файлу, розбір YAML або порушення
/// контракту.
#[derive(Debug)]
pub enum MandatesError {
    Io(std::io::Error),
    Yaml(serde_norway::Error),
    Validation(String),
}

impl fmt::Display for MandatesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MandatesError::Io(e) => write!(f, "не вдалося прочитати mandates.yaml: {e}"),
            MandatesError::Yaml(e) => write!(f, "не вдалося розібрати mandates.yaml: {e}"),
            MandatesError::Validation(reason) => write!(f, "mandates.yaml невалідний: {reason}"),
        }
    }
}

impl std::error::Error for MandatesError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            MandatesError::Io(e) => Some(e),
            MandatesError::Yaml(e) => Some(e),
            MandatesError::Validation(_) => None,
        }
    }
}

impl From<std::io::Error> for MandatesError {
    fn from(e: std::io::Error) -> Self {
        MandatesError::Io(e)
    }
}

/// Парсить `.mt/mandates.yaml` за шляхом і повністю валідує за контрактом.
pub fn parse_mandates(path: &Path) -> Result<MandatesFile, MandatesError> {
    let text = std::fs::read_to_string(path)?;
    parse_mandates_str(&text)
}

/// [`parse_mandates`] з рядка — для тестів і викликачів, що вже тримають
/// вміст файлу (напр. napi-шар, що читає файл на JS-стороні).
pub fn parse_mandates_str(text: &str) -> Result<MandatesFile, MandatesError> {
    let file: MandatesFile = serde_norway::from_str(text).map_err(MandatesError::Yaml)?;
    validate(&file).map_err(MandatesError::Validation)?;
    Ok(file)
}

/// Повна структурна валідація за контрактом. `pub(crate)`, бо
/// [`crate::change::validate_mandate_change`] перевіряє нею новий стан
/// файлу диференційовано від crypto/підписних правил.
pub(crate) fn validate(file: &MandatesFile) -> Result<(), String> {
    if file.generation < 1 {
        return Err(format!(
            "generation має бути ≥ 1, отримано {}",
            file.generation
        ));
    }

    if file.mandates.is_empty() {
        return Err("mandates[] порожній — файл без кореневого мандата невалідний".to_string());
    }

    let mut owners: HashSet<&str> = HashSet::new();
    let mut escalates_to_map: HashMap<&str, Option<&str>> = HashMap::new();
    let mut root_owner: Option<&str> = None;
    let mut root_count = 0u32;

    for mandate in &file.mandates {
        validate_mandate_shape(mandate)?;

        if !owners.insert(mandate.owner.as_str()) {
            return Err(format!(
                "owner '{}' зустрічається у мандатах більше одного разу — \
                 кожен owner має рівно один активний запис",
                mandate.owner
            ));
        }
        escalates_to_map.insert(mandate.owner.as_str(), mandate.escalates_to.as_deref());

        if mandate.escalates_to.is_none() {
            root_count += 1;
            root_owner = Some(mandate.owner.as_str());
        }
    }

    match root_count {
        0 => {
            return Err(
                "немає кореневого мандата (escalates_to: null) — рівно один обов'язковий"
                    .to_string(),
            )
        }
        1 => {}
        n => {
            return Err(format!(
                "{n} мандатів з escalates_to: null — має бути рівно один кореневий"
            ))
        }
    }
    let root_owner = root_owner.expect("root_count == 1 гарантує Some");

    // Досяжність: з кожного owner скінченним ланцюгом escalates_to до кореня,
    // без циклів і висячих handle. Bound = кількість мандатів + 1: довший
    // ланцюг без досягнення кореня в ациклічному графі можливий лише циклом.
    let max_steps = file.mandates.len() + 1;
    for &owner in &owners {
        let mut current = owner;
        let mut steps = 0usize;
        loop {
            if current == root_owner {
                break;
            }
            steps += 1;
            if steps > max_steps {
                return Err(format!(
                    "цикл ескалації: '{owner}' не досягає кореня '{root_owner}' \
                     за {max_steps} кроків"
                ));
            }
            match escalates_to_map.get(current) {
                Some(Some(next)) => current = next,
                Some(None) => break, // інший корінь — вже відхилено вище, недосяжно
                None => {
                    return Err(format!(
                        "'{owner}' ескалює до '{current}', якого немає серед owner мандатів \
                         (висячий handle)"
                    ))
                }
            }
        }
    }

    Ok(())
}

/// Перевірки одного запису: непорожній scope, `audacity` лише для
/// `kind: model`.
fn validate_mandate_shape(mandate: &Mandate) -> Result<(), String> {
    if mandate.scope.refs.is_empty() {
        return Err(format!(
            "owner '{}': scope.refs порожній — порожній масив недопустимий (не «усе»)",
            mandate.owner
        ));
    }
    if mandate.scope.decision_types.is_empty() {
        return Err(format!(
            "owner '{}': scope.decision_types порожній — порожній масив недопустимий (не «усе»)",
            mandate.owner
        ));
    }
    if mandate.kind == MandateKind::Person && mandate.thresholds.audacity.is_some() {
        return Err(format!(
            "owner '{}': thresholds.audacity неприпустиме для kind: person",
            mandate.owner
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID: &str = r#"
generation: 3
mandates:
  - owner: olena
    scope:
      refs: ["refs/mt/tasks/design/**"]
      decision_types: [architecture, ux]
    thresholds: { budget_eur: 2000, risk: medium, irreversible: false }
    escalates_to: vitalii
  - owner: fable-5
    kind: model
    scope:
      refs: ["refs/mt/tasks/routine/**"]
      decision_types: [ops]
    thresholds: { budget_eur: 200, risk: low, irreversible: false, audacity: medium }
    escalates_to: olena
  - owner: vitalii
    scope: { refs: ["refs/mt/**"], decision_types: ["*"] }
    thresholds: {}
    escalates_to: null
"#;

    #[test]
    fn valid_fixture_parses() {
        let file = parse_mandates_str(VALID).expect("valid fixture");
        assert_eq!(file.generation, 3);
        assert_eq!(file.mandates.len(), 3);
    }

    #[test]
    fn zero_roots_is_invalid() {
        let text = VALID.replace("escalates_to: null", "escalates_to: vitalii-sr");
        let err = parse_mandates_str(&text).unwrap_err();
        assert!(matches!(err, MandatesError::Validation(_)));
    }

    #[test]
    fn two_roots_is_invalid() {
        let text = VALID.replace("escalates_to: vitalii", "escalates_to: null");
        let err = parse_mandates_str(&text).unwrap_err();
        assert!(format!("{err}").contains("кореневий"));
    }

    #[test]
    fn escalation_cycle_is_invalid() {
        let text = r#"
generation: 1
mandates:
  - owner: a
    scope: { refs: ["refs/mt/**"], decision_types: ["*"] }
    thresholds: {}
    escalates_to: b
  - owner: b
    scope: { refs: ["refs/mt/**"], decision_types: ["*"] }
    thresholds: {}
    escalates_to: a
"#;
        let err = parse_mandates_str(text).unwrap_err();
        assert!(format!("{err}").contains("кореневого"));
    }

    #[test]
    fn dangling_escalation_handle_is_invalid() {
        let text = r#"
generation: 1
mandates:
  - owner: a
    scope: { refs: ["refs/mt/**"], decision_types: ["*"] }
    thresholds: {}
    escalates_to: ghost
"#;
        let err = parse_mandates_str(text).unwrap_err();
        assert!(matches!(err, MandatesError::Validation(_)));
    }

    #[test]
    fn empty_refs_scope_is_invalid() {
        let text = VALID.replace(r#"refs: ["refs/mt/tasks/design/**"]"#, "refs: []");
        let err = parse_mandates_str(&text).unwrap_err();
        assert!(format!("{err}").contains("scope.refs"));
    }

    #[test]
    fn empty_decision_types_scope_is_invalid() {
        let text = VALID.replace("decision_types: [architecture, ux]", "decision_types: []");
        let err = parse_mandates_str(&text).unwrap_err();
        assert!(format!("{err}").contains("scope.decision_types"));
    }

    #[test]
    fn audacity_on_person_is_invalid() {
        let text = VALID.replace(
            "thresholds: { budget_eur: 2000, risk: medium, irreversible: false }",
            "thresholds: { budget_eur: 2000, risk: medium, irreversible: false, audacity: low }",
        );
        let err = parse_mandates_str(&text).unwrap_err();
        assert!(format!("{err}").contains("audacity"));
    }

    #[test]
    fn generation_zero_is_invalid() {
        let text = VALID.replace("generation: 3", "generation: 0");
        let err = parse_mandates_str(&text).unwrap_err();
        assert!(format!("{err}").contains("generation"));
    }

    #[test]
    fn empty_mandates_array_is_invalid() {
        let text = "generation: 1\nmandates: []\n";
        let err = parse_mandates_str(text).unwrap_err();
        assert!(format!("{err}").contains("кореневого"));
    }

    #[test]
    fn duplicate_owner_is_invalid() {
        let text = r#"
generation: 1
mandates:
  - owner: a
    scope: { refs: ["refs/mt/**"], decision_types: ["*"] }
    thresholds: {}
    escalates_to: null
  - owner: a
    scope: { refs: ["refs/mt/other/**"], decision_types: ["*"] }
    thresholds: {}
    escalates_to: null
"#;
        let err = parse_mandates_str(text).unwrap_err();
        assert!(format!("{err}").contains("більше одного разу"));
    }
}
