//! Інтеграційні тести WS-транспорту: реальний сервер на ефемерному порту,
//! tungstenite-клієнт, хід через скриптований runner (офлайн).

use std::sync::Arc;

use agent_protocol::{ClientHello, Envelope, Event, ServerHello, PROTOCOL_VERSION};
use agent_server::{serve, AppState, ScriptedTurnRunner, SessionHost};
use chrono::Utc;
use futures::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;
use uuid::Uuid;

type WsStream =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// Сервер зі скриптованим runner-ом: на кожен хід — наступний текст.
async fn start_server(dir: &tempfile::TempDir, responses: Vec<&str>) -> String {
    let runner = ScriptedTurnRunner::new(responses);
    let state = Arc::new(AppState::new(
        SessionHost::new(dir.path().to_path_buf()).unwrap(),
        Arc::new(runner),
        Some("test-token".into()),
    ));
    let (addr, _handle) = serve(state, "127.0.0.1:0".parse().unwrap()).await.unwrap();
    format!("ws://{addr}/ws")
}

fn hello(version: u32, replay_from: Option<u64>) -> ClientHello {
    ClientHello {
        protocol_version: version,
        device_id: Uuid::from_u128(7),
        device_token: "test-token".into(),
        client_kind: "cli".into(),
        client_capabilities: vec!["approvals".into()],
        lang: "uk".into(),
        want_replay_from: replay_from,
    }
}

async fn connect(url: &str, hello_frame: &ClientHello) -> WsStream {
    let (mut stream, _) = tokio_tungstenite::connect_async(url).await.unwrap();
    stream
        .send(Message::text(serde_json::to_string(hello_frame).unwrap()))
        .await
        .unwrap();
    stream
}

async fn next_json<T: serde::de::DeserializeOwned>(stream: &mut WsStream) -> T {
    loop {
        let message = tokio::time::timeout(std::time::Duration::from_secs(5), stream.next())
            .await
            .expect("timeout очікування кадру")
            .expect("стрім закрито")
            .unwrap();
        if let Message::Text(text) = message {
            return serde_json::from_str(text.as_str()).unwrap();
        }
    }
}

fn user_message(node: &str, text: &str) -> Message {
    let envelope = Envelope {
        seq: 0,
        ts: Utc::now(),
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

/// Повний хід: хендшейк → UserMessage → стрічка подій ходу з
/// монотонними seq, які призначає хост.
#[tokio::test]
async fn handshake_turn_and_event_stream() {
    let dir = tempfile::tempdir().unwrap();
    let url = start_server(&dir, vec!["відповідь агента"]).await;
    let mut stream = connect(&url, &hello(PROTOCOL_VERSION, None)).await;

    let server_hello: ServerHello = next_json(&mut stream).await;
    assert_eq!(server_hello.protocol_version, PROTOCOL_VERSION);

    stream.send(user_message("demo", "питання")).await.unwrap();

    let user: Envelope = next_json(&mut stream).await;
    let delta: Envelope = next_json(&mut stream).await;
    let done: Envelope = next_json(&mut stream).await;

    assert_eq!(
        user.event,
        Event::UserMessage {
            text: "питання".into(),
            attachments: vec![],
            surface: None
        }
    );
    assert_eq!(
        user.device_id,
        Some(Uuid::from_u128(7)),
        "адресація від ClientHello"
    );
    assert_eq!(
        delta.event,
        Event::AgentTextDelta {
            text: "відповідь агента".into()
        }
    );
    assert_eq!(done.event, Event::AgentTextDone {});
    assert_eq!(
        (user.seq, delta.seq, done.seq),
        (0, 1, 2),
        "seq призначає хост, монотонно"
    );
}

/// Несумісна версія протоколу → Error із підказкою, стрім закривається.
#[tokio::test]
async fn incompatible_version_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let url = start_server(&dir, vec![]).await;
    let mut stream = connect(&url, &hello(3, None)).await;

    let error: Event = next_json(&mut stream).await;
    assert!(
        matches!(error, Event::Error { ref message } if message.contains("v3") && message.contains("v4")),
        "{error:?}"
    );
    let next = stream.next().await;
    assert!(
        matches!(next, None | Some(Ok(Message::Close(_)))),
        "після відмови зʼєднання закрито, отримано: {next:?}"
    );
}

/// Невірний device token → відмова на хендшейку.
#[tokio::test]
async fn invalid_token_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let url = start_server(&dir, vec![]).await;
    let mut bad = hello(PROTOCOL_VERSION, None);
    bad.device_token = "чужий".into();
    let mut stream = connect(&url, &bad).await;

    let error: Event = next_json(&mut stream).await;
    assert!(matches!(error, Event::Error { ref message } if message.contains("token")));
}

/// Реконект із want_replay_from: журнальовані події доїжджають повторно
/// (ефемерні дельти — ні), розмова продовжується тим самим run-ом.
#[tokio::test]
async fn reconnect_replays_journaled_events() {
    let dir = tempfile::tempdir().unwrap();
    let url = start_server(&dir, vec!["перша", "друга"]).await;

    let mut first = connect(&url, &hello(PROTOCOL_VERSION, None)).await;
    let _: ServerHello = next_json(&mut first).await;
    first.send(user_message("demo", "раз")).await.unwrap();
    let _user: Envelope = next_json(&mut first).await;
    let _delta: Envelope = next_json(&mut first).await;
    let done: Envelope = next_json(&mut first).await;
    drop(first); // «закрив ноутбук»

    let mut second = connect(&url, &hello(PROTOCOL_VERSION, Some(0))).await;
    let server_hello: ServerHello = next_json(&mut second).await;
    assert_eq!(server_hello.session_list.len(), 1);
    assert_eq!(server_hello.session_list[0].node_hash, "demo");

    // Реплей: UserMessage + AgentTextDone (дельта ефемерна — не журналиться).
    let replay_user: Envelope = next_json(&mut second).await;
    let replay_done: Envelope = next_json(&mut second).await;
    assert!(matches!(replay_user.event, Event::UserMessage { .. }));
    assert_eq!(replay_done.event, Event::AgentTextDone {});
    assert_eq!(replay_done.seq, done.seq, "той самий журнал, ті самі seq");

    // Розмова продовжується після відновлення.
    second.send(user_message("demo", "два")).await.unwrap();
    let user: Envelope = next_json(&mut second).await;
    assert_eq!(user.seq, done.seq + 1, "seq продовжується без розривів");
    assert_eq!(user.run_token, replay_done.run_token, "той самий run");
}
