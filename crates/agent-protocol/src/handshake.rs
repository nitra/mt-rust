//! Хендшейк клієнт↔хост (спека runtime.md, «Хендшейк»).
//!
//! `ClientHello` → `ServerHello`; несумісна `protocol_version` → відмова
//! з явною помилкою і підказкою оновитись. `lang` (BCP-47) — ОБОВ'ЯЗКОВЕ
//! поле v4: керує live-перекладом (i18n.md), без нього хендшейк не
//! десеріалізується.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::PROTOCOL_VERSION;

/// Перше повідомлення клієнта. `client_kind` і `client_capabilities` —
/// відкриті множини рядків («designer» | «writer» | «cli» | «mobile» |
/// «mt-dashboard» | …; «preview», «approvals», «diff_view»,
/// «self-translate», …) — сервер фільтрує події за capabilities.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClientHello {
    pub protocol_version: u32,
    pub device_id: Uuid,
    pub device_token: String,
    pub client_kind: String,
    pub client_capabilities: Vec<String>,
    /// ОБОВ'ЯЗКОВЕ (v4): BCP-47 мова учасника.
    pub lang: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub want_replay_from: Option<u64>,
}

/// Відповідь сервера на сумісний `ClientHello`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ServerHello {
    pub protocol_version: u32,
    pub session_list: Vec<SessionInfo>,
}

/// Активна сесія хоста у `ServerHello.session_list`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionInfo {
    pub node_hash: String,
    pub run_token: Uuid,
}

/// Помилка хендшейку.
#[derive(Debug, Clone, PartialEq)]
pub enum ProtocolError {
    /// Версії не збігаються — точна рівність, бо номер версії і є мажором
    /// (мінорні розширення — нові Event-варіанти, які клієнт ігнорує).
    IncompatibleVersion { server: u32, client: u32 },
}

impl std::fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProtocolError::IncompatibleVersion { server, client } => write!(
                f,
                "incompatible protocol version: server speaks v{server}, client sent v{client} — \
                 update the {} side to v{server}",
                if client < server { "client" } else { "server" }
            ),
        }
    }
}

impl std::error::Error for ProtocolError {}

/// Перевірка сумісності версії клієнта з [`PROTOCOL_VERSION`].
pub fn check_protocol_version(client: u32) -> Result<(), ProtocolError> {
    if client == PROTOCOL_VERSION {
        Ok(())
    } else {
        Err(ProtocolError::IncompatibleVersion {
            server: PROTOCOL_VERSION,
            client,
        })
    }
}

impl ClientHello {
    /// Перевірка сумісності власної версії хендшейку.
    pub fn check_compatibility(&self) -> Result<(), ProtocolError> {
        check_protocol_version(self.protocol_version)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hello_json(with_lang: bool) -> String {
        let lang = if with_lang { r#""lang": "uk-UA","# } else { "" };
        format!(
            r#"{{
              "protocol_version": 4,
              "device_id": "00000000-0000-0000-0000-000000000001",
              "device_token": "tok",
              "client_kind": "cli",
              "client_capabilities": ["approvals"],
              {lang}
              "want_replay_from": 12
            }}"#
        )
    }

    /// `ClientHello` без `lang` НЕ десеріалізується (обов'язкове поле v4).
    #[test]
    fn client_hello_without_lang_is_rejected() {
        let error = serde_json::from_str::<ClientHello>(&hello_json(false)).unwrap_err();
        assert!(
            error.to_string().contains("lang"),
            "помилка мовчить про lang: {error}"
        );
    }

    #[test]
    fn client_hello_with_lang_roundtrips() {
        let hello: ClientHello = serde_json::from_str(&hello_json(true)).unwrap();
        assert_eq!(hello.lang, "uk-UA");
        assert_eq!(hello.want_replay_from, Some(12));
        let json = serde_json::to_string(&hello).unwrap();
        assert_eq!(serde_json::from_str::<ClientHello>(&json).unwrap(), hello);
    }

    /// Сумісна версія проходить; несумісна → явна помилка з підказкою.
    #[test]
    fn version_check_is_exact_with_hint() {
        assert!(check_protocol_version(PROTOCOL_VERSION).is_ok());
        let error = check_protocol_version(3).unwrap_err();
        assert_eq!(
            error,
            ProtocolError::IncompatibleVersion {
                server: 4,
                client: 3
            }
        );
        let message = error.to_string();
        assert!(
            message.contains("v3") && message.contains("v4"),
            "{message}"
        );
        assert!(message.contains("update the client"), "{message}");
    }

    #[test]
    fn server_hello_roundtrips() {
        let hello = ServerHello {
            protocol_version: PROTOCOL_VERSION,
            session_list: vec![SessionInfo {
                node_hash: "c".repeat(20),
                run_token: Uuid::from_u128(9),
            }],
        };
        let json = serde_json::to_string(&hello).unwrap();
        assert_eq!(serde_json::from_str::<ServerHello>(&json).unwrap(), hello);
    }
}
