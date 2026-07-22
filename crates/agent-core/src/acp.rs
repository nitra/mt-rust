//! Мінімальний ACP-клієнт (Agent Client Protocol v1) — єдиний транспорт
//! AI-викликів (ADR `260713-2110`): JSON-RPC 2.0 поверх ndjson-стріму
//! (звичайно stdio дочірнього процесу ACP-адаптера підписочного CLI).
//!
//! Покрита підмножина, потрібна runner-у інтерактивних сесій:
//! `initialize` → `session/new` → `session/prompt`; нотифікації
//! `session/update` мапляться на `Event` agent-protocol
//! (`AgentTextDelta`/`ToolCall`/`ToolResult`); запит агента
//! `session/request_permission` іде у [`PermissionHandler`] — хост мапить
//! його на `ApprovalRequest` (Ed25519). Клієнт generic над потоками:
//! продакшн — stdio child-процесу, тести — `tokio::io::duplex`.
//!
//! Читання стріму — фоновий таск на весь час життя клієнта, не прив'язаний
//! до конкретного виклику (як у Zed): нотифікації, що приходять між
//! викликами (напр. деякі адаптери, зокрема `pi-acp`, шлють `agent_message_chunk`
//! з prelude-банером самого CLI одразу після `session/new`, ще до першого
//! prompt), не приліплюються механічно до наступної відповіді.

use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use agent_protocol::Event;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, Mutex as AsyncMutex};

/// Помилка ACP-транспорту/протоколу.
#[derive(Debug)]
pub struct AcpError(pub String);

impl fmt::Display for AcpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for AcpError {}

/// Обробник `session/request_permission`: `(action, diff) → approved`.
/// Хост підключає сюди approval-гейт (`ApprovalRequest` + підпис пристрою).
pub type PermissionHandler =
    Arc<dyn Fn(String, Option<String>) -> Pin<Box<dyn Future<Output = bool> + Send>> + Send + Sync>;

/// Скільки чекати на "осідання" нотифікацій одразу після відповіді на
/// `session/new`, перш ніж вважати чергу порожньою — щоб prelude-банер
/// адаптера не приліпився до першого `prompt`.
const SETTLE_TIMEOUT: Duration = Duration::from_millis(150);

/// Класифіковане повідомлення від фонового читача стріму.
enum Incoming {
    /// Відповідь на наш запит (`id` — наш власний лічильник).
    Response(u64, Result<Value, AcpError>),
    /// `session/update`-нотифікація (без `id`).
    Notification(Value),
}

/// ACP-клієнт однієї агент-сесії поверх пари потоків.
pub struct AcpClient<W> {
    writer: Arc<AsyncMutex<W>>,
    next_id: u64,
    rx: mpsc::UnboundedReceiver<Incoming>,
    reader: tokio::task::JoinHandle<()>,
}

impl<W> Drop for AcpClient<W> {
    fn drop(&mut self) {
        self.reader.abort();
    }
}

