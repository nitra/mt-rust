//! `mt serve` / `mt attach` — хост-процес і тонкий клієнт до нього.
//!
//! Раніше жили окремим бінарником `agent-cli`; за roadmap M1 командна
//! поверхня одна — `mt`. Хост за overview.md — це «orchestrator + runner +
//! session host» в одному процесі, тому `serve` піднімає і orchestrator-роль.
//!
//! `serve` — WS на 127.0.0.1, discovery port-file + токен; runner — ACP-
//! адаптер підписочного CLI (`--acp-cmd` або env `MT_ACP_AGENT_CMD`), без
//! нього echo-заглушка транспорту.
//! `attach <node>` — читає discovery, хендшейк v4, REPL: stdin →
//! `UserMessage`, стрічка подій → термінал.

use std::io::Write as _;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use agent_core::PermissionHandler;
use agent_protocol::{ClientHello, Envelope, Event, ServerHello, PROTOCOL_VERSION};
use agent_server::approvals_gate::request_approval;
use agent_server::orchestrator::{Orchestrator, Wake};
use agent_server::{
    serve, spawn_relay_bridge, AcpTurnRunner, AppState, ApprovalGate, Discovery, EchoTurnRunner,
    GraphConfig, PermissionFactory, RelayBridgeConfig, SessionHost, TurnRunner,
};
use clap::Args;
use futures::{SinkExt, StreamExt};
use tokio::io::AsyncBufReadExt;
use tokio_tungstenite::tungstenite::Message;
use uuid::Uuid;


/// Інтервал fallback-прокидання оркестратора, коли relay мовчить.
const WAKE_FALLBACK_SEC: u64 = 300;

fn state_dir(cli_dir: Option<PathBuf>) -> PathBuf {
    cli_dir.unwrap_or_else(|| {
        PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into())).join(".nitra")
    })
}

#[derive(Args)]
pub struct ServeArgs {
    /// Директорія discovery/стану (дефолт — ~/.nitra).
    #[arg(long)]
    pub state_dir: Option<PathBuf>,
    /// Порт (0 — ефемерний).
    #[arg(long, default_value_t = 0)]
    pub port: u16,
    /// Команда ACP-адаптера підписочного CLI (напр. `npx claude-code-acp`).
    #[arg(long, env = "MT_ACP_AGENT_CMD")]
    pub acp_cmd: Option<String>,
    /// Адреса relay (`ws://…`/`wss://…`) — вмикає міст до relay.
    #[arg(long)]
    pub relay_url: Option<String>,
    /// device_token host-пристрою на relay.
    #[arg(long, default_value = "")]
    pub relay_token: String,
    /// Кімната relay (кореневий вузол задачі).
    #[arg(long, default_value = "")]
    pub relay_root: String,
    /// Не піднімати orchestrator-роль (лише session host).
    #[arg(long)]
    pub no_orchestrator: bool,
}

#[derive(Args)]
pub struct AttachArgs {
    /// Вузол (шлях у tasks-директорії).
    pub node: String,
    /// Директорія discovery/стану (дефолт — ~/.nitra).
    #[arg(long)]
    pub state_dir: Option<PathBuf>,
    /// BCP-47 мова учасника (обовʼязкове поле v4).
    #[arg(long, default_value = "uk")]
    pub lang: String,
}

/// Синхронна обгортка: решта команд `mt` синхронні, тому рантайм
/// створюється точково, а не робить увесь CLI async.
fn block_on<F: std::future::Future<Output = Result<(), Box<dyn std::error::Error>>>>(
    future: F,
) -> Result<(), String> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?
        .block_on(future)
        .map_err(|e| e.to_string())
}

pub fn run_serve_cmd(args: ServeArgs, _json: bool) -> Result<(), String> {
    let relay = args.relay_url.clone().map(|url| RelayBridgeConfig {
        url,
        device_token: args.relay_token.clone(),
        root: args.relay_root.clone(),
    });
    block_on(run_serve(
        state_dir(args.state_dir),
        args.port,
        args.acp_cmd,
        relay,
        !args.no_orchestrator,
    ))
}

