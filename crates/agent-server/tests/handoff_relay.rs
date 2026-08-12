//! Міграція сесії між хостами **через relay** (runtime.md, «Міграція сесії
//! між хостами», кроки 1-3): два незалежні `AppState` з власними мостами до
//! спільного mock-relay. Хост-1 тримає вузол, хост-2 просить «перенести
//! сюди» — і після `HandoffRequest`/`HandoffAck` вузол виконується на
//! хості-2 з успадкованим журналом.
//!
//! Це перевіряє саме те, чого не покривав `handoff_ws`: там тікет передавали
//! з рук у руки в самому тесті, тут він іде мережею, а обидві сторони —
//! хости, тобто наскрізь працює виняток з анти-циклу.

use std::sync::Arc;
use std::time::Duration;

use agent_protocol::{Envelope, Event};
use agent_server::{
    serve, spawn_relay_bridge, AppState, ApprovalGate, GraphConfig, RelayBridgeConfig,
    ScriptedTurnRunner, SessionHost,
};
use futures::{SinkExt, StreamExt};
use mt_core::test_support::{commit_all, push_head, TestRepo};
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tokio::sync::broadcast;
use tokio_tungstenite::tungstenite::Message;
use uuid::Uuid;

mod common;
use common::{next_json, WsStream};

/// Mock-relay на двох (і більше) підключень: кожен вхідний envelope-кадр
/// ретранслюється **всім іншим** із `from_host: true` — так робить реальний
/// relay для пристроїв ролі host (`clientEnvelope` у `relay/lib/relay.mjs`).
/// На `hello` віддає `{kind:"ok", device_id}` з унікальним id — саме за ним
/// міст відрізняє власне ехо.
async fn mock_relay() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (bus, _) = broadcast::channel::<(u64, Value)>(64);

    tokio::spawn(async move {
        let mut next_peer = 0u64;
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                break;
            };
            next_peer += 1;
            let peer = next_peer;
            let bus = bus.clone();
            tokio::spawn(async move {
                let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
                let mut room = bus.subscribe();
                loop {
                    tokio::select! {
                        incoming = ws.next() => match incoming {
                            Some(Ok(Message::Text(text))) => {
                                let frame: Value = serde_json::from_str(text.as_str()).unwrap();
                                match frame.get("kind").and_then(Value::as_str) {
                                    Some("hello") => {
                                        let ack = json!({
                                            "kind": "ok",
                                            "device_id": format!("00000000-0000-0000-0000-{peer:012}"),
                                        });
                                        if ws.send(Message::text(ack.to_string())).await.is_err() {
                                            break;
                                        }
                                    }
                                    Some("envelope") => {
                                        let _ = bus.send((peer, frame));
                                    }
                                    _ => {}
                                }
                            }
                            Some(Ok(_)) => {}
                            _ => break,
                        },
                        relayed = room.recv() => match relayed {
                            Ok((from, mut frame)) => {
                                if from == peer {
                                    continue;
                                }
                                frame["from_host"] = Value::Bool(true);
                                if ws.send(Message::text(frame.to_string())).await.is_err() {
                                    break;
                                }
                            }
                            Err(broadcast::error::RecvError::Lagged(_)) => {}
                            Err(broadcast::error::RecvError::Closed) => break,
                        },
                    }
                }
            });
        }
    });

    format!("ws://127.0.0.1:{port}")
}

/// Bare-origin + робочий клон із вузлом `mt/demo`.
fn fixture() -> (tempfile::TempDir, tempfile::TempDir) {
    let TestRepo { origin, work } = TestRepo::new();
    std::fs::create_dir_all(work.path().join("mt/demo")).unwrap();
    std::fs::write(work.path().join("mt/demo/task.md"), "## Task\n").unwrap();
    commit_all(work.path(), "add task");
    push_head(work.path(), "refs/heads/main");
    (origin, work)
}

/// WS-клієнт до локального хоста.
async fn connect(url: &str) -> WsStream {
    common::connect(url, 1).await
}

