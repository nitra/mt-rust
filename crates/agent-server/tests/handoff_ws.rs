//! Кооперативний handoff на рівні AppState/session (runtime.md, «Міграція
//! сесії між хостами», кроки 2-3): дві незалежні `AppState` (окремі
//! `state_dir` — симуляція двох хостів), той самий git-репозиторій.
//! Хід на хості 1 → `handoff_node` → `resume_node` на хості 2 з тим самим
//! тікетом → журнал успадкований, наступний хід продовжує seq без розривів.

use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use agent_protocol::{Envelope, Event};
use agent_server::{serve, AppState, ApprovalGate, GraphConfig, ScriptedTurnRunner, SessionHost};
use futures::SinkExt;
use tokio_tungstenite::tungstenite::Message;
use uuid::Uuid;

mod common;
use common::{next_json, WsStream};

fn sh(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_AUTHOR_NAME", "test")
        .env("GIT_AUTHOR_EMAIL", "t@t.local")
        .env("GIT_COMMITTER_NAME", "test")
        .env("GIT_COMMITTER_EMAIL", "t@t.local")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Bare-origin + робочий клон із вузлом `mt/demo` — спільна координатна
/// точка «двох хостів» (обидва працюють у тому самому локальному клоні;
/// реалістичніше було б два окремі клони, але git-операції йдуть через
/// origin однаково, а тест — послідовний, без гонки між хостами).
struct Fixture {
    #[allow(dead_code)]
    origin: tempfile::TempDir,
    work: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let origin = tempfile::tempdir().unwrap();
        sh(origin.path(), &["init", "--bare", "-q", "-b", "main"]);
        let work = tempfile::tempdir().unwrap();
        sh(work.path(), &["init", "-q", "-b", "main"]);
        std::fs::create_dir_all(work.path().join("mt/demo")).unwrap();
        std::fs::write(work.path().join("mt/demo/task.md"), "## Task\n").unwrap();
        sh(work.path(), &["add", "."]);
        sh(work.path(), &["commit", "-q", "-m", "init"]);
        sh(
            work.path(),
            &["remote", "add", "origin", origin.path().to_str().unwrap()],
        );
        sh(work.path(), &["push", "-q", "origin", "main"]);
        Self { origin, work }
    }

    fn config(&self) -> GraphConfig {
        GraphConfig::new(self.work.path().join("mt"))
    }
}

/// Стартує AppState (свій `state_dir`, скриптований runner) + WS-сервер.
async fn start_host(
    fixture: &Fixture,
    responses: Vec<&str>,
) -> (Arc<AppState>, String, tempfile::TempDir) {
    let runner = ScriptedTurnRunner::new(responses);
    let state_dir = tempfile::tempdir().unwrap();
    let state = Arc::new(
        AppState::from_parts(
            Arc::new(SessionHost::new(state_dir.path().to_path_buf()).unwrap()),
            Arc::new(ApprovalGate::default()),
            Arc::new(runner),
            None,
        )
        .with_graph(fixture.config()),
    );
    let (addr, _handle) = serve(Arc::clone(&state), "127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    (state, format!("ws://{addr}/ws"), state_dir)
}

/// WS-клієнт цього тест-бінарника (обидва «хости» — той самий device_id 1,
/// як у вихідному сценарії handoff).
async fn connect(url: &str) -> WsStream {
    common::connect(url, 1).await
}

async fn next_matching(stream: &mut WsStream, matches_event: impl Fn(&Event) -> bool) -> Envelope {
    loop {
        let envelope: Envelope = next_json(stream).await;
        if matches_event(&envelope.event) {
            return envelope;
        }
    }
}

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

/// Наскрізно: хід на хості 1 → handoff_node → resume_node на хості 2 з тим
/// самим тікетом → журнал успадкований (get_or_open бачить хід хоста 1) →
/// наступний хід продовжує seq без розривів.
#[tokio::test(flavor = "multi_thread")]
async fn handoff_then_resume_inherits_journal_and_continues_seq() {
    let fixture = Fixture::new();

    // Хост 1: хід, що завершується AgentTextDone.
    let (host1, url1, _dir1) = start_host(&fixture, vec!["перший хост"]).await;
    let mut client1 = connect(&url1).await;
    client1.send(user_message("demo", "почни")).await.unwrap();
    let user_envelope =
        next_matching(&mut client1, |e| matches!(e, Event::UserMessage { .. })).await;
    next_matching(&mut client1, |e| matches!(e, Event::AgentTextDone {})).await;
    drop(client1);

    let ticket = host1.handoff_node("demo").await.unwrap();
    assert_eq!(ticket.generation, 1);

    // Хост 2: інший AppState (інший state_dir), той самий тікет.
    let (host2, url2, _dir2) = start_host(&fixture, vec!["другий хост"]).await;
    host2.resume_node("demo", &ticket).await.unwrap();

    // Журнал хоста 1 успадкований локальною сесією хоста 2 ще ДО будь-якого
    // нового ходу.
    let inherited = host2.sessions.get_or_open("demo").unwrap().replay_from(0);
    assert!(
        inherited
            .iter()
            .any(|e| e.event == user_envelope.event && e.seq == user_envelope.seq),
        "{inherited:?}"
    );
    let last_inherited_seq = inherited.last().unwrap().seq;

    // Новий хід на хості 2 продовжує seq без розривів.
    let mut client2 = connect(&url2).await;
    client2.send(user_message("demo", "продовж")).await.unwrap();
    let second_user = next_matching(&mut client2, |e| matches!(e, Event::UserMessage { .. })).await;
    assert_eq!(
        second_user.seq,
        last_inherited_seq + 1,
        "seq продовжується без розривів після resume"
    );
    next_matching(&mut client2, |e| matches!(e, Event::AgentTextDone {})).await;
}
