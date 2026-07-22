//! Сесії: збірка `Envelope`, журнал `session.jsonl`, broadcast, реплей
//! (спека runtime.md, «Протокол подій» і «Інтерактивна сесія = run вузла»).
//!
//! `seq` монотонний у межах run і призначається хостом (тримачем claim).
//! Ефемерні події (`AgentTextDelta`, `PreviewScreenshot`) не журналяться —
//! журналиться `AgentTextDone`-агрегат; решта — append-only рядки
//! `session.jsonl`, з якого сесія відновлюється після рестарту хоста.

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use agent_protocol::{Envelope, Event};
use chrono::Utc;
use tokio::sync::broadcast;
use uuid::Uuid;

/// Ефемерні події: лише relay/WS, ніколи в журнал чи git.
pub fn is_ephemeral(event: &Event) -> bool {
    matches!(
        event,
        Event::AgentTextDelta { .. } | Event::PreviewScreenshot { .. }
    )
}

/// Одна сесія (run вузла): лічильник seq, журнал, файл `session.jsonl`.
pub struct Session {
    pub node_hash: String,
    pub run_token: Uuid,
    journal_path: PathBuf,
    state: Mutex<SessionState>,
}

struct SessionState {
    next_seq: u64,
    journal: Vec<Envelope>,
}

impl Session {
    /// Відкриває сесію: якщо `session.jsonl` існує — відновлює журнал і
    /// продовжує seq з останнього запису (реплей після рестарту хоста).
    fn open(dir: &Path, node_hash: &str) -> std::io::Result<Self> {
        let journal_path = dir.join(format!("{node_hash}.session.jsonl"));
        let mut journal: Vec<Envelope> = Vec::new();
        if journal_path.exists() {
            for line in fs::read_to_string(&journal_path)?.lines() {
                if let Ok(envelope) = serde_json::from_str::<Envelope>(line) {
                    journal.push(envelope);
                }
            }
        }
        let next_seq = journal.last().map(|envelope| envelope.seq + 1).unwrap_or(0);
        let run_token = journal
            .last()
            .map(|envelope| envelope.run_token)
            .unwrap_or_else(Uuid::new_v4);
        Ok(Self {
            node_hash: node_hash.to_string(),
            run_token,
            journal_path,
            state: Mutex::new(SessionState { next_seq, journal }),
        })
    }

    /// Збирає `Envelope` (хост призначає seq і ts), журналить
    /// неефемерні події та повертає конверт для broadcast.
    pub fn append(
        &self,
        event: Event,
        device_id: Option<Uuid>,
        account_id: Option<Uuid>,
    ) -> Envelope {
        let mut state = self.state.lock().unwrap();
        let envelope = Envelope {
            seq: state.next_seq,
            ts: Utc::now(),
            node_hash: self.node_hash.clone(),
            run_token: self.run_token,
            device_id,
            account_id,
            event,
        };
        state.next_seq += 1;
        if !is_ephemeral(&envelope.event) {
            state.journal.push(envelope.clone());
            // Append-only запис; помилка диска не валить сесію — журнал
            // лишається в памʼяті, персистентність відновиться наступним записом.
            if let Ok(mut file) = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.journal_path)
            {
                let _ = writeln!(file, "{}", serde_json::to_string(&envelope).unwrap());
            }
        }
        envelope
    }

    /// Журнальовані події з `seq >= from` (реплей для реконекту).
    pub fn replay_from(&self, from: u64) -> Vec<Envelope> {
        self.state
            .lock()
            .unwrap()
            .journal
            .iter()
            .filter(|envelope| envelope.seq >= from)
            .cloned()
            .collect()
    }
}

/// Реєстр сесій хоста + один спільний broadcast-канал усіх кімнат
/// (клієнт фільтрує за node_hash/capabilities на боці хоста при відправці).
pub struct SessionHost {
    state_dir: PathBuf,
    sessions: Mutex<HashMap<String, std::sync::Arc<Session>>>,
    broadcast: broadcast::Sender<Envelope>,
}

impl SessionHost {
    pub fn new(state_dir: PathBuf) -> std::io::Result<Self> {
        fs::create_dir_all(&state_dir)?;
        let (broadcast, _) = broadcast::channel(1024);
        Ok(Self {
            state_dir,
            sessions: Mutex::new(HashMap::new()),
            broadcast,
        })
    }

