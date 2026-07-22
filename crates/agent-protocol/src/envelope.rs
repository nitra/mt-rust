//! `Envelope`/`Event` — стрічка подій сесії (спека runtime.md, «Протокол подій»).
//!
//! `session.jsonl` — append-only список Envelope-ів; ефемерні події
//! (`PreviewScreenshot`, `AgentTextDelta`) можна не журналити. Невідомий
//! `Event`-варіант у межах сумісної версії десеріалізується в
//! [`Event::Unknown`] і клієнтом ігнорується (forward-compatibility
//! мінорних розширень).

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;
use uuid::Uuid;

/// Один запис стрічки подій run-а. `seq` монотонний у межах run;
/// призначає тримач claim. `run_token` = token claim-а (ідентифікатор сесії).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Envelope {
    pub seq: u64,
    pub ts: DateTime<Utc>,
    /// Кімната/адреса вузла.
    pub node_hash: String,
    pub run_token: Uuid,
    /// Хто ініціював — для подій від клієнтів.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_id: Option<Uuid>,
    /// У спільних задачах учасників кілька.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_id: Option<Uuid>,
    pub event: Event,
}

/// Прямокутник контексту вибору (`ContextSelected.bounding_box`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Знімок claim-а вузла у `NodeState` (джерело істини — git ref).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClaimInfo {
    pub holder_device: Uuid,
    pub lease_until: DateTime<Utc>,
    pub generation: u64,
}

/// Подія протоколу v4. Варіанти «клієнт → хост» ідуть першими, далі
/// «хост → клієнти» — порядок і назви віддзеркалюють runtime.md.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Event {
    // ── клієнт → хост ──────────────────────────────────────────────────────
    /// `surface`-hint («designer» | «writer» | «cli» | …) — агент може
    /// підставити відповідний профіль провайдера/промпт.
    UserMessage {
        text: String,
        attachments: Vec<Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        surface: Option<String>,
    },
    /// Контекст, у який «тицьнув» користувач, незалежно від додатку:
    /// `kind` — «dom_element» | «text_range» | «file_region» | ….
    ContextSelected {
        kind: String,
        payload: Value,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bounding_box: Option<Rect>,
    },
    /// Ed25519-підпис пристрою над `(request_id, approved, node_hash,
    /// run_token)`; пристрій може належати ІНШОМУ акаунту з роллю approver+.
    ApprovalResponse {
        request_id: String,
        approved: bool,
        #[serde(with = "base64_bytes")]
        signature: Vec<u8>,
    },
    CancelTurn {},
    /// Завершити run вузла: хост виконує `mt done`-семантику — fenced
    /// publish fact у main (мінорне розширення v4).
    DoneSession {},
    /// Пауза/відпустити: хост CAS-delete claim; журнал лишається в run ref
    /// базою відновлення (мінорне розширення v4).
    ReleaseSession {},

    // ── хост → клієнти ─────────────────────────────────────────────────────
    /// ЕФЕМЕРНА: не журналиться — журналиться `AgentTextDone`-агрегат.
    AgentTextDelta {
        text: String,
    },
    AgentTextDone {},
    ToolCall {
        call_id: String,
        name: String,
        args: Value,
    },
    ToolResult {
        call_id: String,
        ok: bool,
        summary: String,
    },
    ApprovalRequest {
        request_id: String,
        action: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        diff: Option<String>,
    },
    /// ЕФЕМЕРНА: лише relay/WS, ніколи в git; лише клієнтам з capability
    /// «preview». Несе лише `ref_id` — байти клієнт тягне окремим запитом.
    PreviewScreenshot {
        ref_id: String,
        mime: String,
    },
    FileChanged {
        path: String,
    },
    Committed {
        commit_hash: String,
        message: String,
    },
    /// Derived-стан вузла — і для сесії, і для `mt-dashboard`.
    NodeState {
        path: String,
        state: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        claim: Option<ClaimInfo>,
    },
    /// Транслюється relay-ем; джерело істини — git ref.
    ClaimChanged {
        node_hash: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        holder_device_id: Option<Uuid>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        lease_until: Option<DateTime<Utc>>,
        generation: u64,
    },
    /// `role: None` = учасника видалено.
    MemberChanged {
        account_id: Uuid,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        role: Option<String>,
    },
    /// Composite-план чекає approve.
    PlanReview {
        plan_ref: String,
    },
    /// Fact чекає вердикту людини-аудитора.
    AuditPending {
        fact_ref: String,
    },
    /// Ескалація «вгору»: записка власника гілки замовникові вузла
    /// (owner-app, спека 260714). `from`/`to` — handles, як у git-файлах
    /// (`escalation_NNN.md`); `to_account_id` резолвиться емітером через
    /// git-ignored `.mt/directory.json` (PII у стрічку не тече — account_id
    /// непрозорий) і потрібен relay для адресного push «потребує уваги».
    Escalation {
        from: String,
        to: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        to_account_id: Option<Uuid>,
        /// Шлях записки у теці вузла, напр. `escalation_001.md`.
        reason_ref: String,
    },
    Error {
        message: String,
    },

    /// Невідомий варіант сумісної версії — клієнт ігнорує. Хости цей
    /// варіант ніколи не надсилають.
    #[serde(other)]
    Unknown,
}