pub fn run_attach_cmd(args: AttachArgs, _json: bool) -> Result<(), String> {
    block_on(run_attach(
        state_dir(args.state_dir),
        args.node,
        args.lang,
    ))
}

async fn run_serve(
    dir: PathBuf,
    port: u16,
    acp_cmd: Option<String>,
    relay: Option<RelayBridgeConfig>,
    with_orchestrator: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let sessions = Arc::new(SessionHost::new(dir.join("sessions"))?);
    let gate = Arc::new(ApprovalGate::default());
    // Виконавець ходу — ACP-адаптер підписочного CLI; request_permission
    // мапиться на approval-гейт (ApprovalRequest у кімнату вузла, таймаут
    // 120s → відмова). Без адаптера — echo-заглушка транспорту.
    let runner: Arc<dyn TurnRunner> = match acp_cmd {
        Some(command) => {
            let approval_sessions = Arc::clone(&sessions);
            let approval_gate = Arc::clone(&gate);
            let factory: PermissionFactory = Arc::new(move |node: &str| {
                let sessions = Arc::clone(&approval_sessions);
                let gate = Arc::clone(&approval_gate);
                let node = node.to_string();
                let handler: PermissionHandler = Arc::new(move |action, diff| {
                    let sessions = Arc::clone(&sessions);
                    let gate = Arc::clone(&gate);
                    let node = node.clone();
                    Box::pin(async move {
                        let Ok(receiver) = request_approval(&sessions, &gate, &node, action, diff)
                        else {
                            return false;
                        };
                        matches!(
                            tokio::time::timeout(std::time::Duration::from_secs(120), receiver)
                                .await,
                            Ok(Ok(true))
                        )
                    })
                });
                handler
            });
            println!("ACP-адаптер: {command}");
            Arc::new(AcpTurnRunner::new(&command, Some(factory)))
        }
        None => Arc::new(EchoTurnRunner),
    };
    let token = Uuid::new_v4().to_string();
    let mut state = AppState::from_parts(sessions, gate, runner, Some(token.clone()));
    // Кімната = вузол графа, якщо запущено з кореня MT-проєкту (tasks-дир
    // `mt/` поряд): UserMessage веде claim/worktree, /done — fenced publish.
    let tasks_dir = std::env::current_dir()?.join("mt");
    if tasks_dir.is_dir() {
        state = state.with_graph(GraphConfig::new(tasks_dir));
    }
    let state = Arc::new(state);
    let (addr, handle) = serve(Arc::clone(&state), format!("127.0.0.1:{port}").parse()?).await?;
    let discovery = Discovery::new(dir);
    discovery.write(addr.port(), &token)?;
    println!("agent-server: ws://{addr}/ws (protocol v{PROTOCOL_VERSION})");
    // Міст до relay: віддалені пристрої бачать стрічку і шлють команди.
    let relay_bridge = relay.map(|config| {
        println!("relay-міст: {} (кімната {})", config.url, config.root);
        spawn_relay_bridge(Arc::clone(&state), config)
    });

    // Orchestrator-роль у тому самому процесі (overview.md: хост — це
    // orchestrator + runner + session host). Wake-петля живе в окремому
    // потоці: tick робить блокуючі git- і процес-виклики, у tokio-таску
    // вони б тримали виконавця.
    let orchestrator = with_orchestrator
        .then(|| {
            let tasks_dir = std::env::current_dir().ok()?.join("mt");
            if !tasks_dir.is_dir() {
                return None;
            }
            let project_root = tasks_dir.parent()?.to_path_buf();
            let mut wake = Wake::new(&project_root, Duration::from_secs(WAKE_FALLBACK_SEC));
            state.set_wake(wake.signaller());
            let tasks_dir = tasks_dir.to_string_lossy().into_owned();
            println!("orchestrator: {tasks_dir} (wake: relay push | .mt/wake | {WAKE_FALLBACK_SEC}s)");
            Some(std::thread::spawn(move || {
                let mut orch = Orchestrator::new(tasks_dir, 5);
                loop {
                    wake.wait();
                    let report = orch.tick();
                    for node in &report.alerts {
                        eprintln!("⚠ unresolvable: {node} — чекає людину");
                    }
                    for error in &report.errors {
                        eprintln!("orchestrator: {error}");
                    }
                }
            }))
        })
        .flatten();
    let _ = &orchestrator;

    tokio::signal::ctrl_c().await?;
    discovery.remove()?;
    if let Some(bridge) = relay_bridge {
        bridge.abort();
    }
    handle.abort();
    Ok(())
}

