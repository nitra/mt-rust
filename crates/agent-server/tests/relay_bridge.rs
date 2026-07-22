//! Міст agent-server ↔ relay проти mock-relay (tungstenite-сервер у тесті,
//! кадровий протокол relay): віддалений UserMessage → хід агента →
//! host-кадри доїжджають у relay; host-ехо назад — без зациклення.

use std::sync::Arc;

use agent_server::{spawn_relay_bridge, AppState, EchoTurnRunner, RelayBridgeConfig, SessionHost};
use futures::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

/// Mock-relay: одне зʼєднання; всі отримані кадри — у канал тесту,
/// кадри з каналу тесту — мосту.
async fn mock_relay() -> (String, mpsc::Receiver<Value>, mpsc::Sender<Value>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (received_tx, received_rx) = mpsc::channel::<Value>(64);
    let (outgoing_tx, mut outgoing_rx) = mpsc::channel::<Value>(64);

    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
        loop {
            tokio::select! {
                incoming = ws.next() => match incoming {
                    Some(Ok(Message::Text(text))) => {
                        let frame: Value = serde_json::from_str(text.as_str()).unwrap();
                        let _ = received_tx.send(frame).await;
                    }
                    Some(Ok(_)) => {}
                    _ => break,
                },
                outgoing = outgoing_rx.recv() => match outgoing {
                    Some(frame) => {
                        if ws.send(Message::text(frame.to_string())).await.is_err() {
                            break;
                        }
                    }
                    None => break,
                },
            }
        }
    });

    (format!("ws://127.0.0.1:{port}"), received_rx, outgoing_tx)
}

/// Наступний кадр від моста з таймаутом.
async fn next_frame(rx: &mut mpsc::Receiver<Value>) -> Value {
    tokio::time::timeout(std::time::Duration::from_secs(10), rx.recv())
        .await
        .expect("timeout очікування кадру від моста")
        .expect("канал закрито")
}

fn remote_user_message(text: &str) -> Value {
    json!({
        "kind": "envelope",
        "envelope": {
            "seq": 0,
            "ts": "2026-07-12T00:00:00Z",
            "node_hash": "demo",
            "run_token": "00000000-0000-0000-0000-000000000001",
            "device_id": "00000000-0000-0000-0000-00000000000a",
            "event": { "type": "UserMessage", "text": text, "attachments": [] }
        }
    })
}

#[tokio::test(flavor = "multi_thread")]
async fn remote_user_message_runs_turn_and_streams_back() {
    let state_dir = tempfile::tempdir().unwrap();
    let state = Arc::new(AppState::new(
        SessionHost::new(state_dir.path().to_path_buf()).unwrap(),
        Arc::new(EchoTurnRunner),
        None,
    ));
    let (url, mut received, outgoing) = mock_relay().await;
    let _bridge = spawn_relay_bridge(
        Arc::clone(&state),
        RelayBridgeConfig {
            url,
            device_token: "host-token".into(),
            root: "demo".into(),
        },
    );

    // Хендшейк моста: hello з device_token → subscribe кімнати.
    let hello = next_frame(&mut received).await;
    assert_eq!(hello["kind"], "hello");
    assert_eq!(hello["device_token"], "host-token");
    let subscribe = next_frame(&mut received).await;
    assert_eq!(subscribe["kind"], "subscribe");
    assert_eq!(subscribe["root"], "demo");
    let pubkeys_request = next_frame(&mut received).await;
    assert_eq!(pubkeys_request["kind"], "pubkeys");

    // Віддалений клієнт шле UserMessage через relay.
    outgoing.send(remote_user_message("привіт")).await.unwrap();

    // Міст ретранслює host-стрічку: echo UserMessage (seq призначив хост),
    // дельта відповіді агента, AgentTextDone.
    let user_echo = next_frame(&mut received).await;
    assert_eq!(user_echo["kind"], "envelope");
    assert_eq!(user_echo["envelope"]["event"]["type"], "UserMessage");
    assert_eq!(user_echo["envelope"]["seq"], 0);
    assert_eq!(
        user_echo["envelope"]["device_id"], "00000000-0000-0000-0000-00000000000a",
        "device_id віддаленого пристрою збережено"
    );
    let delta = next_frame(&mut received).await;
    assert_eq!(delta["envelope"]["event"]["type"], "AgentTextDelta");
    assert_eq!(delta["envelope"]["event"]["text"], "echo: привіт");
    let done = next_frame(&mut received).await;
    assert_eq!(done["envelope"]["event"]["type"], "AgentTextDone");

    // Анти-цикл: relay повертає host-ехо (from_host: true) — міст ігнорує;
    // у журналі сесії рівно один UserMessage.
    let mut echoed = user_echo.clone();
    echoed["from_host"] = json!(true);
    outgoing.send(echoed).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    let session = state.sessions.get_or_open("demo").unwrap();
    let user_messages = session
        .replay_from(0)
        .iter()
        .filter(|envelope| matches!(envelope.event, agent_protocol::Event::UserMessage { .. }))
        .count();
    assert_eq!(user_messages, 1, "host-ехо не мусить оброблятись повторно");
}

