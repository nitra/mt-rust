//! `client_kind: mt-dashboard` (runtime.md, «`mt-dashboard`»): дашборд
//! бачить граф як одну картину — лише події стану вузлів, без чат-стріму.
//!
//! Перевіряється не «клієнт відсіє зайве», а що хост **не надсилає**
//! чат-стрім дашборду: інакше кожен дашборд тягнув би дельти тексту всіх
//! ходів, щоб їх викинути.

use std::sync::Arc;

use agent_protocol::{Envelope, Event};
use agent_server::{serve, AppState, EchoTurnRunner, SessionHost};
use futures::SinkExt;
use tokio_tungstenite::tungstenite::Message;
use uuid::Uuid;

mod common;
use common::{next_json, WsStream};

/// Піднімає хост і повертає URL + стан.
async fn start() -> (Arc<AppState>, String, tempfile::TempDir) {
    let state_dir = tempfile::tempdir().unwrap();
    let state = Arc::new(AppState::new(
        SessionHost::new(state_dir.path().to_path_buf()).unwrap(),
        Arc::new(EchoTurnRunner),
        None,
    ));
    let (addr, _handle) = serve(Arc::clone(&state), "127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    (state, format!("ws://{addr}/ws"), state_dir)
}

fn user_message(node: &str, text: &str) -> Message {
    let envelope = Envelope {
        seq: 0,
        ts: chrono::Utc::now(),
        node_hash: node.into(),
        run_token: Uuid::from_u128(1),
        device_id: None,
        account_id: None,
        event: Event::UserMessage {
            text: text.into(),
            attachments: vec![],
            surface: None,
        },
    };
    Message::text(serde_json::to_string(&envelope).unwrap())
}

async fn next_matching(stream: &mut WsStream, matches: impl Fn(&Event) -> bool) -> Envelope {
    loop {
        let envelope: Envelope = next_json(stream).await;
        if matches(&envelope.event) {
            return envelope;
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn dashboard_sees_graph_events_without_chat_stream() {
    let (state, url, _dir) = start().await;

    let mut dashboard = common::connect_as(&url, 7, "mt-dashboard").await;
    let mut cli = common::connect_as(&url, 8, "cli").await;

    // Хід агента: у стрічці зʼявляються UserMessage, дельти тексту, Done.
    cli.send(user_message("demo", "привіт")).await.unwrap();
    next_matching(&mut cli, |e| matches!(e, Event::AgentTextDone {})).await;

    // Слідом — графова подія. Дашборд мусить отримати саме її першою:
    // чат-стрім до нього не доїхав узагалі.
    let session = state.sessions.get_or_open("demo").unwrap();
    state.sessions.publish(
        &session,
        Event::NodeState {
            path: "demo".into(),
            state: "running".into(),
            claim: None,
        },
        None,
        None,
    );

    let first: Envelope = next_json(&mut dashboard).await;
    assert!(
        matches!(first.event, Event::NodeState { .. }),
        "дашборд отримав чат-стрім: {:?}",
        first.event
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn cli_client_still_gets_full_stream() {
    // Фільтр не має протікати на звичайних клієнтів.
    let (_state, url, _dir) = start().await;
    let mut cli = common::connect_as(&url, 9, "cli").await;
    cli.send(user_message("demo", "привіт")).await.unwrap();
    next_matching(&mut cli, |e| matches!(e, Event::AgentTextDelta { .. })).await;
}
