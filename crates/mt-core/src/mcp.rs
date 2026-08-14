//! MCP-сервери surface (спека `surfaces.md`, «Tools: MCP — нормативний
//! механізм розширення»).
//!
//! Нормативне тут: **власного тул-протоколу не вводиться**. Кожен зовнішній
//! тул — MCP-сервер, оголошений у `mcp_servers` і згаданий у `tools`
//! профілю як `mcp:<name>`; сервери передаються виконавцю surface при старті
//! ACP-сесії.
//!
//! **Що робить цей модуль і чого не робить.** Він резолвить декларацію в
//! готовий payload `session/new`: підставляє значення `secret:<key>` через
//! брокер ([`crate::secrets`]) і перевіряє, що всі згадані в `tools` сервери
//! існують. Процесами MCP-серверів керує сам ACP-виконавець — лінивий старт
//! і `idle_ttl_sec` виконує він, а MT їх декларує. Це названо прямо, щоб
//! «життєвий цикл» зі спеки не виглядав реалізованим тут.
//!
//! Спеціалізація і є економія контексту: схеми тулів потрапляють у контекст
//! агента лише для ходів того surface, який їх оголосив, — тому вибірка
//! серверів іде від профілю, а не «усі оголошені».

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::secrets::{resolve_ref, SecretStore, SECRET_REF_PREFIX};
use crate::surfaces::SurfaceProfile;

/// Префікс посилання на MCP-сервер у `tools` профілю.
pub const MCP_TOOL_PREFIX: &str = "mcp:";

/// Дефолтний idle-TTL (`surfaces.md`); `0` — жити до кінця сесії.
pub const DEFAULT_IDLE_TTL_SEC: u64 = 600;

/// Декларація MCP-сервера з `.mt.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct McpServer {
    /// Програма запуску.
    pub command: String,
    /// Аргументи.
    pub args: Vec<String>,
    /// ENV; значення `secret:<key>` резолвить брокер.
    pub env: BTreeMap<String, String>,
    /// Скільки жити без використання; `0` — до кінця сесії.
    pub idle_ttl_sec: u64,
}

impl Default for McpServer {
    fn default() -> Self {
        Self {
            command: String::new(),
            args: Vec::new(),
            env: BTreeMap::new(),
            idle_ttl_sec: DEFAULT_IDLE_TTL_SEC,
        }
    }
}

/// Чому набір серверів не зібрався.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpError {
    /// `tools` посилається на сервер, якого немає в `mcp_servers`.
    UnknownServer { name: String },
    /// Секрет із `env` не резолвиться.
    MissingSecret { server: String, key: String },
    /// Декларація без `command`.
    NoCommand { name: String },
}

impl std::fmt::Display for McpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownServer { name } => write!(
                f,
                "surface посилається на `{MCP_TOOL_PREFIX}{name}`, але сервера {name} немає в mcp_servers"
            ),
            Self::MissingSecret { server, key } => write!(
                f,
                "MCP-сервер {server}: секрет `{key}` не резолвиться — сервер не стартує"
            ),
            Self::NoCommand { name } => {
                write!(f, "MCP-сервер {name}: не задано command")
            }
        }
    }
}

