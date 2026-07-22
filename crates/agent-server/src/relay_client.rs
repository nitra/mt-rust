//! Міст до relay — транспорт (в) із runtime.md: хост тримає вихідне
//! WS-зʼєднання до relay і ретранслює кімнату задачі.
//!
//! Вихідний напрям: broadcast сесій хоста → `{kind:"envelope", root,
//! envelope}` у relay (віддалені тонкі клієнти бачать стрічку). Вхідний:
//! кадри `!from_host` (клієнтські події віддалених пристроїв) → штатна
//! обробка кадру клієнта. `from_host` ставить relay за роллю пристрою —
//! host-ехо, що повертається, міст ігнорує (анти-цикл). Reconnect із
//! експоненційним backoff; після реконекту стрічка цілісна через журнал
//! сесій (реплей — обовʼязок клієнтів, не relay).

use std::sync::Arc;
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Message;
use uuid::Uuid;

use crate::ws::{handle_client_frame, AppState};

/// Конфіг моста до relay.
#[derive(Debug, Clone)]
pub struct RelayBridgeConfig {
    /// Адреса relay (`ws://…` dev / `wss://…` прод).
    pub url: String,
    /// device_token host-пристрою, зареєстрованого на relay.
    pub device_token: String,
    /// Кімната = кореневий вузол задачі (access.md).
    pub root: String,
}

/// Стартує міст у фоні; жиє до аборту хоста, падіння зʼєднання лікує
/// reconnect-ом (backoff 1s → 30s).
pub fn spawn_relay_bridge(state: Arc<AppState>, config: RelayBridgeConfig) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut backoff = 1u64;
        loop {
            if run_bridge(&state, &config).await.is_ok() {
                backoff = 1;
            }
            tokio::time::sleep(Duration::from_secs(backoff)).await;
            backoff = (backoff * 2).min(30);
        }
    })
}

/// Одна сесія зʼєднання: hello → subscribe → двонаправлена ретрансляція.
async fn run_bridge(state: &Arc<AppState>, config: &RelayBridgeConfig) -> Result<(), String> {
    let (mut ws, _) = tokio_tungstenite::connect_async(&config.url)
        .await
        .map_err(|error| error.to_string())?;

    let hello = json!({ "kind": "hello", "device_token": config.device_token });
    ws.send(Message::text(hello.to_string()))
        .await
        .map_err(|error| error.to_string())?;
    let subscribe = json!({ "kind": "subscribe", "root": config.root });
    ws.send(Message::text(subscribe.to_string()))
        .await
        .map_err(|error| error.to_string())?;
    // Pubkey-кеш для перевірки підписів approvals (access.md); разом із
    // ним гейт вмикає require_signed.
    let pubkeys_request = json!({ "kind": "pubkeys", "root": config.root });
    ws.send(Message::text(pubkeys_request.to_string()))
        .await
        .map_err(|error| error.to_string())?;

    let mut updates = state.sessions.subscribe();
    loop {
        tokio::select! {
            update = updates.recv() => match update {
                Ok(envelope) => {
                    let frame = json!({
                        "kind": "envelope",
                        "root": config.root,
                        "envelope": serde_json::to_value(&envelope).unwrap(),
                    });
                    ws.send(Message::text(frame.to_string()))
                        .await
                        .map_err(|error| error.to_string())?;
                }
                // Випали з буфера — віддалені клієнти доберуть реплеєм у хоста.
                Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(broadcast::error::RecvError::Closed) => return Ok(()),
            },
            incoming = ws.next() => match incoming {
                Some(Ok(Message::Text(text))) => {
                    handle_incoming(state, text.as_str()).await;
                }
                Some(Ok(Message::Close(_))) | None => return Ok(()),
                Some(Ok(_)) => {}
                Some(Err(error)) => return Err(error.to_string()),
            },
        }
    }
}

/// Вхідний кадр relay: обробляємо лише envelope БЕЗ `from_host`
/// (клієнтські події віддалених пристроїв); host-ехо і службові кадри
/// (`ok`/`error`) ігноруються.
async fn handle_incoming(state: &Arc<AppState>, text: &str) {
    let Ok(frame) = serde_json::from_str::<Value>(text) else {
        return;
    };
    match frame.get("kind").and_then(Value::as_str) {
        Some("pubkeys") => {
            update_pubkeys(state, &frame);
            return;
        }
        Some("envelope") => {}
        _ => return,
    }
    if frame
        .get("from_host")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return;
    }
    let Some(envelope) = frame.get("envelope") else {
        return;
    };
    let device_id = envelope
        .get("device_id")
        .and_then(Value::as_str)
        .and_then(|raw| Uuid::parse_str(raw).ok());
    // Окрема задача: хід агента може чекати ApprovalResponse із relay —
    // інлайн-обробка заблокувала б читання наступних кадрів (deadlock).
    let state = Arc::clone(state);
    let raw_envelope = envelope.to_string();
    tokio::spawn(async move {
        handle_client_frame(&state, &raw_envelope, device_id).await;
    });
}

/// Кадр `pubkeys` від relay → оновлення pubkey-кешу гейту approvals.
/// `pubkey` — hex 32-байтового Ed25519 ключа; непарсибельні записи
/// пропускаються (пристрої без валідного ключа не можуть підписувати).
fn update_pubkeys(state: &Arc<AppState>, frame: &Value) {
    let Some(list) = frame.get("pubkeys").and_then(Value::as_array) else {
        return;
    };
    let keys = list
        .iter()
        .filter_map(|entry| {
            let device_id = entry
                .get("device_id")
                .and_then(Value::as_str)
                .and_then(|raw| Uuid::parse_str(raw).ok())?;
            let hex = entry.get("pubkey").and_then(Value::as_str)?;
            let bytes = decode_hex_32(hex)?;
            let key = agent_protocol::VerifyingKey::from_bytes(&bytes).ok()?;
            Some((device_id, key))
        })
        .collect();
    state.approvals.set_pubkeys(keys);
}

/// Hex → 32 байти (Ed25519 pubkey); інша довжина/не-hex → None.
fn decode_hex_32(hex: &str) -> Option<[u8; 32]> {
    if hex.len() != 64 {
        return None;
    }
    let mut bytes = [0u8; 32];
    for (index, chunk) in hex.as_bytes().chunks(2).enumerate() {
        let high = (chunk[0] as char).to_digit(16)?;
        let low = (chunk[1] as char).to_digit(16)?;
        bytes[index] = ((high << 4) | low) as u8;
    }
    Some(bytes)
}
