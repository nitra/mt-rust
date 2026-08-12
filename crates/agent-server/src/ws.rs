//! WS-транспорт: хендшейк v4, стрічка подій, capability-фільтр
//! (спека runtime.md, «Протокол подій» / «Хендшейк» / backpressure).
//!
//! Кадри — JSON: перший від клієнта `ClientHello` (несумісна версія чи
//! невірний токен → `Event::Error` + закриття), відповідь `ServerHello`,
//! далі від клієнта — `Envelope` (host ігнорує клієнтські seq/ts і
//! призначає власні), від хоста — `Envelope` стрічки. Повільний клієнт,
//! що випав із broadcast-буфера, повертається реплеєм за `want_replay_from`.

use std::collections::HashMap;
use std::io;
use std::net::SocketAddr;
use std::sync::Arc;

use agent_protocol::{ClientHello, Envelope, Event, ServerHello, PROTOCOL_VERSION};
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::Response;
use axum::routing::get;
use axum::Router;
use serde::Serialize;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use uuid::Uuid;

/// Скільки чекати `HandoffAck` від тримача (реалізація, не контракт):
/// тримач мусить завершити поточний хід, закомітити й запушити run ref.
const HANDOFF_TIMEOUT_SEC: u64 = 60;

use crate::approvals_gate::ApprovalGate;
use crate::graph::{self, GraphConfig, InteractiveRun};
use crate::runner::TurnRunner;
use crate::session::{Session, SessionHost};

/// Стан сервера: сесії + виконавець ходів + очікуваний токен discovery +
/// опційний graph-міст (без нього — транспортний режим, кімнати не
/// прив'язані до вузлів графа).
pub struct AppState {
    pub sessions: Arc<SessionHost>,
    pub runner: Arc<dyn TurnRunner>,
    /// `None` — без перевірки (embedded/in-process клієнт).
    pub token: Option<String>,
    /// Гейт підписаних approvals (access.md); pubkey-кеш наповнює relay-міст.
    pub approvals: Arc<ApprovalGate>,
    graph: Option<GraphConfig>,
    /// Ручка пробудження orchestrator-ролі: relay push «є нові події у
    /// задачі X» має будити хост негайно, а не чекати fallback-таймера
    /// (runtime.md, «Wake: push замість polling»). `None` — orchestrator
    /// у цьому процесі не запущений.
    wake: std::sync::Mutex<Option<std::sync::Arc<std::sync::atomic::AtomicBool>>>,
    /// Активні інтерактивні run-и за node-ключем кімнати. Git-операції
    /// швидкі й локальні — виконуються під локом (spawn_blocking — TODO
    /// разом із віддаленими remote).
    runs: tokio::sync::Mutex<HashMap<String, InteractiveRun>>,
    /// Хто чекає на `HandoffAck` (цей хост попросив вузол «сюди»).
    /// Наявність запису тут — і є ознака «ack адресований нам»: у кімнаті
    /// подія широкомовна, і власний ack емітента відсіюється саме тим, що
    /// в нього очікувача немає.
    handoff_waiters:
        tokio::sync::Mutex<HashMap<String, tokio::sync::oneshot::Sender<graph::HandoffTicket>>>,
}

impl AppState {
    pub fn new(sessions: SessionHost, runner: Arc<dyn TurnRunner>, token: Option<String>) -> Self {
        Self::from_parts(
            Arc::new(sessions),
            Arc::new(ApprovalGate::default()),
            runner,
            token,
        )
    }

    /// Реєструє ручку пробудження orchestrator-ролі.
    pub fn set_wake(&self, signaller: std::sync::Arc<std::sync::atomic::AtomicBool>) {
        if let Ok(mut slot) = self.wake.lock() {
            *slot = Some(signaller);
        }
    }

    /// Будить orchestrator-роль, якщо вона є в цьому процесі. Тихо
    /// нічого не робить інакше — relay-міст не має знати, чи хост
    /// виконує ще й оркестрацію.
    pub fn wake_orchestrator(&self) {
        if let Ok(slot) = self.wake.lock() {
            if let Some(flag) = slot.as_ref() {
                flag.store(true, std::sync::atomic::Ordering::SeqCst);
            }
        }
    }

