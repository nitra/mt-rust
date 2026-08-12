//! Surface-профілі — іменовані профілі спеціалізації агента
//! (спека `surfaces.md`).
//!
//! Surface — **не окремий додаток і не тип сесії**: один agent-server
//! обслуговує будь-які surface, клієнт лише називає режим у
//! `UserMessage.surface`, а хост збирає конфігурацію ходу. Тому тут немає
//! нічого про транспорт — лише резолюція «рядок → профіль» і межі, у яких
//! цей профіль діє.
//!
//! Три поняття зі спеки, які легко сплутати:
//! `client_kind` — **хто** підключився, `client_capabilities` — **що клієнт
//! уміє показати**, `surface` — **у якому режимі працює агент**. Тут — лише
//! третє.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Профіль одного surface із `.mt.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SurfaceProfile {
    /// Виконавець цього surface (ACP). Єдине обовʼязкове поле зі спеки;
    /// відсутній профіль → виконавець сесії за замовчуванням.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_cli: Option<String>,
    /// Шлях до system-prompt цього режиму.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
    /// Скіли режиму (стеля — `a.md.skills` вузла, див. [`capped_skills`]).
    #[serde(default)]
    pub skills: Vec<String>,
    /// MCP-сервери режиму у формі `mcp:<name>`.
    #[serde(default)]
    pub tools: Vec<String>,
    /// Які `ContextSelected.kind` цей режим уміє інтерпретувати.
    #[serde(default)]
    pub context_kinds: Vec<String>,
}

/// Помилка резолюції surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SurfaceError {
    /// `ContextSelected` із kind, якого режим не розуміє.
    UnsupportedContextKind { surface: String, kind: String },
}

impl std::fmt::Display for SurfaceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedContextKind { surface, kind } => write!(
                f,
                "surface {surface} не інтерпретує context kind {kind} — \
                 подія відхилена (surfaces.md: не губимо мовчки)"
            ),
        }
    }
}

/// Профілі з конфігу проєкту.
///
/// Спека розширює `surface_profiles` з мапи «рядок → виконавець» до мапи
/// «рядок → обʼєкт». Обидві форми читаються: рядкове значення — це профіль
/// з одним `agent_cli`, і старий конфіг лишається валідним.
pub fn surface_profiles(config: &Value) -> std::collections::BTreeMap<String, SurfaceProfile> {
    let mut out = std::collections::BTreeMap::new();
    let Some(map) = config.get("surface_profiles").and_then(Value::as_object) else {
        return out;
    };
    for (name, raw) in map {
        let profile = match raw {
            Value::String(agent_cli) => SurfaceProfile {
                agent_cli: Some(agent_cli.clone()),
                ..SurfaceProfile::default()
            },
            other => serde_json::from_value(other.clone()).unwrap_or_default(),
        };
        out.insert(name.clone(), profile);
    }
    out
}

/// Default-surface за типом клієнта (спека: «на старті — default за
/// `client_kind`»).
///
/// Мапа свідомо крихітна і без конфігу: це стартова здогадка на перший хід,
/// яку клієнт перекриває першим же `surface`-hint-ом.
pub fn default_surface(client_kind: &str) -> Option<&'static str> {
    match client_kind {
        "cli" => Some("cli"),
        "designer" => Some("designer"),
        "writer" => Some("writer"),
        _ => None,
    }
}

/// Резолюція surface ходу: hint → попередній хід → default за `client_kind`.
///
/// Порядок зі спеки і не переставляється: hint сильніший за липкість, бо
/// саме ним клієнт перемикає режим усередині однієї сесії («тицьнув елемент
/// у preview → designer; попросив переписати абзац → writer»).
pub fn resolve_surface(
    hint: Option<&str>,
    previous: Option<&str>,
    client_kind: &str,
) -> Option<String> {
    hint.map(str::to_string)
        .or_else(|| previous.map(str::to_string))
        .or_else(|| default_surface(client_kind).map(str::to_string))
}

/// Ефективні скіли ходу — **перетин** профілю surface і стелі вузла.
///
/// Інваріант зі спеки: surface не може дати агенту більше, ніж дозволяє
/// задача. `a.md.skills` — стеля, surface — спеціалізація в її межах, тож
/// це саме перетин, а не обʼєднання й не заміна.
///
/// Порожня стеля означає «вузол не обмежує» — тоді діє профіль як є.
pub fn capped_skills(profile: &SurfaceProfile, node_ceiling: &[String]) -> Vec<String> {
    if node_ceiling.is_empty() {
        return profile.skills.clone();
    }
    if profile.skills.is_empty() {
        return node_ceiling.to_vec();
    }
    let ceiling: BTreeSet<&str> = node_ceiling.iter().map(String::as_str).collect();
    profile
        .skills
        .iter()
        .filter(|skill| ceiling.contains(skill.as_str()))
        .cloned()
        .collect()
}

