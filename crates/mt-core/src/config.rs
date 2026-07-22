//! Конфігурація `.mt.json`: дефолти + merge (порт `npm/lib/core/config.mjs`).
//!
//! Читання файлів лишається на боці викликача (JS-обгортка зберігає ін'єкції
//! `exists`/`readFile`); сюди приходить лише сирий текст `.mt.json` або `None`.
//!
//! Пріоритет ефективного конфігу вузла (спадання): `plan_NNN.md` frontmatter >
//! `.mt-override.json` > `task.md` frontmatter > `.mt.json` > дефолти.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::frontmatter::parse_front_matter;

/// Дефолтні значення конфігурації — 1:1 з JS `CONFIG_DEFAULTS`
/// (порядок ключів значущий: JS-об'єкт зберігає порядок вставки).
pub fn config_defaults() -> Value {
    serde_json::json!({
        "mt_dir": "./mt",
        "worktrees_dir": "./.worktrees",
        "warn_worktrees_above": 4,
        "max_worktrees": 8,
        "default_budget_sec": 1800,
        "default_mode": "human",
        "default_model_tier": "AVG",
        "budget_hard_sec_multiplier": 3,
        "progress_timeout_sec": 300,
        "agent_concurrency": 5,
        "claim_lease_sec": 3600,
        "claim_grace_sec": 60,
        "publish_retry_max": 8,
        "publish_retry_base_ms": 250,
        "stale_worktree_min": 30,
        "system_prompt": ".mt/system-prompt.md"
    })
}

/// Зливає сирий текст `.mt.json` з дефолтами (JS `loadConfig` без FS).
/// `None` / невалідний JSON / не-об'єкт → чисті дефолти. Модельна
/// конфігурація виконавців — НЕ тут: вона user-level, через ENV
/// (`MT_AGENT_CLI` / `MT_CLOUD_AGENT_CLIS` / `MT_AGENT_CLI_MODEL_MAP`).
pub fn merge_config(raw: Option<&str>) -> Value {
    let defaults = config_defaults();
    let Some(raw) = raw else {
        return defaults;
    };
    let Ok(Value::Object(overrides)) = serde_json::from_str::<Value>(raw) else {
        return defaults;
    };

    let Value::Object(mut merged) = defaults else {
        unreachable!("config_defaults is an object");
    };
    for (k, v) in overrides {
        merged.insert(k, v);
    }
    Value::Object(merged)
}

/// Плоский merge JSON-об'єктів: ключі `over` поверх `base` (не-об'єкти ігноруються).
fn overlay(base: &mut Map<String, Value>, over: &Value) {
    if let Value::Object(o) = over {
        for (k, v) in o {
            base.insert(k.clone(), v.clone());
        }
    }
}

/// Ефективний конфіг вузла (spec-пріоритет): дефолти ← `.mt.json` ←
/// `task.md` frontmatter ← `.mt-override.json` ← `plan_NNN.md` frontmatter.
/// Кожен аргумент — сирий текст відповідного файлу, якщо він існує.
pub fn effective_config(
    mt_json: Option<&str>,
    task_md: Option<&str>,
    mt_override_json: Option<&str>,
    plan_md: Option<&str>,
) -> Value {
    let Value::Object(mut merged) = merge_config(mt_json) else {
        unreachable!("merge_config returns an object");
    };
    if let Some(text) = task_md {
        overlay(&mut merged, &parse_front_matter(text));
    }
    if let Some(raw) = mt_override_json {
        if let Ok(v) = serde_json::from_str::<Value>(raw) {
            overlay(&mut merged, &v);
        }
    }
    if let Some(text) = plan_md {
        overlay(&mut merged, &parse_front_matter(text));
    }
    Value::Object(merged)
}

/// Канонізує тир моделі: uppercase (`MIN` | `AVG` | `MAX`); порожнє → `""`
/// (порт JS `normalizeModelTier`).
pub fn normalize_model_tier(tier: &str) -> String {
    tier.to_uppercase()
}

/// Конфігурація виконавців — **user-level, з ENV** (runtime.md «Підписочні
/// CLI-виконавці»): вона спільна для всіх репозиторіїв користувача і тому НЕ
/// живе у repo-scoped `.mt.json`. Порт JS `loadAgentCliEnv`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentCliEnv {
    /// Дефолтний CLI (`MT_AGENT_CLI`): claude | codex | cursor | pi.
    pub agent_cli: String,
    /// Каскад хмарних CLI (`MT_CLOUD_AGENT_CLIS`, comma-separated).
    pub cloud_agent_clis: Vec<String>,
    /// JSON-мапа «CLI → тир → модель» (`MT_AGENT_CLI_MODEL_MAP`).
    pub model_map: Value,
}

impl Default for AgentCliEnv {
    fn default() -> Self {
        Self {
            agent_cli: "claude".to_string(),
            cloud_agent_clis: Vec::new(),
            model_map: Value::Object(Map::new()),
        }
    }
}