    /// Конструктор зі спільними частинами — коли sessions/gate потрібні
    /// runner-фабриці ДО створення AppState (approval-гейт тулів).
    pub fn from_parts(
        sessions: Arc<SessionHost>,
        approvals: Arc<ApprovalGate>,
        runner: Arc<dyn TurnRunner>,
        token: Option<String>,
    ) -> Self {
        Self {
            sessions,
            runner,
            token,
            approvals,
            wake: std::sync::Mutex::new(None),
            graph: None,
            runs: tokio::sync::Mutex::new(HashMap::new()),
            handoff_waiters: tokio::sync::Mutex::new(HashMap::new()),
        }
    }

    /// Mid-run approval-гейт: шле `ApprovalRequest` у кімнату і повертає
    /// one-shot із підписаним вердиктом (access.md, перший гейт).
    pub fn request_approval(
        &self,
        node: &str,
        action: String,
        diff: Option<String>,
    ) -> std::io::Result<tokio::sync::oneshot::Receiver<bool>> {
        crate::approvals_gate::request_approval(&self.sessions, &self.approvals, node, action, diff)
    }

    /// Увімкнути graph-міст: кімната = вузол, UserMessage веде claim/worktree.
    pub fn with_graph(mut self, config: GraphConfig) -> Self {
        self.graph = Some(config);
        self
    }

    /// Кооперативний handoff вузла (runtime.md, «Міграція сесії між
    /// хостами», крок 2): знімає run з обліку, `InteractiveRun::handoff`
    /// пише `run_NNN.md (result: handoff)` і CAS-delete claim; сповіщає
    /// сесію `ClaimChanged { holder: None }` — той самий сигнал, що й
    /// release (деталь «це handoff, не пауза» лишається в run-файлі).
    pub async fn handoff_node(&self, node: &str) -> Result<graph::HandoffTicket, String> {
        let Some(run) = self.runs.lock().await.remove(node) else {
            return Err(format!("handoff: вузол {node} без активного run"));
        };
        let generation = run.generation();
        let ticket = run.handoff()?;
        if let Ok(session) = self.sessions.get_or_open(node) {
            self.sessions.publish(
                &session,
                Event::ClaimChanged {
                    node_hash: node.to_string(),
                    holder_device_id: None,
                    lease_until: None,
                    generation,
                },
                None,
                None,
            );
        }
        Ok(ticket)
    }

    /// Чи тримає цей хост активний run вузла. Джерело істини «хто пише» —
    /// git claim; це лише локальний облік процесу.
    pub async fn holds_node(&self, node: &str) -> bool {
        self.runs.lock().await.contains_key(node)
    }

    /// Вузли, які цей хост зараз тримає — вміст presence «активні вузли»
    /// (access.md). Джерело — локальний облік run-ів, тобто те саме, що
    /// відповідає на `holds_node`.
    pub async fn active_nodes(&self) -> Vec<String> {
        self.runs.lock().await.keys().cloned().collect()
    }

    /// Generation claim-а активного run-а (для `ClaimChanged` після pull).
    pub async fn node_generation(&self, node: &str) -> Option<u64> {
        self.runs
            .lock()
            .await
            .get(node)
            .map(InteractiveRun::generation)
    }

    /// Обробка чужого `HandoffRequest` (крок 2): якщо цей хост тримає run —
    /// віддає його і публікує `HandoffAck` у кімнату. Якщо не тримає —
    /// **мовчить**: спека не має відмови на цей випадок, у неї інший шлях
    /// (крок 4 — lease expiry + grace takeover), і фальшива відмова від
    /// не-тримача лише збила б прохача з цього шляху.
    pub async fn serve_handoff_request(&self, node: &str) {
        let Ok(ticket) = self.handoff_node(node).await else {
            return;
        };
        if let Ok(session) = self.sessions.get_or_open(node) {
            self.sessions.publish(
                &session,
                Event::HandoffAck {
                    node_hash: node.to_string(),
                    run_token: ticket.run_token,
                    generation: ticket.generation,
                },
                None,
                None,
            );
        }
    }

    /// Приймає `HandoffAck` — віддає тікет тому, хто чекає. Ack без
    /// очікувача (у т.ч. власне ехо емітента) ігнорується.
    pub async fn accept_handoff_ack(&self, node: &str, ticket: graph::HandoffTicket) {
        if let Some(waiter) = self.handoff_waiters.lock().await.remove(node) {
            let _ = waiter.send(ticket);
        }
    }