impl<W> AcpClient<W>
where
    W: AsyncWrite + Unpin + Send + 'static,
{
    /// Стартує фоновий читач стріму, живе разом із клієнтом.
    pub fn new<R>(reader: R, writer: W, permission: Option<PermissionHandler>) -> Self
    where
        R: AsyncRead + Unpin + Send + 'static,
    {
        let writer = Arc::new(AsyncMutex::new(writer));
        let (tx, rx) = mpsc::unbounded_channel();
        let reader = tokio::spawn(read_loop(reader, tx, Arc::clone(&writer), permission));
        Self {
            writer,
            next_id: 0,
            rx,
            reader,
        }
    }

    /// `initialize`: хендшейк версії протоколу (v1). ФС-можливостей клієнт
    /// не заявляє — файли виконавець править сам у `cwd` сесії.
    pub async fn initialize(&mut self) -> Result<(), AcpError> {
        let params = json!({
            "protocolVersion": 1,
            "clientCapabilities": { "fs": { "readTextFile": false, "writeTextFile": false } }
        });
        self.call("initialize", params, &|_| {}).await.map(|_| ())
    }

    /// `session/new` у робочій директорії (worktree run-а) → sessionId.
    /// Дренує prelude-нотифікації, що осіли одразу після відповіді
    /// (`SETTLE_TIMEOUT`), перш ніж повернути керування — інакше вони
    /// приліпляться до першого `prompt`.
    pub async fn new_session(&mut self, cwd: &str) -> Result<String, AcpError> {
        let params = json!({ "cwd": cwd, "mcpServers": [] });
        let result = self.call("session/new", params, &|_| {}).await?;
        self.settle(&|_| {}).await;
        result["sessionId"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| AcpError("session/new без sessionId".into()))
    }

    /// `session/prompt`: один хід. Події ходу емітяться через `emit`;
    /// завершення → `AgentTextDone` + stopReason.
    pub async fn prompt(
        &mut self,
        session_id: &str,
        text: &str,
        emit: &(dyn Fn(Event) + Send + Sync),
    ) -> Result<String, AcpError> {
        let params = json!({
            "sessionId": session_id,
            "prompt": [ { "type": "text", "text": text } ]
        });
        let result = self.call("session/prompt", params, emit).await?;
        emit(Event::AgentTextDone {});
        Ok(result["stopReason"]
            .as_str()
            .unwrap_or("end_turn")
            .to_string())
    }

    /// Викликає метод і читає з черги фонового читача до відповіді на свій
    /// id, обробляючи дорогою нотифікації (`session/update` → Event).
    /// Зустрічні запити агента (`session/request_permission`) обробляє сам
    /// фоновий читач — незалежно від того, який виклик зараз активний.
    async fn call(
        &mut self,
        method: &str,
        params: Value,
        emit: &(dyn Fn(Event) + Send + Sync),
    ) -> Result<Value, AcpError> {
        self.next_id += 1;
        let id = self.next_id;
        self.send(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }))
            .await?;

        loop {
            match self.rx.recv().await {
                Some(Incoming::Response(rid, result)) if rid == id => {
                    return result
                        .map_err(|AcpError(error)| AcpError(format!("{method}: {error}")));
                }
                Some(Incoming::Response(..)) => continue,
                Some(Incoming::Notification(message)) => self.handle_notification(&message, emit),
                None => return Err(AcpError("ACP-агент закрив стрім".into())),
            }
        }
    }

    /// Дренує чергу, поки нотифікації надходять швидше за `SETTLE_TIMEOUT`;
    /// тайм-аут або порожня черга — сигнал, що осідання завершилось.
    async fn settle(&mut self, emit: &(dyn Fn(Event) + Send + Sync)) {
        loop {
            match tokio::time::timeout(SETTLE_TIMEOUT, self.rx.recv()).await {
                Ok(Some(Incoming::Notification(message))) => {
                    self.handle_notification(&message, emit)
                }
                Ok(Some(Incoming::Response(..))) | Ok(None) | Err(_) => return,
            }
        }
    }

    /// `session/update` → Event: agent_message_chunk → AgentTextDelta;
    /// tool_call → ToolCall; tool_call_update (термінальний статус) →
    /// ToolResult. Невідомі варіанти ігноруються (forward-compat).
    fn handle_notification(&self, message: &Value, emit: &(dyn Fn(Event) + Send + Sync)) {
        if message["method"] != "session/update" {
            return;
        }
        let update = &message["params"]["update"];
        match update["sessionUpdate"].as_str() {
            Some("agent_message_chunk") => {
                if let Some(text) = update["content"]["text"].as_str() {
                    emit(Event::AgentTextDelta {
                        text: text.to_string(),
                    });
                }
            }
            Some("tool_call") => emit(Event::ToolCall {
                call_id: update["toolCallId"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                name: update["title"]
                    .as_str()
                    .or(update["kind"].as_str())
                    .unwrap_or("tool")
                    .to_string(),
                args: update["rawInput"].clone(),
            }),
            Some("tool_call_update") => {
                let status = update["status"].as_str().unwrap_or_default();
                if status == "completed" || status == "failed" {
                    emit(Event::ToolResult {
                        call_id: update["toolCallId"]
                            .as_str()
                            .unwrap_or_default()
                            .to_string(),
                        ok: status == "completed",
                        summary: update["title"].as_str().unwrap_or(status).to_string(),
                    });
                }
            }
            _ => {}
        }
    }

    async fn send(&self, message: &Value) -> Result<(), AcpError> {
        send_frame(&self.writer, message).await
    }
}

async fn send_frame(
    writer: &AsyncMutex<impl AsyncWrite + Unpin>,
    message: &Value,
) -> Result<(), AcpError> {
    let mut frame = message.to_string();
    frame.push('\n');
    let mut writer = writer.lock().await;
    writer
        .write_all(frame.as_bytes())
        .await
        .map_err(|e| AcpError(format!("запис ACP-стріму: {e}")))?;
    writer
        .flush()
        .await
        .map_err(|e| AcpError(format!("flush ACP-стріму: {e}")))
}

