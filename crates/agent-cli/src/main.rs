//! Тонкий клієнт agent-server (M1-заділ `mt serve`/`mt attach`).
//!
//! `serve` — стартує хост-процес: WS на 127.0.0.1, discovery port-file +
//! токен; runner — ACP-адаптер підписочного CLI (`--acp-cmd` або env
//! `MT_ACP_AGENT_CMD`; ADR `260713-2110`: ACP — єдиний транспорт
//! AI-викликів), без нього — echo-заглушка транспорту.
//! `attach <node>` — читає discovery, хендшейк v4, REPL: stdin →
//! `UserMessage`, стрічка подій → термінал. M1-заділ адресує кімнату
//! рядком вузла; hash-адресація і graph-операції (claim/publish через
//! `mt … --json`) — окрема задача інтеграції.

use std::io::Write as _;
use std::path::PathBuf;
use std::sync::Arc;

use agent_core::PermissionHandler;
use agent_protocol::{ClientHello, Envelope, Event, ServerHello, PROTOCOL_VERSION};
use agent_server::approvals_gate::request_approval;
use agent_server::{
    serve, spawn_relay_bridge, AcpTurnRunner, AppState, ApprovalGate, Discovery, EchoTurnRunner,
    GraphConfig, PermissionFactory, RelayBridgeConfig, SessionHost, TurnRunner,
};
use clap::{Parser, Subcommand};
use futures::{SinkExt, StreamExt};
use tokio::io::AsyncBufReadExt;
use tokio_tungstenite::tungstenite::Message;
use uuid::Uuid;

#[derive(Parser)]
#[command(name = "agent-cli", about = "Тонкий клієнт agent-server (M1)")]
struct Cli {
    /// Директорія discovery/стану (дефолт — ~/.nitra).
    #[arg(long, global = true)]
    state_dir: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Запустити хост-процес (WS + discovery).
    Serve {
        /// Порт (0 — ефемерний).
        #[arg(long, default_value_t = 0)]
        port: u16,
        /// Команда ACP-адаптера підписочного CLI (напр. `npx claude-code-acp`);
        /// без прапора береться env `MT_ACP_AGENT_CMD`, без обох — echo-заглушка.
        #[arg(long, env = "MT_ACP_AGENT_CMD")]
        acp_cmd: Option<String>,
        /// Адреса relay (`ws://…`/`wss://…`) — вмикає міст до relay.
        #[arg(long)]
        relay_url: Option<String>,
        /// device_token host-пристрою на relay.
        #[arg(long, default_value = "")]
        relay_token: String,
        /// Кімната relay (кореневий вузол задачі).
        #[arg(long, default_value = "")]
        relay_root: String,
    },
    /// Підключитись до вузла інтерактивною сесією.
    Attach {
        /// Вузол (шлях у tasks-директорії).
        node: String,
        /// BCP-47 мова учасника (обовʼязкове поле v4).
        #[arg(long, default_value = "uk")]
        lang: String,
    },
}

fn state_dir(cli_dir: Option<PathBuf>) -> PathBuf {
    cli_dir.unwrap_or_else(|| {
        PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into())).join(".nitra")
    })
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    match cli.command {
        Command::Serve {
            port,
            acp_cmd,
            relay_url,
            relay_token,
            relay_root,
        } => {
            let relay = relay_url.map(|url| RelayBridgeConfig {
                url,
                device_token: relay_token,
                root: relay_root,
            });
            run_serve(state_dir(cli.state_dir), port, acp_cmd, relay).await
        }
        Command::Attach { node, lang } => run_attach(state_dir(cli.state_dir), node, lang).await,
    }
}

async fn run_serve(
    dir: PathBuf,
    port: u16,
    acp_cmd: Option<String>,
    relay: Option<RelayBridgeConfig>,
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
