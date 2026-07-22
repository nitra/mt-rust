//! Фейковий ACP-агент для інтеграційних тестів AcpTurnRunner (без мережі й
//! LLM): ndjson JSON-RPC на stdio — відповідає на initialize/session\/new,
//! на session/prompt шле chunk-нотифікацію з echo-текстом і end_turn.

use std::io::{BufRead, Write};

fn main() {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let message: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let id = message["id"].clone();
        let reply = match message["method"].as_str() {
            Some("initialize") => vec![serde_json::json!({
                "jsonrpc": "2.0", "id": id, "result": { "protocolVersion": 1 }
            })],
            Some("session/new") => vec![serde_json::json!({
                "jsonrpc": "2.0", "id": id, "result": { "sessionId": "fake" }
            })],
            Some("session/prompt") => {
                let text = message["params"]["prompt"][0]["text"]
                    .as_str()
                    .unwrap_or("");
                vec![
                    serde_json::json!({
                        "jsonrpc": "2.0", "method": "session/update",
                        "params": { "sessionId": "fake", "update": {
                            "sessionUpdate": "agent_message_chunk",
                            "content": { "type": "text", "text": format!("acp: {text}") } } }
                    }),
                    serde_json::json!({
                        "jsonrpc": "2.0", "id": id, "result": { "stopReason": "end_turn" }
                    }),
                ]
            }
            _ => continue,
        };
        for frame in reply {
            let _ = writeln!(stdout, "{frame}");
            let _ = stdout.flush();
        }
    }
}