/// Serde-хелпер: `signature: bytes` як base64-рядок у JSON.
mod base64_bytes {
    use super::*;

    pub fn serialize<S: Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&BASE64.encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
        let encoded = String::deserialize(deserializer)?;
        BASE64.decode(encoded).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn envelope(event: Event) -> Envelope {
        Envelope {
            seq: 7,
            ts: DateTime::parse_from_rfc3339("2026-07-11T12:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
            node_hash: "a".repeat(20),
            run_token: Uuid::from_u128(42),
            device_id: Some(Uuid::from_u128(1)),
            account_id: None,
            event,
        }
    }

    /// Roundtrip serde на КОЖЕН варіант Event з runtime.md.
    #[test]
    fn roundtrip_every_event_variant() {
        let variants = vec![
            Event::UserMessage {
                text: "зроби прев'ю".into(),
                attachments: vec![serde_json::json!({"path": "logo.svg"})],
                surface: Some("designer".into()),
            },
            Event::ContextSelected {
                kind: "dom_element".into(),
                payload: serde_json::json!({"selector": "#hero"}),
                bounding_box: Some(Rect {
                    x: 1.0,
                    y: 2.0,
                    width: 30.0,
                    height: 40.0,
                }),
            },
            Event::ApprovalResponse {
                request_id: "req-1".into(),
                approved: true,
                signature: vec![0xAB; 64],
            },
            Event::CancelTurn {},
            Event::DoneSession {},
            Event::ReleaseSession {},
            Event::AgentTextDelta {
                text: "част".into(),
            },
            Event::AgentTextDone {},
            Event::ToolCall {
                call_id: "c1".into(),
                name: "bash".into(),
                args: serde_json::json!({"command": "ls"}),
            },
            Event::ToolResult {
                call_id: "c1".into(),
                ok: true,
                summary: "2 файли".into(),
            },
            Event::ApprovalRequest {
                request_id: "req-1".into(),
                action: "git push origin main".into(),
                diff: Some("+1 -1".into()),
            },
            Event::PreviewScreenshot {
                ref_id: "shot-9".into(),
                mime: "image/png".into(),
            },
            Event::FileChanged {
                path: "src/app.vue".into(),
            },
            Event::Committed {
                commit_hash: "deadbeef".into(),
                message: "fix: hero".into(),
            },
            Event::NodeState {
                path: "mt/demo".into(),
                state: "running".into(),
                claim: Some(ClaimInfo {
                    holder_device: Uuid::from_u128(2),
                    lease_until: DateTime::parse_from_rfc3339("2026-07-11T13:00:00Z")
                        .unwrap()
                        .with_timezone(&Utc),
                    generation: 3,
                }),
            },
            Event::ClaimChanged {
                node_hash: "b".repeat(20),
                holder_device_id: None,
                lease_until: None,
                generation: 4,
            },
            Event::MemberChanged {
                account_id: Uuid::from_u128(5),
                role: None,
            },
            Event::PlanReview {
                plan_ref: "refs/mt/runs/x/plan".into(),
            },
            Event::AuditPending {
                fact_ref: "refs/mt/runs/x/fact".into(),
            },
            Event::Escalation {
                from: "olena".into(),
                to: "vkozlov".into(),
                to_account_id: Some(Uuid::from_u128(6)),
                reason_ref: "escalation_001.md".into(),
            },
            Event::Error {
                message: "boom".into(),
            },
        ];
        for event in variants {
            let original = envelope(event);
            let json = serde_json::to_string(&original).unwrap();
            let parsed: Envelope = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, original, "roundtrip зламано: {json}");
        }
    }

    /// Невідомий tag сумісної версії → `Event::Unknown` (ігнорується), а не помилка.
    #[test]
    fn unknown_variant_deserializes_to_unknown() {
        let json = r#"{"type": "SomeFutureEvent", "anything": 1}"#;
        let event: Event = serde_json::from_str(json).unwrap();
        assert_eq!(event, Event::Unknown);
    }

    /// Підпис їде як base64-рядок, не масив байтів.
    #[test]
    fn signature_serializes_as_base64_string() {
        let event = Event::ApprovalResponse {
            request_id: "req-1".into(),
            approved: false,
            signature: vec![1, 2, 3],
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["signature"], serde_json::json!("AQID"));
    }
}