    /// Засіває журнал сесії напряму у файл (attach_resume після handoff:
    /// новий хост успадковує `.nitra/session.jsonl` вже готовим — той самий
    /// формат, що й локальний журнал). Наступний `get_or_open` прочитає
    /// його як після рестарту хоста — продовжить seq/run_token природно.
    /// Помилка, якщо сесія для цього ключа вже відкрита: живий стан у
    /// пам'яті заднім числом не перечитується, сіяти треба ДО першого
    /// `get_or_open`.
    pub fn seed_journal(&self, node: &str, jsonl: &str) -> std::io::Result<()> {
        if self.sessions.lock().unwrap().contains_key(node) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                format!("seed_journal: сесія {node} вже відкрита"),
            ));
        }
        fs::write(self.state_dir.join(format!("{node}.session.jsonl")), jsonl)
    }

    /// Сесія кімнати; створюється (або відновлюється з журналу) ліниво.
    pub fn get_or_open(&self, node_hash: &str) -> std::io::Result<std::sync::Arc<Session>> {
        let mut sessions = self.sessions.lock().unwrap();
        if let Some(session) = sessions.get(node_hash) {
            return Ok(std::sync::Arc::clone(session));
        }
        let session = std::sync::Arc::new(Session::open(&self.state_dir, node_hash)?);
        sessions.insert(node_hash.to_string(), std::sync::Arc::clone(&session));
        Ok(session)
    }

    /// Append у сесію + broadcast підключеним клієнтам.
    pub fn publish(
        &self,
        session: &Session,
        event: Event,
        device_id: Option<Uuid>,
        account_id: Option<Uuid>,
    ) -> Envelope {
        let envelope = session.append(event, device_id, account_id);
        let _ = self.broadcast.send(envelope.clone());
        envelope
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Envelope> {
        self.broadcast.subscribe()
    }

    /// Активні сесії для `ServerHello.session_list`.
    pub fn session_list(&self) -> Vec<agent_protocol::SessionInfo> {
        self.sessions
            .lock()
            .unwrap()
            .values()
            .map(|session| agent_protocol::SessionInfo {
                node_hash: session.node_hash.clone(),
                run_token: session.run_token,
            })
            .collect()
    }

    /// Реплей журнальованих подій усіх сесій із `seq >= from`,
    /// стабільно впорядкований за (node_hash, seq).
    pub fn replay_from(&self, from: u64) -> Vec<Envelope> {
        let sessions = self.sessions.lock().unwrap();
        let mut envelopes: Vec<Envelope> = sessions
            .values()
            .flat_map(|session| session.replay_from(from))
            .collect();
        envelopes.sort_by(|a, b| (&a.node_hash, a.seq).cmp(&(&b.node_hash, b.seq)));
        envelopes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host() -> (tempfile::TempDir, SessionHost) {
        let dir = tempfile::tempdir().unwrap();
        let host = SessionHost::new(dir.path().to_path_buf()).unwrap();
        (dir, host)
    }

    /// seq монотонний; ефемерні події не потрапляють у журнал.
    #[test]
    fn seq_is_monotonic_and_ephemeral_events_skip_journal() {
        let (_dir, host) = host();
        let session = host.get_or_open("room-1").unwrap();

        let first = session.append(
            Event::UserMessage {
                text: "привіт".into(),
                attachments: vec![],
                surface: None,
            },
            None,
            None,
        );
        let delta = session.append(Event::AgentTextDelta { text: "п".into() }, None, None);
        let done = session.append(Event::AgentTextDone {}, None, None);

        assert_eq!((first.seq, delta.seq, done.seq), (0, 1, 2));
        let journaled: Vec<u64> = session
            .replay_from(0)
            .iter()
            .map(|envelope| envelope.seq)
            .collect();
        assert_eq!(journaled, vec![0, 2], "ефемерна дельта не журналиться");
    }

    /// Сесія відновлюється з session.jsonl: журнал, seq і run_token
    /// переживають «рестарт хоста».
    #[test]
    fn session_restores_from_journal_file() {
        let dir = tempfile::tempdir().unwrap();
        let (token, last_seq) = {
            let host = SessionHost::new(dir.path().to_path_buf()).unwrap();
            let session = host.get_or_open("room-1").unwrap();
            session.append(Event::AgentTextDone {}, None, None);
            let last = session.append(
                Event::Committed {
                    commit_hash: "abc".into(),
                    message: "fix".into(),
                },
                None,
                None,
            );
            (session.run_token, last.seq)
        };

        // «Новий процес» над тим самим state_dir.
        let host = SessionHost::new(dir.path().to_path_buf()).unwrap();
        let session = host.get_or_open("room-1").unwrap();
        assert_eq!(session.run_token, token, "run_token успадковано з журналу");
        let next = session.append(Event::AgentTextDone {}, None, None);
        assert_eq!(next.seq, last_seq + 1, "seq продовжується, без розривів");
        assert_eq!(session.replay_from(0).len(), 3);
    }

    /// publish доставляє конверт підписникам broadcast.
    #[tokio::test]
    async fn publish_broadcasts_to_subscribers() {
        let (_dir, host) = host();
        let session = host.get_or_open("room-1").unwrap();
        let mut receiver = host.subscribe();

        host.publish(&session, Event::AgentTextDone {}, None, None);

        let received = receiver.recv().await.unwrap();
        assert_eq!(received.node_hash, "room-1");
        assert_eq!(received.event, Event::AgentTextDone {});
    }

    /// seed_journal: сіяний журнал підхоплюється наступним get_or_open —
    /// той самий механізм відновлення, що й після рестарту хоста.
    #[test]
    fn seed_journal_is_picked_up_by_next_open() {
        let (_dir, host) = host();
        let seeded = Envelope {
            seq: 0,
            ts: Utc::now(),
            node_hash: "room-1".into(),
            run_token: Uuid::from_u128(9),
            device_id: None,
            account_id: None,
            event: Event::UserMessage {
                text: "успадковано".into(),
                attachments: vec![],
                surface: None,
            },
        };
        let jsonl = format!("{}\n", serde_json::to_string(&seeded).unwrap());
        host.seed_journal("room-1", &jsonl).unwrap();

        let session = host.get_or_open("room-1").unwrap();
        assert_eq!(session.run_token, Uuid::from_u128(9));
        assert_eq!(session.replay_from(0), vec![seeded]);

        let next = session.append(Event::AgentTextDone {}, None, None);
        assert_eq!(next.seq, 1, "seq продовжується від сіяного журналу");
    }

    /// seed_journal після відкриття сесії — явна помилка (живий стан у
    /// пам'яті заднім числом не перечитується).
    #[test]
    fn seed_journal_after_open_is_rejected() {
        let (_dir, host) = host();
        host.get_or_open("room-1").unwrap();
        let error = host.seed_journal("room-1", "").unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
    }
}