/// Кадр ApprovalResponse віддаленого пристрою (підпис — за протоколом).
fn remote_approval_response(
    request_id: &str,
    approved: bool,
    signature: Vec<u8>,
    device_id: uuid::Uuid,
) -> Value {
    let envelope = agent_protocol::Envelope {
        seq: 0,
        ts: chrono::Utc::now(),
        node_hash: "demo".into(),
        run_token: uuid::Uuid::nil(),
        device_id: Some(device_id),
        account_id: None,
        event: agent_protocol::Event::ApprovalResponse {
            request_id: request_id.into(),
            approved,
            signature,
        },
    };
    json!({ "kind": "envelope", "envelope": serde_json::to_value(&envelope).unwrap() })
}

/// Наскрізний approvals-потік (access.md): pubkeys з relay → ApprovalRequest
/// у кімнату → підписаний ApprovalResponse віддаленого пристрою → вердикт;
/// невалідний підпис → Error у стрічку, запит живий до валідної відповіді.
#[tokio::test(flavor = "multi_thread")]
async fn signed_approval_flow_via_relay() {
    let state_dir = tempfile::tempdir().unwrap();
    let state = Arc::new(AppState::new(
        SessionHost::new(state_dir.path().to_path_buf()).unwrap(),
        Arc::new(EchoTurnRunner),
        None,
    ));
    let (url, mut received, outgoing) = mock_relay().await;
    let _bridge = spawn_relay_bridge(
        Arc::clone(&state),
        RelayBridgeConfig {
            url,
            device_token: "host-token".into(),
            root: "demo".into(),
        },
    );
    // hello / subscribe / pubkeys-запит.
    for _ in 0..3 {
        next_frame(&mut received).await;
    }

    // Relay віддає pubkey телефона-approver-а (hex Ed25519).
    let phone_key = agent_protocol::SigningKey::from_bytes(&[5u8; 32]);
    let phone_device = uuid::Uuid::from_u128(0xF0);
    let pubkey_hex: String = phone_key
        .verifying_key()
        .to_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    outgoing
        .send(json!({
            "kind": "pubkeys",
            "root": "demo",
            "pubkeys": [{ "device_id": phone_device.to_string(), "pubkey": pubkey_hex }]
        }))
        .await
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // Хост просить approval деструктивної дії.
    let verdict = state
        .request_approval("demo", "git push origin main".into(), Some("+1 -1".into()))
        .unwrap();
    let request_frame = next_frame(&mut received).await;
    assert_eq!(
        request_frame["envelope"]["event"]["type"],
        "ApprovalRequest"
    );
    let request_id = request_frame["envelope"]["event"]["request_id"]
        .as_str()
        .unwrap()
        .to_string();
    let run_token: uuid::Uuid = request_frame["envelope"]["run_token"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();

    // Спершу — зіпсований підпис: Error у стрічці, вердикту немає.
    let payload = agent_protocol::ApprovalPayload {
        request_id: request_id.clone(),
        approved: true,
        node_hash: "demo".into(),
        run_token,
    };
    let mut corrupted = agent_protocol::sign_approval(&phone_key, &payload)
        .to_bytes()
        .to_vec();
    corrupted[7] ^= 0xFF;
    outgoing
        .send(remote_approval_response(
            &request_id,
            true,
            corrupted,
            phone_device,
        ))
        .await
        .unwrap();
    let error_frame = next_frame(&mut received).await;
    assert_eq!(error_frame["envelope"]["event"]["type"], "Error");

    // Валідний підпис завершує запит.
    let signature = agent_protocol::sign_approval(&phone_key, &payload)
        .to_bytes()
        .to_vec();
    outgoing
        .send(remote_approval_response(
            &request_id,
            true,
            signature,
            phone_device,
        ))
        .await
        .unwrap();
    let echoed = next_frame(&mut received).await;
    assert_eq!(
        echoed["envelope"]["event"]["type"], "ApprovalResponse",
        "верифікований вердикт журналюється в сесію"
    );
    assert_eq!(verdict.await, Ok(true));
}