/// Будує [`AgentCliEnv`] через getter змінних середовища (ін'єкція для
/// тестів; продакшн — [`agent_cli_env_from_process`]). Невалідний або
/// не-об'єктний `MT_AGENT_CLI_MODEL_MAP` → порожня мапа.
pub fn load_agent_cli_env(get: impl Fn(&str) -> Option<String>) -> AgentCliEnv {
    let model_map = get("MT_AGENT_CLI_MODEL_MAP")
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| Value::Object(Map::new()));
    let agent_cli = get("MT_AGENT_CLI")
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "claude".to_string())
        .to_lowercase();
    let cloud_agent_clis = get("MT_CLOUD_AGENT_CLIS")
        .unwrap_or_default()
        .split(',')
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect();
    AgentCliEnv {
        agent_cli,
        cloud_agent_clis,
        model_map,
    }
}

/// [`AgentCliEnv`] зі змінних середовища поточного процесу.
pub fn agent_cli_env_from_process() -> AgentCliEnv {
    load_agent_cli_env(|k| std::env::var(k).ok())
}

/// Резолвить конкретну модель тиру для підписочного CLI: MIN/AVG/MAX →
/// `model_map[<cli>][<tier>]`. Немає мапінгу → `None`: CLI резолвить модель
/// сам, тир лишається hint-ом env `MT_MODEL_TIER` (порт JS `resolveModelForCli`).
pub fn resolve_model_for_cli(
    cli_env: &AgentCliEnv,
    agent_cli: &str,
    model_tier: &str,
) -> Option<String> {
    cli_env
        .model_map
        .get(agent_cli)
        .and_then(|m| m.get(normalize_model_tier(model_tier)))
        .and_then(Value::as_str)
        .map(String::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_when_no_raw() {
        let cfg = merge_config(None);
        assert_eq!(cfg["mt_dir"], "./mt");
        assert_eq!(cfg["system_prompt"], ".mt/system-prompt.md");
        assert!(cfg.get("tasks_dir").is_none());
    }

    #[test]
    fn defaults_on_invalid_json() {
        assert_eq!(merge_config(Some("not json {")), config_defaults());
        assert_eq!(merge_config(Some("[1,2]")), config_defaults());
    }

    #[test]
    fn merges_overrides_and_keeps_defaults() {
        let cfg = merge_config(Some(r#"{"mt_dir":"./my-tasks","max_worktrees":12}"#));
        assert_eq!(cfg["mt_dir"], "./my-tasks");
        assert_eq!(cfg["max_worktrees"], 12);
        assert_eq!(cfg["worktrees_dir"], "./.worktrees");
    }

    #[test]
    fn no_model_keys_in_defaults() {
        // Модельна конфігурація виконавців — user-level ENV, не .mt.json.
        let cfg = merge_config(None);
        assert!(cfg.get("model_map").is_none());
        assert!(cfg.get("claude_model").is_none());
        assert!(cfg.get("audit_model").is_none());
    }

    #[test]
    fn agent_cli_env_defaults_and_parsing() {
        let env = load_agent_cli_env(|_| None);
        assert_eq!(env.agent_cli, "claude");
        assert!(env.cloud_agent_clis.is_empty());
        assert!(env.model_map.as_object().unwrap().is_empty());

        let env = load_agent_cli_env(|k| match k {
            "MT_AGENT_CLI" => Some("CODEX".to_string()),
            "MT_CLOUD_AGENT_CLIS" => Some(" Codex, cursor ,,".to_string()),
            "MT_AGENT_CLI_MODEL_MAP" => Some(r#"{"codex":{"AVG":"gpt-5.6-terra"}}"#.to_string()),
            _ => None,
        });
        assert_eq!(env.agent_cli, "codex");
        assert_eq!(env.cloud_agent_clis, ["codex", "cursor"]);
        assert_eq!(
            resolve_model_for_cli(&env, "codex", "avg").as_deref(),
            Some("gpt-5.6-terra")
        );
        // Немає мапінгу → None: CLI резолвить модель сам.
        assert_eq!(resolve_model_for_cli(&env, "cursor", "AVG"), None);
    }

    #[test]
    fn agent_cli_env_invalid_model_map_is_empty() {
        let env = load_agent_cli_env(|k| match k {
            "MT_AGENT_CLI_MODEL_MAP" => Some("[not an object]".to_string()),
            _ => None,
        });
        assert!(env.model_map.as_object().unwrap().is_empty());
        assert_eq!(resolve_model_for_cli(&env, "claude", "MAX"), None);
    }

    #[test]
    fn effective_config_priority_chain() {
        let cfg = effective_config(
            Some(r#"{"default_budget_sec": 100, "progress_timeout_sec": 60}"#),
            Some("---\ndefault_budget_sec: 200\nhint: atomic\n---\n"),
            Some(r#"{"default_budget_sec": 300}"#),
            Some("---\ndefault_budget_sec: 400\n---\n"),
        );
        // plan_NNN > .mt-override.json > task.md > .mt.json
        assert_eq!(cfg["default_budget_sec"], 400);
        assert_eq!(cfg["hint"], "atomic");
        assert_eq!(cfg["progress_timeout_sec"], 60);
        assert_eq!(cfg["mt_dir"], "./mt");
    }

    #[test]
    fn effective_config_without_layers_is_merge_config() {
        assert_eq!(effective_config(None, None, None, None), config_defaults());
    }
}