/// Декларації з конфігу проєкту.
pub fn mcp_servers(config: &Value) -> BTreeMap<String, McpServer> {
    config
        .get("mcp_servers")
        .and_then(Value::as_object)
        .map(|map| {
            map.iter()
                .map(|(name, raw)| {
                    (
                        name.clone(),
                        serde_json::from_value(raw.clone()).unwrap_or_default(),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Імена серверів, згаданих у `tools` профілю (`mcp:<name>`).
///
/// Записи без префікса ігноруються: `tools` — відкритий список, і не кожен
/// його елемент зобовʼязаний бути MCP-сервером.
pub fn referenced_servers(profile: &SurfaceProfile) -> Vec<String> {
    profile
        .tools
        .iter()
        .filter_map(|tool| tool.strip_prefix(MCP_TOOL_PREFIX))
        .map(str::to_string)
        .collect()
}

/// Готовий до запуску сервер: секрети вже підставлені.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedServer {
    /// Імʼя з конфігу.
    pub name: String,
    /// Програма.
    pub command: String,
    /// Аргументи.
    pub args: Vec<String>,
    /// ENV із підставленими значеннями.
    pub env: BTreeMap<String, String>,
    /// Idle-TTL, який виконує ACP-виконавець.
    pub idle_ttl_sec: u64,
}

/// Збирає сервери surface із підставленими секретами.
///
/// **Fail closed на обох краях.** Невідомий `mcp:<name>` — помилка, а не
/// тихий пропуск: інакше агент отримав би surface без тула, який його
/// профіль обіцяє, і мовчазну неможливість зробити роботу. Нерезолвлений
/// `secret:` — теж помилка: підставити літерал `secret:figma-token` у ENV
/// означало б віддати серверу рядок замість токена і отримати незрозумілу
/// відмову вже всередині нього.
///
/// # Errors
/// Невідомий сервер, відсутній секрет або декларація без `command`.
pub fn servers_for_surface(
    config: &Value,
    profile: &SurfaceProfile,
    store: &dyn SecretStore,
) -> Result<Vec<ResolvedServer>, McpError> {
    let declared = mcp_servers(config);
    let mut out = Vec::new();
    for name in referenced_servers(profile) {
        let Some(server) = declared.get(&name) else {
            return Err(McpError::UnknownServer { name });
        };
        if server.command.is_empty() {
            return Err(McpError::NoCommand { name });
        }
        let mut env = BTreeMap::new();
        for (key, raw) in &server.env {
            let Some(value) = resolve_ref(raw, store) else {
                return Err(McpError::MissingSecret {
                    server: name.clone(),
                    key: raw
                        .strip_prefix(SECRET_REF_PREFIX)
                        .unwrap_or(raw)
                        .to_string(),
                });
            };
            env.insert(key.clone(), value);
        }
        out.push(ResolvedServer {
            name: name.clone(),
            command: server.command.clone(),
            args: server.args.clone(),
            env,
            idle_ttl_sec: server.idle_ttl_sec,
        });
    }
    Ok(out)
}

/// Payload `session/new.mcpServers` у формі ACP.
///
/// `env` — масив пар `{name, value}`: саме так його описує ACP, і
/// перекладати сюди JSON-обʼєкт конфігу означало б розійтися з протоколом.
pub fn acp_payload(servers: &[ResolvedServer]) -> Value {
    Value::Array(
        servers
            .iter()
            .map(|server| {
                json!({
                    "name": server.name,
                    "command": server.command,
                    "args": server.args,
                    "env": server
                        .env
                        .iter()
                        .map(|(name, value)| json!({"name": name, "value": value}))
                        .collect::<Vec<_>>(),
                })
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::MemorySecretStore;

    fn config() -> Value {
        json!({
            "mcp_servers": {
                "figma": {
                    "command": "npx",
                    "args": ["figma-mcp"],
                    "env": {"FIGMA_TOKEN": "secret:figma-token"},
                    "idle_ttl_sec": 600
                },
                "browser": {"command": "npx", "args": ["browser-mcp"]}
            }
        })
    }

    fn profile(tools: &[&str]) -> SurfaceProfile {
        SurfaceProfile {
            tools: tools.iter().map(|t| t.to_string()).collect(),
            ..SurfaceProfile::default()
        }
    }

    fn store() -> MemorySecretStore {
        MemorySecretStore::new([("figma-token".to_string(), "fk_live_1".to_string())])
    }

    #[test]
    fn secret_ref_is_resolved_into_env() {
        let servers =
            servers_for_surface(&config(), &profile(&["mcp:figma"]), &store()).unwrap();
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].env["FIGMA_TOKEN"], "fk_live_1");
        assert_eq!(servers[0].idle_ttl_sec, 600);
    }

    #[test]
    fn missing_secret_refuses_the_server() {
        // Підставити літерал `secret:…` означало б віддати серверу рядок
        // замість токена і отримати незрозумілу відмову вже всередині нього.
        let empty = MemorySecretStore::default();
        let error =
            servers_for_surface(&config(), &profile(&["mcp:figma"]), &empty).unwrap_err();
        assert_eq!(
            error,
            McpError::MissingSecret {
                server: "figma".into(),
                key: "figma-token".into()
            }
        );
        assert!(error.to_string().contains("не стартує"));
    }

    #[test]
    fn unknown_server_is_an_error_not_a_silent_skip() {
        // Інакше агент отримав би surface без тула, який профіль обіцяє,
        // і мовчазну неможливість зробити роботу.
        let error =
            servers_for_surface(&config(), &profile(&["mcp:notion"]), &store()).unwrap_err();
        assert_eq!(
            error,
            McpError::UnknownServer {
                name: "notion".into()
            }
        );
    }

    #[test]
    fn non_mcp_tools_are_ignored() {
        // `tools` — відкритий список; не кожен елемент є MCP-сервером.
        let servers =
            servers_for_surface(&config(), &profile(&["preview", "mcp:browser"]), &store())
                .unwrap();
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].name, "browser");
    }

    #[test]
    fn declaration_without_command_is_refused() {
        let config = json!({"mcp_servers": {"broken": {"args": ["x"]}}});
        let error =
            servers_for_surface(&config, &profile(&["mcp:broken"]), &store()).unwrap_err();
        assert_eq!(
            error,
            McpError::NoCommand {
                name: "broken".into()
            }
        );
    }

    #[test]
    fn idle_ttl_defaults_to_spec_value() {
        let servers =
            servers_for_surface(&config(), &profile(&["mcp:browser"]), &store()).unwrap();
        assert_eq!(servers[0].idle_ttl_sec, DEFAULT_IDLE_TTL_SEC);
    }

    #[test]
    fn surface_without_tools_declares_nothing() {
        // Спеціалізація і є економія контексту: схеми тулів не течуть у
        // ходи чужого surface.
        let servers = servers_for_surface(&config(), &SurfaceProfile::default(), &store()).unwrap();
        assert!(servers.is_empty());
        assert_eq!(acp_payload(&servers), json!([]));
    }

    #[test]
    fn acp_payload_uses_protocol_env_shape() {
        // ACP описує env масивом пар {name, value}; обʼєкт конфігу тут
        // розійшовся б із протоколом.
        let servers =
            servers_for_surface(&config(), &profile(&["mcp:figma"]), &store()).unwrap();
        assert_eq!(
            acp_payload(&servers),
            json!([{
                "name": "figma",
                "command": "npx",
                "args": ["figma-mcp"],
                "env": [{"name": "FIGMA_TOKEN", "value": "fk_live_1"}]
            }])
        );
    }
}
