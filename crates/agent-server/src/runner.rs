//! Виконавці ходу інтерактивної сесії.
//!
//! `UserMessage` клієнта запускає хід агента; всі події ходу емітяться в
//! сесію (Envelope збирає session host). Транспорт виконавця — **ACP (Agent
//! Client Protocol)**: [`AcpTurnRunner`] спавнить ACP-адаптер підписочного
//! CLI (claude / codex / cursor / pi) per-кімнату і мапить
//! `session/request_permission` на approval-гейт (`ApprovalRequest`,
//! ADR `260713-2110`). [`EchoTurnRunner`] — заглушка для demo/CLI і тестів
//! транспорту.

use std::collections::HashMap;
use std::fmt;
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;

use agent_core::{AcpClient, PermissionHandler};
use agent_protocol::Event;
use async_trait::async_trait;

/// Помилка ходу виконавця (текстова: транспорт/виконавець повідомляє причину).
#[derive(Debug)]
pub struct TurnError(pub String);

impl fmt::Display for TurnError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for TurnError {}

/// Виконавець одного ходу кімнати. `workdir` — робоча директорія ходу
/// (worktree інтерактивного run-а); runner-и без файлових тулів її ігнорують.
#[async_trait]
pub trait TurnRunner: Send + Sync {
    async fn run_turn(
        &self,
        node_hash: &str,
        user_text: &str,
        workdir: Option<&Path>,
        emit: &(dyn Fn(Event) + Send + Sync),
    ) -> Result<String, TurnError>;
}

/// Фабрика [`PermissionHandler`] для кімнати: хост дає обробник
/// `request_permission`, що знає вузол (роутинг `ApprovalRequest` у правильну
/// кімнату).
pub type PermissionFactory = Arc<dyn Fn(&str) -> PermissionHandler + Send + Sync>;

/// Жива ACP-сесія кімнати: процес адаптера + клієнт + sessionId.
struct AcpRoom {
    /// Тримаємо процес живим на весь час кімнати (kill_on_drop).
    _child: tokio::process::Child,
    client: AcpClient<tokio::process::ChildStdin>,
    session_id: String,
}

/// Референсний виконавець: зовнішній підписочний CLI через ACP-адаптер.
/// Per-кімнату — окремий процес адаптера (своя історія в сесії агента);
/// `workdir` ходу стає `cwd` ACP-сесії (worktree run-а).
pub struct AcpTurnRunner {
    argv: Vec<String>,
    permission_factory: Option<PermissionFactory>,
    rooms: tokio::sync::Mutex<HashMap<String, AcpRoom>>,
}

impl AcpTurnRunner {
    /// `command` — рядок команди ACP-адаптера (whitespace-токенізація, без
    /// shell-метасимволів), напр. `npx claude-code-acp`.
    pub fn new(command: &str, permission_factory: Option<PermissionFactory>) -> Self {
        Self {
            argv: command.split_whitespace().map(str::to_string).collect(),
            permission_factory,
            rooms: tokio::sync::Mutex::new(HashMap::new()),
        }
    }

    /// Спавнить адаптер і відкриває ACP-сесію кімнати (initialize +
    /// session/new у workdir).
    async fn open_room(
        &self,
        node_hash: &str,
        workdir: Option<&Path>,
    ) -> Result<AcpRoom, TurnError> {
        let program = self
            .argv
            .first()
            .ok_or_else(|| TurnError("порожня команда ACP-адаптера".into()))?;
        let mut child = tokio::process::Command::new(program)
            .args(&self.argv[1..])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| TurnError(format!("spawn ACP-адаптера {program}: {e}")))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| TurnError("stdin адаптера".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| TurnError("stdout адаптера".into()))?;
        let permission = self.permission_factory.as_ref().map(|f| f(node_hash));
        let mut client = AcpClient::new(stdout, stdin, permission);
        client
            .initialize()
            .await
            .map_err(|e| TurnError(e.to_string()))?;
        // ACP-спека вимагає абсолютний cwd (NewSessionRequest.cwd); без
        // workdir (M1 CLI без графа/worktree) беремо cwd поточного процесу —
        // деякі адаптери (claude-agent-acp) відкидають "." як невалідний.
        let cwd = match workdir {
            Some(p) => p.to_string_lossy().into_owned(),
            None => std::env::current_dir()
                .map_err(|e| TurnError(format!("cwd поточного процесу: {e}")))?
                .to_string_lossy()
                .into_owned(),
        };
        let session_id = client
            .new_session(&cwd)
            .await
            .map_err(|e| TurnError(e.to_string()))?;
        Ok(AcpRoom {
            _child: child,
            client,
            session_id,
        })
    }
}

#[async_trait]
impl TurnRunner for AcpTurnRunner {
    async fn run_turn(
        &self,
        node_hash: &str,
        user_text: &str,
        workdir: Option<&Path>,
        emit: &(dyn Fn(Event) + Send + Sync),
    ) -> Result<String, TurnError> {
        let mut rooms = self.rooms.lock().await;
        if !rooms.contains_key(node_hash) {
            let room = self.open_room(node_hash, workdir).await?;
            rooms.insert(node_hash.to_string(), room);
        }
        let room = rooms.get_mut(node_hash).expect("щойно вставлена кімната");
        let session_id = room.session_id.clone();
        room.client
            .prompt(&session_id, user_text, emit)
            .await
            .map_err(|e| TurnError(e.to_string()))
    }
}

/// Скриптований виконавець для тестів транспорту/сесій: на кожен хід
/// віддає наступний текст зі скрипту (емітить `AgentTextDelta` +
/// `AgentTextDone`), не викликаючи жодного LLM.
pub struct ScriptedTurnRunner {
    responses: std::sync::Mutex<std::collections::VecDeque<String>>,
}

impl ScriptedTurnRunner {
    /// Створює runner зі списком відповідей (по одній на хід).
    pub fn new<I, S>(responses: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            responses: std::sync::Mutex::new(responses.into_iter().map(Into::into).collect()),
        }
    }
}

#[async_trait]
impl TurnRunner for ScriptedTurnRunner {
    async fn run_turn(
        &self,
        _node_hash: &str,
        _user_text: &str,
        _workdir: Option<&Path>,
        emit: &(dyn Fn(Event) + Send + Sync),
    ) -> Result<String, TurnError> {
        let text = self
            .responses
            .lock()
            .expect("responses mutex")
            .pop_front()
            .unwrap_or_default();
        emit(Event::AgentTextDelta { text: text.clone() });
        emit(Event::AgentTextDone {});
        Ok(text)
    }
}

/// Заглушка без LLM: віддзеркалює текст користувача. Для demo `attach`
/// без підключеного ACP-виконавця і для тестів транспорту.
pub struct EchoTurnRunner;

#[async_trait]
impl TurnRunner for EchoTurnRunner {
    async fn run_turn(
        &self,
        _node_hash: &str,
        user_text: &str,
        _workdir: Option<&Path>,
        emit: &(dyn Fn(Event) + Send + Sync),
    ) -> Result<String, TurnError> {
        let text = format!("echo: {user_text}");
        emit(Event::AgentTextDelta { text: text.clone() });
        emit(Event::AgentTextDone {});
        Ok(text)
    }
}
