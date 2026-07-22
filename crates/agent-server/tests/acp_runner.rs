//! Інтеграція AcpTurnRunner з реальним child-процесом: фейковий ACP-агент
//! (bin `fake-acp-agent`) на stdio — хід повертає echo-текст і емітить
//! AgentTextDelta/AgentTextDone.

use std::sync::Mutex;

use agent_protocol::Event;
use agent_server::{AcpTurnRunner, TurnRunner};

#[tokio::test(flavor = "multi_thread")]
async fn acp_turn_runner_spawns_adapter_and_streams_turn() {
    let runner = AcpTurnRunner::new(env!("CARGO_BIN_EXE_fake-acp-agent"), None);
    let events = Mutex::new(Vec::new());
    let emit = |event: Event| events.lock().unwrap().push(event);

    let first = runner.run_turn("room-1", "раз", None, &emit).await.unwrap();
    let second = runner.run_turn("room-1", "два", None, &emit).await.unwrap();

    assert_eq!((first.as_str(), second.as_str()), ("end_turn", "end_turn"));
    assert_eq!(
        *events.lock().unwrap(),
        vec![
            Event::AgentTextDelta {
                text: "acp: раз".into()
            },
            Event::AgentTextDone {},
            Event::AgentTextDelta {
                text: "acp: два".into()
            },
            Event::AgentTextDone {},
        ]
    );
}
