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
///
/// Модуль спільний для кількох тестових бінарників, і кожен використовує
/// свій підмножину хелперів — звідси `allow(dead_code)`.
#[allow(dead_code)]
pub async fn connect(url: &str, device_id: u128) -> WsStream {
    connect_as(url, device_id, "cli").await
}

/// Те саме, але з явним `client_kind` — для `mt-dashboard`, який отримує
/// інший зріз стрічки.
#[allow(dead_code)]
pub async fn connect_as(url: &str, device_id: u128, client_kind: &str) -> WsStream {
    let hello = ClientHello {
        protocol_version: PROTOCOL_VERSION,
        device_id: Uuid::from_u128(device_id),
        device_token: String::new(),
        client_kind: client_kind.into(),
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