/// Хост із мостом до relay; `state_dir` тримаємо, щоб tempdir не зникла.
async fn start_host(
    tasks_dir: std::path::PathBuf,
    relay_url: &str,
    responses: Vec<&str>,
) -> (Arc<AppState>, String, tempfile::TempDir) {
    let state_dir = tempfile::tempdir().unwrap();
    let state = Arc::new(
        AppState::from_parts(
            Arc::new(SessionHost::new(state_dir.path().to_path_buf()).unwrap()),
            Arc::new(ApprovalGate::default()),
            Arc::new(ScriptedTurnRunner::new(responses)),
            None,
        )
        .with_graph(GraphConfig::new(tasks_dir)),
    );
    spawn_relay_bridge(
        Arc::clone(&state),
        RelayBridgeConfig {
            url: relay_url.to_string(),
            device_token: "тест".into(),
            root: "mt/demo".into(),
        },
    );
    let (addr, _handle) = serve(Arc::clone(&state), "127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    (state, format!("ws://{addr}/ws"), state_dir)
}

/// Кадр `UserMessage` для локального WS хоста.
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

/// Чекає подію заданого виду у стрічці клієнта.
async fn next_matching(stream: &mut WsStream, matches_event: impl Fn(&Event) -> bool) -> Envelope {
    loop {
        let envelope: Envelope = next_json(stream).await;
        if matches_event(&envelope.event) {
            return envelope;
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn handoff_request_migrates_node_between_hosts() {
    let (_origin, work) = fixture();
    let relay_url = mock_relay().await;

    let (host1, url1, _dir1) =
        start_host(work.path().join("mt"), &relay_url, vec!["перший хост"]).await;
    let (host2, _url2, _dir2) =
        start_host(work.path().join("mt"), &relay_url, vec!["другий хост"]).await;
    // Мости підключаються асинхронно; без цього HandoffRequest пішов би в
    // кімнату, якої ще ніхто не слухає.
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Хост-1 бере вузол ходом — саме його журнал має переїхати.
    let mut client1 = connect(&url1).await;
    client1.send(user_message("demo", "почни")).await.unwrap();
    let first_user = next_matching(&mut client1, |e| matches!(e, Event::UserMessage { .. })).await;
    next_matching(&mut client1, |e| matches!(e, Event::AgentTextDone {})).await;
    drop(client1);
    assert!(host1.holds_node("demo").await, "хост-1 мав узяти вузол");

    // Хост-2 просить «перенести сюди» — запит і ack ідуть через relay.
    host2
        .pull_node("demo", Duration::from_secs(20))
        .await
        .expect("handoff мав пройти через relay");

    assert!(
        !host1.holds_node("demo").await,
        "хост-1 мусив відпустити вузол"
    );
    assert!(
        host2.holds_node("demo").await,
        "хост-2 мусив узяти вузол під облік"
    );

    // Журнал успадкований: подія хоста-1 є в сесії хоста-2.
    let inherited = host2.sessions.get_or_open("demo").unwrap().replay_from(0);
    assert!(
        inherited
            .iter()
            .any(|e| e.event == first_user.event && e.seq == first_user.seq),
        "{inherited:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn handoff_without_holder_times_out_with_takeover_hint() {
    // Тримача немає взагалі: спека на цей випадок не має відмови — вона має
    // інший шлях (lease expiry + grace takeover), і помилка мусить на нього
    // вказувати, а не мовчки висіти.
    let (_origin, work) = fixture();
    let relay_url = mock_relay().await;
    let (host, _url, _dir) = start_host(work.path().join("mt"), &relay_url, vec![]).await;
    tokio::time::sleep(Duration::from_millis(300)).await;

    let error = host
        .pull_node("demo", Duration::from_millis(500))
        .await
        .expect_err("без тримача handoff не може завершитись успіхом");
    assert!(error.contains("takeover"), "{error}");
}