/// Фоновий читач стріму: класифікує кадри на відповіді/нотифікації
/// (форвардить у канал виклику) і сам відповідає на зустрічні запити
/// агента (`session/request_permission` → `PermissionHandler`) — доки
/// живе клієнт, незалежно від того, який `call()` зараз читає з каналу.
async fn read_loop(
    reader: impl AsyncRead + Unpin,
    tx: mpsc::UnboundedSender<Incoming>,
    writer: Arc<AsyncMutex<impl AsyncWrite + Unpin>>,
    permission: Option<PermissionHandler>,
) {
    let mut lines = BufReader::new(reader).lines();
    loop {
        let line = match lines.next_line().await {
            Ok(Some(line)) => line,
            _ => return,
        };
        if line.trim().is_empty() {
            continue;
        }
        let message: Value = match serde_json::from_str(&line) {
            Ok(value) => value,
            Err(_) => continue,
        };

        if message["method"].is_string() {
            if message["id"].is_null() {
                let _ = tx.send(Incoming::Notification(message));
            } else {
                handle_agent_request(&writer, &permission, &message).await;
            }
            continue;
        }
        let Some(id) = message["id"].as_u64() else {
            continue;
        };
        let result = match message.get("error").filter(|e| !e.is_null()) {
            Some(error) => Err(AcpError(error.to_string())),
            None => Ok(message["result"].clone()),
        };
        let _ = tx.send(Incoming::Response(id, result));
    }
}