async fn run_attach(
    dir: PathBuf,
    node: String,
    lang: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let (port_file, token) = Discovery::new(dir).read().map_err(|error| {
        format!("discovery не знайдено ({error}); спершу запусти `agent-cli serve`")
    })?;
    let url = format!("ws://127.0.0.1:{}/ws", port_file.port);
    let (mut stream, _) = tokio_tungstenite::connect_async(&url).await?;

    let hello = ClientHello {
        protocol_version: PROTOCOL_VERSION,
        device_id: Uuid::new_v4(),
        device_token: token,
        client_kind: "cli".into(),
        client_capabilities: vec!["approvals".into(), "diff_view".into()],
        lang,
        want_replay_from: Some(0),
    };
    stream
        .send(Message::text(serde_json::to_string(&hello)?))
        .await?;

    let Some(Ok(Message::Text(first))) = stream.next().await else {
        return Err("сервер закрив зʼєднання на хендшейку".into());
    };
    if let Ok(Event::Error { message }) = serde_json::from_str::<Event>(first.as_str()) {
        return Err(message.into());
    }
    let server_hello: ServerHello = serde_json::from_str(first.as_str())?;
    println!(
        "підключено (v{}); сесій: {}. Пиши повідомлення, Ctrl-D — вихід.",
        server_hello.protocol_version,
        server_hello.session_list.len()
    );

    let mut stdin = tokio::io::BufReader::new(tokio::io::stdin()).lines();
    loop {
        tokio::select! {
            incoming = stream.next() => match incoming {
                Some(Ok(Message::Text(text))) => {
                    if let Ok(envelope) = serde_json::from_str::<Envelope>(text.as_str()) {
                        print_event(&node, &envelope);
                    }
                }
                Some(Ok(_)) => {}
                _ => break,
            },
            line = stdin.next_line() => match line? {
                Some(text) if !text.trim().is_empty() => {
                    // Команди сесії: /done — fenced publish fact у main,
                    // /release — пауза (відпустити claim).
                    let event = match text.trim() {
                        "/done" => Event::DoneSession {},
                        "/release" => Event::ReleaseSession {},
                        _ => Event::UserMessage { text, attachments: vec![], surface: Some("cli".into()) },
                    };
                    let envelope = Envelope {
                        seq: 0,
                        ts: chrono_now(),
                        node_hash: node.clone(),
                        run_token: Uuid::nil(),
                        device_id: Some(hello.device_id),
                        account_id: None,
                        event,
                    };
                    stream.send(Message::text(serde_json::to_string(&envelope)?)).await?;
                }
                Some(_) => {}
                None => break,
            },
        }
    }
    Ok(())
}

/// `agent-cli` не залежить від chrono напряму — бере реекспорт типу з
/// agent-protocol через Envelope; клієнтський ts сервер однаково ігнорує.
fn chrono_now() -> chrono::DateTime<chrono::Utc> {
    chrono::Utc::now()
}

fn print_event(node: &str, envelope: &Envelope) {
    if envelope.node_hash != node {
        return;
    }
    match &envelope.event {
        Event::AgentTextDelta { text } => {
            print!("{text}");
            let _ = std::io::stdout().flush();
        }
        Event::AgentTextDone {} => println!(),
        Event::UserMessage { text, .. } => println!("> {text}"),
        Event::ToolCall { name, .. } => println!("⚙ {name} …"),
        Event::ToolResult { ok, summary, .. } => {
            println!("{} {summary}", if *ok { "✓" } else { "✗" })
        }
        Event::Committed {
            commit_hash,
            message,
        } => println!("✔ {message} ({commit_hash})"),
        Event::ClaimChanged {
            holder_device_id: None,
            ..
        } => println!("⏸ claim відпущено — вузол вільний, журнал у run ref"),
        Event::Error { message } => eprintln!("помилка: {message}"),
        _ => {}
    }
}