/// Перевіряє `ContextSelected.kind` проти профілю.
///
/// Профіль без `context_kinds` не обмежує нічого: це «поле не заповнене»,
/// а не «нічого не дозволено» — інакше додавання профілю ламало б робочі
/// сценарії.
///
/// # Errors
/// Kind, якого режим не розуміє.
pub fn check_context_kind(
    surface: &str,
    profile: &SurfaceProfile,
    kind: &str,
) -> Result<(), SurfaceError> {
    if profile.context_kinds.is_empty() || profile.context_kinds.iter().any(|k| k == kind) {
        return Ok(());
    }
    Err(SurfaceError::UnsupportedContextKind {
        surface: surface.to_string(),
        kind: kind.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(json: &str) -> Value {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn object_and_string_forms_both_parse() {
        // Спека розширила формат з рядка до обʼєкта; старий конфіг має
        // лишитись валідним, інакше розширення схеми — ламка зміна.
        let profiles = surface_profiles(&config(
            r#"{"surface_profiles": {
                "designer": {"agent_cli": "pi", "skills": ["preview"], "context_kinds": ["dom_element"]},
                "legacy": "claude"
            }}"#,
        ));
        assert_eq!(profiles["designer"].agent_cli.as_deref(), Some("pi"));
        assert_eq!(profiles["designer"].context_kinds, ["dom_element"]);
        assert_eq!(profiles["legacy"].agent_cli.as_deref(), Some("claude"));
        assert!(profiles["legacy"].skills.is_empty());
    }

    #[test]
    fn missing_section_is_empty_not_error() {
        assert!(surface_profiles(&config("{}")).is_empty());
    }

    #[test]
    fn hint_beats_previous_and_default() {
        assert_eq!(
            resolve_surface(Some("writer"), Some("designer"), "cli").as_deref(),
            Some("writer")
        );
    }

    #[test]
    fn without_hint_surface_is_sticky() {
        // «Без hint — профіль попереднього ходу»: інакше режим злітав би
        // на кожному повідомленні без явної позначки.
        assert_eq!(
            resolve_surface(None, Some("designer"), "cli").as_deref(),
            Some("designer")
        );
    }

    #[test]
    fn first_turn_falls_back_to_client_kind() {
        assert_eq!(resolve_surface(None, None, "cli").as_deref(), Some("cli"));
        assert_eq!(resolve_surface(None, None, "mt-dashboard"), None);
    }

    #[test]
    fn skills_are_capped_by_node_ceiling() {
        // Surface не може дати більше, ніж дозволяє задача.
        let profile = SurfaceProfile {
            skills: vec!["read-files".into(), "write-files".into(), "bash".into()],
            ..SurfaceProfile::default()
        };
        let ceiling = vec!["read-files".to_string(), "write-files".to_string()];
        assert_eq!(
            capped_skills(&profile, &ceiling),
            ["read-files", "write-files"]
        );
    }

    #[test]
    fn empty_ceiling_means_unbounded_node() {
        let profile = SurfaceProfile {
            skills: vec!["bash".into()],
            ..SurfaceProfile::default()
        };
        assert_eq!(capped_skills(&profile, &[]), ["bash"]);
    }

    #[test]
    fn empty_profile_skills_inherit_ceiling() {
        // Профіль без skills — «не звужую», а не «нічого не можна».
        let ceiling = vec!["read-files".to_string()];
        assert_eq!(
            capped_skills(&SurfaceProfile::default(), &ceiling),
            ["read-files"]
        );
    }

    #[test]
    fn unknown_context_kind_is_rejected_explicitly() {
        // Спека: події з іншими kind хост відхиляє з Error, а не губить.
        let profile = SurfaceProfile {
            context_kinds: vec!["text_range".into()],
            ..SurfaceProfile::default()
        };
        assert!(check_context_kind("writer", &profile, "text_range").is_ok());
        let error = check_context_kind("writer", &profile, "dom_element").unwrap_err();
        assert_eq!(
            error,
            SurfaceError::UnsupportedContextKind {
                surface: "writer".into(),
                kind: "dom_element".into()
            }
        );
        assert!(error.to_string().contains("dom_element"));
    }

    #[test]
    fn profile_without_context_kinds_accepts_any() {
        assert!(check_context_kind("cli", &SurfaceProfile::default(), "file_region").is_ok());
    }
}