    /// «Перенести сюди» (runtime.md, крок 1 + 3): публікує `HandoffRequest`
    /// у кімнату, чекає `HandoffAck` до `timeout` і відновлює вузол на
    /// цьому хості.
    ///
    /// Таймаут — не косметика: тримач може бути офлайн або взагалі відсутній,
    /// і тоді штатний шлях не «висіти», а повернути помилку, з якої видно
    /// альтернативу зі спеки — дочекатись expiry+grace і зробити takeover.
    pub async fn pull_node(
        self: &Arc<Self>,
        node: &str,
        timeout: std::time::Duration,
    ) -> Result<(), String> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.handoff_waiters
            .lock()
            .await
            .insert(node.to_string(), tx);

        // Broadcast без запису в журнал: вузол ще не наш, і локальна сесія
        // тут відкриватись не має — інакше `resume_node` не зможе засіяти
        // успадкований журнал (session.rs, `broadcast_only`).
        self.sessions.broadcast_only(
            node,
            Event::HandoffRequest {
                node_hash: node.to_string(),
            },
        );

        let ticket = match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(ticket)) => ticket,
            _ => {
                self.handoff_waiters.lock().await.remove(node);
                return Err(format!(
                    "handoff: тримач вузла {node} не відповів — лишається шлях MT: \
                     дочекатись закінчення lease + grace і зробити takeover"
                ));
            }
        };
        self.resume_node(node, &ticket).await
    }

    /// Відновлення на цьому хості після кооперативного handoff (runtime.md,
    /// крок 3): `attach_resume` матеріалізує worktree зі стану старого run
    /// ref → журнал `.nitra/session.jsonl` засіває локальну сесію (best
    /// effort: помилка сіву не валить resume — сесія просто почне з
    /// чистого seq) → run під обліком, renewal запущено.
    pub async fn resume_node(
        self: &Arc<Self>,
        node: &str,
        ticket: &graph::HandoffTicket,
    ) -> Result<(), String> {
        let config = self
            .graph
            .as_ref()
            .ok_or_else(|| "resume: graph-міст не увімкнено".to_string())?;
        let run = graph::attach_resume(config, node, ticket)?;

        if let Ok(jsonl) = std::fs::read_to_string(run.worktree.join(".nitra/session.jsonl")) {
            let _ = self.sessions.seed_journal(node, &jsonl);
        }

        let lease_sec = config.lease_sec;
        self.runs.lock().await.insert(node.to_string(), run);
        spawn_renewal(Arc::clone(self), node.to_string(), lease_sec);
        Ok(())
    }
}

/// Маршрути хоста: єдина точка `/ws`.
pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/ws", get(ws_handler))
        .with_state(state)
}

/// Біндить адресу (порт 0 → ефемерний) і запускає сервер у фоні.
pub async fn serve(
    state: Arc<AppState>,
    addr: SocketAddr,
) -> io::Result<(SocketAddr, JoinHandle<()>)> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let local_addr = listener.local_addr()?;
    let app = router(state);
    let handle = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    Ok((local_addr, handle))
}

/// Ліміт кадру — 2 MB, спільний із relay (runtime.md). Без нього один
/// великий diff чи прев'ю від клієнта міг би роздути памʼять хоста.
const MAX_FRAME_BYTES: usize = 2 * 1024 * 1024;

async fn ws_handler(ws: WebSocketUpgrade, State(state): State<Arc<AppState>>) -> Response {
    ws.max_message_size(MAX_FRAME_BYTES)
        .max_frame_size(MAX_FRAME_BYTES)
        .on_upgrade(move |socket| client_connection(socket, state))
}

async fn send_json<T: Serialize>(socket: &mut WebSocket, value: &T) -> Result<(), axum::Error> {
    socket
        .send(Message::Text(serde_json::to_string(value).unwrap().into()))
        .await
}

/// Відмова на хендшейку: `Event::Error` + коректний Close-кадр.
async fn reject(socket: &mut WebSocket, message: String) {
    let _ = send_json(socket, &Event::Error { message }).await;
    let _ = socket.send(Message::Close(None)).await;
}

/// Чи можна доставити подію клієнту з такими capabilities
/// (`PreviewScreenshot` — лише клієнтам із «preview»).
fn allowed(event: &Event, capabilities: &[String]) -> bool {
    match event {
        Event::PreviewScreenshot { .. } => capabilities.iter().any(|c| c == "preview"),
        _ => true,
    }
}

