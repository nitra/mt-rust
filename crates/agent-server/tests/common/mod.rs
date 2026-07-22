//! Спільні WS-хелпери інтеграційних тестів agent-server: підключення
//! клієнта (ClientHello → ServerHello) і читання кадрів — спільний код
//! тестових бінарників graph_wiring і handoff_ws.

use agent_protocol::{ClientHello, ServerHello, PROTOCOL_VERSION};
use futures::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;
use uuid::Uuid;

pub type WsStream =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// Підключає WS-клієнта: шле ClientHello, чекає ServerHello, повертає стрім.
/// `device_id` розрізняє клієнтів у сценаріях із кількома підключеннями.
pub async fn connect(url: &str, device_id: u128) -> WsStream {
    let hello = ClientHello {
        protocol_version: PROTOCOL_VERSION,
        device_id: Uuid::from_u128(device_id),
        device_token: String::new(),
        client_kind: "cli".into(),
        client_capabilities: vec![],
        lang: "uk".into(),
        want_replay_from: None,
    };
    let (mut stream, _) = tokio_tungstenite::connect_async(url).await.unwrap();
    stream
        .send(Message::text(serde_json::to_string(&hello).unwrap()))
        .await
        .unwrap();
    let _: ServerHello = next_json(&mut stream).await;
    stream
}

/// Наступний текстовий кадр стріму як десеріалізований `T` (таймаут 10 с).
pub async fn next_json<T: serde::de::DeserializeOwned>(stream: &mut WsStream) -> T {
    loop {
        let message = tokio::time::timeout(std::time::Duration::from_secs(10), stream.next())
            .await
            .expect("timeout очікування кадру")
            .expect("стрім закрито")
            .unwrap();
        if let Message::Text(text) = message {
            return serde_json::from_str(text.as_str()).unwrap();
        }
    }
}