/// Зустрічний запит агента. `session/request_permission` → handler
/// (без handler-а — відмова); вибирається перший option відповідного
/// kind (`allow*`/`reject*`). Інші методи → JSON-RPC method not found.
async fn handle_agent_request(
    writer: &AsyncMutex<impl AsyncWrite + Unpin>,
    permission: &Option<PermissionHandler>,
    message: &Value,
) {
    let id = message["id"].clone();
    if message["method"] != "session/request_permission" {
        let _ = send_frame(
            writer,
            &json!({
                "jsonrpc": "2.0", "id": id,
                "error": { "code": -32601, "message": "method not found" }
            }),
        )
        .await;
        return;
    }
    let params = &message["params"];
    let action = params["toolCall"]["title"]
        .as_str()
        .or(params["toolCall"]["kind"].as_str())
        .unwrap_or("tool")
        .to_string();
    let diff = params["toolCall"]["content"].as_str().map(str::to_string);
    let approved = match permission {
        Some(handler) => handler(action, diff).await,
        None => false,
    };
    let wanted = if approved { "allow" } else { "reject" };
    let option_id = params["options"]
        .as_array()
        .and_then(|options| {
            options
                .iter()
                .find(|o| o["kind"].as_str().unwrap_or_default().starts_with(wanted))
        })
        .and_then(|o| o["optionId"].as_str())
        .unwrap_or(wanted)
        .to_string();
    let _ = send_frame(
        writer,
        &json!({
            "jsonrpc": "2.0", "id": id,
            "result": { "outcome": { "outcome": "selected", "optionId": option_id } }
        }),
    )
    .await;
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    /// Фейковий ACP-агент на другому кінці duplex: скриптує initialize,
    /// session/new і session/prompt (чанки + tool call + відповідь).
    /// `prelude` — імітує `pi-acp`: одразу після `session/new`, ще до
    /// першого prompt, шле `agent_message_chunk` з банером CLI.
    async fn fake_agent(stream: tokio::io::DuplexStream, request_permission: bool, prelude: bool) {
        let (read, mut write) = tokio::io::split(stream);
        let mut lines = BufReader::new(read).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let message: Value = serde_json::from_str(&line).unwrap();
            let id = message["id"].clone();
            match message["method"].as_str() {
                Some("initialize") => {
                    respond(
                        &mut write,
                        &json!({ "jsonrpc": "2.0", "id": id, "result": { "protocolVersion": 1 } }),
                    )
                    .await;
                }
                Some("session/new") => {
                    respond(
                        &mut write,
                        &json!({ "jsonrpc": "2.0", "id": id, "result": { "sessionId": "s1" } }),
                    )
                    .await;
                    if prelude {
                        respond(
                            &mut write,
                            &json!({
                                "jsonrpc": "2.0", "method": "session/update",
                                "params": { "sessionId": "s1", "update": {
                                    "sessionUpdate": "agent_message_chunk",
                                    "content": { "type": "text", "text": "pi v0.79.9\n---\n" } } }
                            }),
                        )
                        .await;
                    }
                }
                Some("session/prompt") => {
                    for text in ["при", "віт"] {
                        respond(
                            &mut write,
                            &json!({
                                "jsonrpc": "2.0", "method": "session/update",
                                "params": { "sessionId": "s1", "update": {
                                    "sessionUpdate": "agent_message_chunk",
                                    "content": { "type": "text", "text": text } } }
                            }),
                        )
                        .await;
                    }
                    if request_permission {
                        respond(
                            &mut write,
                            &json!({
                                "jsonrpc": "2.0", "id": 777, "method": "session/request_permission",
                                "params": { "sessionId": "s1",
                                    "toolCall": { "title": "write_file", "kind": "edit" },
                                    "options": [
                                        { "optionId": "ok", "kind": "allow_once" },
                                        { "optionId": "no", "kind": "reject_once" } ] }
                            }),
                        )
                        .await;
                        // Відповідь клієнта на permission приходить наступним кадром.
                        let reply = lines.next_line().await.unwrap().unwrap();
                        let reply: Value = serde_json::from_str(&reply).unwrap();
                        let picked = reply["result"]["outcome"]["optionId"].clone();
                        respond(
                            &mut write,
                            &json!({
                                "jsonrpc": "2.0", "method": "session/update",
                                "params": { "sessionId": "s1", "update": {
                                    "sessionUpdate": "tool_call_update", "toolCallId": "c1",
                                    "status": if picked == "ok" { "completed" } else { "failed" },
                                    "title": "write_file" } }
                            }),
                        )
                        .await;
                    }
                    respond(&mut write, &json!({ "jsonrpc": "2.0", "id": id, "result": { "stopReason": "end_turn" } })).await;
                }
                _ => {}
            }
        }
    }

    async fn respond(write: &mut (impl AsyncWrite + Unpin), message: &Value) {
        let mut frame = message.to_string();
        frame.push('\n');
        write.write_all(frame.as_bytes()).await.unwrap();
        write.flush().await.unwrap();
    }

    fn client_for(
        stream: tokio::io::DuplexStream,
        permission: Option<PermissionHandler>,
    ) -> AcpClient<tokio::io::WriteHalf<tokio::io::DuplexStream>> {
        let (read, write) = tokio::io::split(stream);
        AcpClient::new(read, write, permission)
    }

    /// Повний хід: initialize → session/new → prompt; чанки стають
    /// AgentTextDelta, завершення — AgentTextDone.
    #[tokio::test]
    async fn prompt_maps_updates_to_events() {
        let (local, remote) = tokio::io::duplex(64 * 1024);
        tokio::spawn(fake_agent(remote, false, false));
        let mut client = client_for(local, None);

        client.initialize().await.unwrap();
        let session = client.new_session("/tmp").await.unwrap();
        assert_eq!(session, "s1");

        let events = Mutex::new(Vec::new());
        let emit = |event: Event| events.lock().unwrap().push(event);
        let stop = client.prompt(&session, "звук", &emit).await.unwrap();

        assert_eq!(stop, "end_turn");
        assert_eq!(
            *events.lock().unwrap(),
            vec![
                Event::AgentTextDelta {
                    text: "при".into()
                },
                Event::AgentTextDelta {
                    text: "віт".into()
                },
                Event::AgentTextDone {},
            ]
        );
    }

    /// request_permission: approve → агент отримує allow-option і шле
    /// completed; deny → reject-option і failed.
    #[tokio::test]
    async fn permission_request_routes_through_handler() {
        for (approve, expect_ok) in [(true, true), (false, false)] {
            let (local, remote) = tokio::io::duplex(64 * 1024);
            tokio::spawn(fake_agent(remote, true, false));
            let handler: PermissionHandler =
                Arc::new(move |_action, _diff| Box::pin(async move { approve }));
            let mut client = client_for(local, Some(handler));

            client.initialize().await.unwrap();
            let session = client.new_session("/tmp").await.unwrap();
            let events = Mutex::new(Vec::new());
            let emit = |event: Event| events.lock().unwrap().push(event);
            client.prompt(&session, "запиши", &emit).await.unwrap();

            let events = events.lock().unwrap();
            assert!(
                events.iter().any(|e| matches!(
                    e,
                    Event::ToolResult { ok, .. } if *ok == expect_ok
                )),
                "{events:?}"
            );
        }
    }

    /// Prelude-банер адаптера (напр. `pi-acp`), що приходить одразу після
    /// `session/new`, ще до першого prompt, — дренується `settle()` і не
    /// потрапляє в події першого реального ходу (регресія на mt/pull/51).
    #[tokio::test]
    async fn session_new_drains_prelude_before_first_prompt() {
        let (local, remote) = tokio::io::duplex(64 * 1024);
        tokio::spawn(fake_agent(remote, false, true));
        let mut client = client_for(local, None);

        client.initialize().await.unwrap();
        let session = client.new_session("/tmp").await.unwrap();

        let events = Mutex::new(Vec::new());
        let emit = |event: Event| events.lock().unwrap().push(event);
        client.prompt(&session, "звук", &emit).await.unwrap();

        assert_eq!(
            *events.lock().unwrap(),
            vec![
                Event::AgentTextDelta {
                    text: "при".into()
                },
                Event::AgentTextDelta {
                    text: "віт".into()
                },
                Event::AgentTextDone {},
            ],
            "банер адаптера не мав приліпитись до першої відповіді"
        );
    }
}