async fn client_connection(mut socket: WebSocket, state: Arc<AppState>) {
    // Хендшейк: перший текстовий кадр мусить бути ClientHello.
    let hello: ClientHello = loop {
        match socket.recv().await {
            Some(Ok(Message::Text(text))) => match serde_json::from_str(text.as_str()) {
                Ok(hello) => break hello,
                Err(error) => {
                    reject(&mut socket, format!("invalid ClientHello: {error}")).await;
                    return;
                }
            },
            Some(Ok(_)) => continue,
            _ => return,
        }
    };
    if let Some(expected) = &state.token {
        if &hello.device_token != expected {
            reject(&mut socket, "invalid device token".into()).await;
            return;
        }
    }
    if let Err(error) = hello.check_compatibility() {
        reject(&mut socket, error.to_string()).await;
        return;
    }

    // Підписка ДО реплею — щоб не загубити події між ними (дублікати
    // клієнт відсіює за seq).
    let mut updates = state.sessions.subscribe();

    if send_json(
        &mut socket,
        &ServerHello {
            protocol_version: PROTOCOL_VERSION,
            session_list: state.sessions.session_list(),
        },
    )
    .await
    .is_err()
    {
        return;
    }

    // Останній надісланий seq — точка, з якої наздоганяємо клієнта при
    // backpressure. Оновлюється і на реплеї, і на живих подіях.
    let mut last_sent_seq: Option<u64> = None;
    if let Some(from) = hello.want_replay_from {
        for envelope in state.sessions.replay_from(from) {
            last_sent_seq = Some(envelope.seq);
            if allowed(&envelope.event, &hello.client_capabilities)
                && send_json(&mut socket, &envelope).await.is_err()
            {
                return;
            }
        }
    }

    loop {
        tokio::select! {
            incoming = socket.recv() => match incoming {
                Some(Ok(Message::Text(text))) => {
                    // Кадр обробляється у окремій задачі: хід агента може
                    // чекати ApprovalResponse із ЦЬОГО Ж зʼєднання —
                    // інлайн-обробка дала б deadlock.
                    let state = Arc::clone(&state);
                    let device_id = hello.device_id;
                    tokio::spawn(async move {
                        handle_client_frame(&state, text.as_str(), Some(device_id)).await;
                    });
                }
                Some(Ok(Message::Close(_))) | None => break,
                Some(Ok(_)) => {}
                Some(Err(_)) => break,
            },
            update = updates.recv() => match update {
                Ok(envelope) => {
                    last_sent_seq = Some(envelope.seq);
                    if allowed(&envelope.event, &hello.client_capabilities)
                        && send_json(&mut socket, &envelope).await.is_err()
                    {
                        break;
                    }
                }
                // Backpressure (runtime.md): повільний клієнт випав із
                // broadcast-буфера. Ефемерні події (дельти тексту, прев'ю)
                // при цьому втрачати можна — вони не журналяться; а от
                // журнальовані **доставляються завжди**, інакше клієнт
                // мовчки побачив би діру в стрічці замість помилки.
                // Тому наздоганяємо їх реплеєм із журналу сесії.
                Err(broadcast::error::RecvError::Lagged(dropped)) => {
                    let from = last_sent_seq.map_or(0, |seq| seq + 1);
                    let missed = state.sessions.replay_from(from);
                    let mut failed = false;
                    for envelope in missed {
                        last_sent_seq = Some(envelope.seq);
                        if allowed(&envelope.event, &hello.client_capabilities)
                            && send_json(&mut socket, &envelope).await.is_err()
                        {
                            failed = true;
                            break;
                        }
                    }
                    if failed {
                        // Черга надсилання не розсмоктується — примусовий
                        // disconnect із Error; клієнт повернеться реплеєм.
                        let _ = send_json(
                            &mut socket,
                            &Event::Error {
                                message: format!(
                                    "backpressure: клієнт відстав на {dropped} подій і не встигає \
                                     приймати наздоганяння — перепідключіться з want_replay_from"
                                ),
                            },
                        )
                        .await;
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Closed) => break,
            },
        }
    }
}

/// Кадр клієнта: Envelope з подією. `UserMessage` запускає хід агента
/// (з graph-мостом — попередньо attach вузла); `DoneSession`/
/// `ReleaseSession` завершують run; невідомі події ігноруються
/// (forward-compatibility).
pub(crate) async fn handle_client_frame(
    state: &Arc<AppState>,
    frame: &str,
    device_id: Option<Uuid>,
) {
    let Ok(envelope) = serde_json::from_str::<Envelope>(frame) else {
        return;
    };
    let node = envelope.node_hash.clone();
    // Пара хост↔хост (runtime.md, «Міграція сесії між хостами») —
    // обробляється ДО `get_or_open`. Це не стиль, а необхідність: хост, який
    // лише просить вузол, ще не має ані run-а, ані права на журнал, а
    // відкриття сесії тут заблокувало б засів успадкованого журналу при
    // відновленні («сесія вже відкрита»).
    match &envelope.event {
        Event::HandoffRequest { node_hash } => {
            state.serve_handoff_request(node_hash).await;
            return;
        }
        Event::HandoffAck {
            node_hash,
            run_token,
            generation,
        } => {
            state
                .accept_handoff_ack(
                    node_hash,
                    graph::HandoffTicket {
                        run_token: run_token.clone(),
                        generation: *generation,
                    },
                )
                .await;
            return;
        }
        _ => {}
    }
    let Ok(session) = state.sessions.get_or_open(&node) else {
        return;
    };
    match envelope.event {
        Event::UserMessage { text, .. } => {
            handle_user_message(
                state,
                &session,
                &node,
                &text,
                device_id,
                envelope.account_id,
            )
            .await;
        }
        Event::HandoffPull { node_hash } => handle_handoff_pull(state, node_hash).await,
        Event::DoneSession {} => handle_done(state, &session, &node).await,
        Event::ReleaseSession {} => handle_release(state, &session, &node).await,
        Event::ApprovalResponse {
            request_id,
            approved,
            signature,
        } => {
            match state
                .approvals
                .resolve(&request_id, approved, &signature, device_id)
            {
                // Верифікований вердикт журналюється у сесію (аудит-трейл)
                // і матеріалізується у run вузла (## Approvals при done).
                Ok(mut record) => {
                    // account_id приходить із конверта підписанта — саме він
                    // відповідає у трейлі за «хто дозволив», а не пристрій.
                    record.account_id = envelope.account_id;
                    if let Some(run) = state.runs.lock().await.get_mut(&node) {
                        run.add_approval(record.to_yaml_line(chrono::Utc::now()));
                    }
                    state.sessions.publish(
                        &session,
                        Event::ApprovalResponse {
                            request_id,
                            approved,
                            signature,
                        },
                        device_id,
                        envelope.account_id,
                    );
                }
                Err(message) => publish_error(state, &session, message),
            }
        }
        _ => {}
    }
}

fn publish_error(state: &AppState, session: &Session, message: String) {
    state
        .sessions
        .publish(session, Event::Error { message }, None, None);
}

async fn handle_user_message(
    state: &Arc<AppState>,
    session: &Arc<Session>,
    node: &str,
    text: &str,
    device_id: Option<Uuid>,
    account_id: Option<Uuid>,
) {
    state.sessions.publish(
        session,
        Event::UserMessage {
            text: text.to_string(),
            attachments: vec![],
            surface: None,
        },
        device_id,
        account_id,
    );

    // Graph-міст: перший хід вузла — attach (CAS claim + worktree + run ref).
    let workdir = if let Some(config) = &state.graph {
        let mut runs = state.runs.lock().await;
        if !runs.contains_key(node) {
            match graph::attach(config, node) {
                Ok(run) => {
                    runs.insert(node.to_string(), run);
                    spawn_renewal(Arc::clone(state), node.to_string(), config.lease_sec);
                }
                Err(error) => {
                    // Без claim хід не виконується — вузол зайнято/недоступно.
                    publish_error(state, session, error);
                    return;
                }
            }
        }
        runs.get(node).map(|run| run.worktree.clone())
    } else {
        None
    };

    let sessions = &state.sessions;
    let emit = |event: Event| {
        sessions.publish(session, event, None, None);
    };
    if let Err(error) = state
        .runner
        .run_turn(node, text, workdir.as_deref(), &emit)
        .await
    {
        publish_error(state, session, error.to_string());
    }

    // Кожен хід — коміт (файли + журнал сесії) → push run ref (git.md).
    if state.graph.is_some() {
        let journal = journal_jsonl(session);
        let runs = state.runs.lock().await;
        if let Some(run) = runs.get(node) {
            let message = format!("mt: {node} інтерактивний хід");
            if let Err(error) = run.commit_turn(&journal, &message) {
                publish_error(state, session, format!("run ref push: {error}"));
            }
        }
    }
}

/// `mt done`-семантика: `## Check` → strip `.nitra/` → fenced publish →
/// `Committed`. Відмова Check чи системна помилка НЕ знімає run — можна
/// виправити й повторити done; run знімається при published (успіх) або
/// fenced (claim втрачено — retry марний).
async fn handle_done(state: &Arc<AppState>, session: &Arc<Session>, node: &str) {
    let mut runs = state.runs.lock().await;
    let Some(run) = runs.get(node) else {
        publish_error(
            state,
            session,
            format!("done: вузол {node} без активного run"),
        );
        return;
    };
    match run.done(8, 250) {
        Ok(outcome) if outcome.published => {
            runs.remove(node);
            state.sessions.publish(
                session,
                Event::Committed {
                    commit_hash: outcome.result_sha.unwrap_or_default(),
                    message: format!("mt: {node} done — fact опубліковано"),
                },
                None,
                None,
            );
        }
        Ok(outcome) => {
            if outcome.fenced {
                runs.remove(node);
            }
            publish_error(
                state,
                session,
                if outcome.fenced {
                    "done: claim втрачено під час publish — worktree лишився для debug".into()
                } else {
                    "done: конкурентний publish виграв гонку — спробуйте пізніше".into()
                },
            );
        }
        Err(error) => publish_error(state, session, format!("done: {error}")),
    }
}

/// Пауза: CAS-delete claim; журнал лишається в run ref базою відновлення.
/// `mt handoff <node>`: локальний клієнт просить цей хост забрати вузол.
/// Результат іде у стрічку тим самим `ClaimChanged`, яким живуть усі зміни
/// тримача — окремої «відповіді на команду» протокол не має і не потребує.
async fn handle_handoff_pull(state: &Arc<AppState>, node: String) {
    let timeout = std::time::Duration::from_secs(HANDOFF_TIMEOUT_SEC);
    match state.pull_node(&node, timeout).await {
        Ok(()) => {
            let generation = state.node_generation(&node).await.unwrap_or_default();
            if let Ok(session) = state.sessions.get_or_open(&node) {
                state.sessions.publish(
                    &session,
                    Event::ClaimChanged {
                        node_hash: node.clone(),
                        holder_device_id: None,
                        lease_until: None,
                        generation,
                    },
                    None,
                    None,
                );
            }
        }
        Err(message) => {
            if let Ok(session) = state.sessions.get_or_open(&node) {
                publish_error(state, &session, message);
            }
        }
    }
}

async fn handle_release(state: &Arc<AppState>, session: &Arc<Session>, node: &str) {
    let Some(run) = state.runs.lock().await.remove(node) else {
        publish_error(
            state,
            session,
            format!("release: вузол {node} без активного run"),
        );
        return;
    };
    let generation = run.generation();
    match run.release() {
        Ok(_) => {
            state.sessions.publish(
                session,
                Event::ClaimChanged {
                    node_hash: node.to_string(),
                    holder_device_id: None,
                    lease_until: None,
                    generation,
                },
                None,
                None,
            );
        }
        Err(error) => publish_error(state, session, format!("release: {error}")),
    }
}

/// Журнал сесії у форматі `session.jsonl` (по рядку на Envelope).
fn journal_jsonl(session: &Session) -> String {
    session
        .replay_from(0)
        .iter()
        .map(|envelope| serde_json::to_string(envelope).unwrap())
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

/// Фоновий renewal lease (~кожну третину lease). Run зник із мапи
/// (done/release) → задача завершується; невдалий renewal → Error у
/// сесію, run прибирається (claim втрачено).
fn spawn_renewal(state: Arc<AppState>, node: String, lease_sec: i64) {
    let period = std::time::Duration::from_secs((lease_sec / 3).max(1) as u64);
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(period);
        interval.tick().await; // перший tick — миттєвий, пропускаємо
        loop {
            interval.tick().await;
            let mut runs = state.runs.lock().await;
            let Some(run) = runs.get_mut(&node) else {
                return;
            };
            match run.renew() {
                Ok(true) => {}
                Ok(false) | Err(_) => {
                    runs.remove(&node);
                    drop(runs);
                    if let Ok(session) = state.sessions.get_or_open(&node) {
                        publish_error(
                            &state,
                            &session,
                            format!("claim-lost: lease вузла {node} не подовжено"),
                        );
                    }
                    return;
                }
            }
        }
    });
}
