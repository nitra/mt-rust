//! Surface-профілі через WS (спека `surfaces.md`): резолюція режиму ходу
//! (hint → липкість → default за `client_kind`) і гейт `context_kinds`.

use std::sync::Arc;

use agent_protocol::{Envelope, Event};
use agent_server::{serve, AppState, EchoTurnRunner, SessionHost};
use futures::SinkExt;
use tokio_tungstenite::tungstenite::Message;
use uuid::Uuid;

mod common;
use common::{next_json, WsStream};

/// Хост із двома профілями: `writer` розуміє лише `text_range`.
async fn start() -> (String, tempfile::TempDir) {
    let config = serde_json::json!({
        "surface_profiles": {
            "writer": {"agent_cli": "codex", "context_kinds": ["text_range"]},
            "designer": {"agent_cli": "pi", "context_kinds": ["dom_element", "file_region"]}
        }
    });
    let state_dir = tempfile::tempdir().unwrap();
    let state = Arc::new(
        AppState::new(
            SessionHost::new(state_dir.path().to_path_buf()).unwrap(),
            Arc::new(EchoTurnRunner),
            None,
        )
        .with_surface_profiles(&config),
    );
    let (addr, _handle) = serve(Arc::clone(&state), "127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    (format!("ws://{addr}/ws"), state_dir)
}

fn envelope(node: &str, event: Event) -> Message {
    let envelope = Envelope {
        seq: 0,
        ts: chrono::Utc::now(),
        node_hash: node.into(),
        run_token: Uuid::from_u128(1),
        device_id: None,
        account_id: None,
        event,
    };
    Message::text(serde_json::to_string(&envelope).unwrap())
}

fn user(text: &str, surface: Option<&str>) -> Event {
    Event::UserMessage {
        text: text.into(),
        attachments: vec![],
        surface: surface.map(str::to_string),
    }
}

async fn next_user_surface(stream: &mut WsStream) -> Option<String> {
    loop {
        let envelope: Envelope = next_json(stream).await;
        if let Event::UserMessage { surface, .. } = envelope.event {
            return surface;
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn surface_is_sticky_until_next_hint() {
    let (url, _dir) = start().await;
    let mut client = common::connect_as(&url, 3, "cli").await;

    client
        .send(envelope("demo", user("перепиши абзац", Some("writer"))))
        .await
        .unwrap();
    assert_eq!(
        next_user_surface(&mut client).await.as_deref(),
        Some("writer")
    );

    // Без hint режим не злітає — інакше кожне повідомлення без позначки
    // повертало б сесію в default.
    client
        .send(envelope("demo", user("ще раз", None)))
        .await
        .unwrap();
    assert_eq!(
        next_user_surface(&mut client).await.as_deref(),
        Some("writer")
    );

    // Новий hint перемикає режим усередині тієї ж сесії.
    client
        .send(envelope("demo", user("а тепер макет", Some("designer"))))
        .await
        .unwrap();
    assert_eq!(
        next_user_surface(&mut client).await.as_deref(),
        Some("designer")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn context_kind_outside_profile_is_rejected_not_dropped() {
    // Найгірший варіант — мовчазна втрата: клієнт вважає, що контекст
    // доїхав, агент його не бачить, і розходження нічим не видно.
    let (url, _dir) = start().await;
    let mut client = common::connect_as(&url, 4, "cli").await;

    client
        .send(envelope("demo", user("текст", Some("writer"))))
        .await
        .unwrap();
    next_user_surface(&mut client).await;

    client
        .send(envelope(
            "demo",
            Event::ContextSelected {
                kind: "dom_element".into(),
                payload: serde_json::json!({}),
                bounding_box: None,
            },
        ))
        .await
        .unwrap();

    loop {
        let incoming: Envelope = next_json(&mut client).await;
        if let Event::Error { message } = incoming.event {
            assert!(message.contains("dom_element"), "{message}");
            assert!(message.contains("writer"), "{message}");
            return;
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn context_kind_inside_profile_passes() {
    let (url, _dir) = start().await;
    let mut client = common::connect_as(&url, 5, "cli").await;

    client
        .send(envelope("demo", user("макет", Some("designer"))))
        .await
        .unwrap();
    next_user_surface(&mut client).await;

    client
        .send(envelope(
            "demo",
            Event::ContextSelected {
                kind: "dom_element".into(),
                payload: serde_json::json!({}),
                bounding_box: None,
            },
        ))
        .await
        .unwrap();

    // Наступний хід іде штатно — відмови не було.
    client
        .send(envelope("demo", user("далі", None)))
        .await
        .unwrap();
    assert_eq!(
        next_user_surface(&mut client).await.as_deref(),
        Some("designer")
    );
}
