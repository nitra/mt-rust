//! Orchestrator-роль хоста (runtime.md, «Wake: push замість polling»).
//!
//! Базовий MT прокидався cron-ом кожні 5 хвилин. Тут — подієвий wake із
//! трьома джерелами:
//!
//! 1. relay push «є нові події у задачі X» → [`Wake::signal`];
//! 2. `post-merge` git hook → `touch .mt/wake`;
//! 3. періодичний tick — **fallback**, щоб система працювала як базовий MT,
//!    коли relay недоступний.
//!
//! На кожен wake виконується та сама трійка `mt watch`-логіки: **dispatch**
//! (запуск готових вузлів), **алерти** (вузли, що стали `unresolvable`) і
//! **GC** (прибирання відпрацьованих worktree).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

/// Підсумок одного прокидання — те, що хост має відзвітувати назовні.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct TickReport {
    /// Вузли, запущені цим прокиданням, і їхні результати.
    pub dispatched: Vec<(String, String)>,
    /// Вузли, що вперше стали `unresolvable` — алерт власнику.
    pub alerts: Vec<String>,
    /// Вузли з відкритим аудит-циклом, які цей tick віддав аудиторові.
    pub audited: Vec<String>,
    /// Прибрані worktree.
    pub pruned: Vec<String>,
    /// Вузли, чий derived-стан змінився з попереднього прокидання —
    /// джерело `NodeState` для `mt-dashboard` (runtime.md).
    pub state_changes: Vec<(String, String)>,
    /// Помилки, які не мають валити цикл (наступний wake спробує знову).
    pub errors: Vec<String>,
}

/// Джерело пробуджень: явний сигнал (relay push), файл-мітка `.mt/wake`
/// (git hook) і періодичний fallback.
pub struct Wake {
    wake_file: PathBuf,
    last_seen: Option<SystemTime>,
    signalled: Arc<AtomicBool>,
    interval: Duration,
}

impl Wake {
    pub fn new(project_root: &Path, interval: Duration) -> Self {
        let wake_file = project_root.join(".mt/wake");
        Self {
            last_seen: file_mtime(&wake_file),
            wake_file,
            signalled: Arc::new(AtomicBool::new(false)),
            interval,
        }
    }

    /// Ручка для relay-клієнта: push «є нові події у задачі X» будить хост
    /// негайно, не чекаючи періодичного tick-а.
    pub fn signaller(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.signalled)
    }

    /// Чи є привід прокинутись **зараз** — без блокування.
    ///
    /// Споживає сигнал і оновлює позначку часу файлу, тож повторний виклик
    /// без нової події поверне `false`: один привід — одне прокидання.
    pub fn should_wake_now(&mut self) -> bool {
        if self.signalled.swap(false, Ordering::SeqCst) {
            return true;
        }
        let current = file_mtime(&self.wake_file);
        if current.is_some() && current != self.last_seen {
            self.last_seen = current;
            return true;
        }
        false
    }

    /// Чекає на привід не довше за `interval`; повертає `true`, якщо
    /// прокинулись за подією, і `false`, якщо спрацював fallback-таймер.
    ///
    /// Опитування файлу — свідомий компроміс: інотифай додав би залежність
    /// і платформні гілки заради того, що на масштабі одного репозиторію
    /// коштує один `stat` на 200 мс.
    pub fn wait(&mut self) -> bool {
        let deadline = SystemTime::now() + self.interval;
        loop {
            if self.should_wake_now() {
                return true;
            }
            if SystemTime::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(200).min(self.interval));
        }
    }
}

/// Черга аудиту (graph.md, «Аудит (async черга)»): вузли у стані
/// `pending-audit`. Саме її розбирає orchestrator на кожному прокиданні —
/// це і є тригер `audit_schedule_days`, тільки подієвий, а не за таймером:
/// цикл, відкритий сигналом `mt audit`, не має чекати наступної доби.
///
/// Порядок детермінований (за шляхом) — щоб два хости, які прокинулись
/// одночасно, бралися за чергу однаково, а не змагались хаотично.
pub fn audit_queue(nodes: &[mt_core::TaskNode]) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack: Vec<&mt_core::TaskNode> = nodes.iter().collect();
    while let Some(node) = stack.pop() {
        if node.state == mt_core::TaskState::PendingAudit {
            out.push(node.path.clone());
        }
        stack.extend(node.children.iter());
    }
    out.sort();
    out
}

fn file_mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).ok()?.modified().ok()
}

/// Стан orchestrator-ролі між прокиданнями.
pub struct Orchestrator {
    tasks_dir: String,
    project_root: PathBuf,
    concurrency: usize,
    /// Вузли, за які алерт уже відправлено — щоб кожне прокидання не
    /// повторювало той самий алерт про той самий термінальний вузол.
    alerted: HashSet<String>,
    /// Derived-стани з попереднього прокидання. Дашборду потрібні **зміни**,
    /// а не знімок: інакше кожен tick слав би стан усього графа, і стрічка
    /// перетворилась би на періодичний дамп.
    states: HashMap<String, mt_core::TaskState>,
}

impl Orchestrator {
    pub fn new(tasks_dir: impl Into<String>, concurrency: usize) -> Self {
        let tasks_dir = tasks_dir.into();
        let project_root = Path::new(&tasks_dir)
            .parent()
            .unwrap_or(Path::new("."))
            .to_path_buf();
        Self {
            tasks_dir,
            project_root,
            concurrency: concurrency.max(1),
            alerted: HashSet::new(),
            states: HashMap::new(),
        }
    }

    /// Вузли, що потребують алерту цього прокидання: у стані `unresolvable`
    /// і ще не оголошені.
    ///
    /// Дедуплікація за станом процесу, а не за файлом-міткою: алерт — це
    /// доставка, а не артефакт графа, і перезапуск хоста має право нагадати
    /// про вузол, який усе ще чекає людину.
    fn pending_alerts(&mut self, nodes: &[mt_core::TaskNode]) -> Vec<String> {
        let mut out = Vec::new();
        let mut stack: Vec<&mt_core::TaskNode> = nodes.iter().collect();
        while let Some(node) = stack.pop() {
            if node.state == mt_core::TaskState::Unresolvable && !self.alerted.contains(&node.path)
            {
                self.alerted.insert(node.path.clone());
                out.push(node.path.clone());
            }
            stack.extend(node.children.iter());
        }
        out.sort();
        out
    }

    /// Вузли, чий стан змінився з попереднього прокидання.
    ///
    /// Перше прокидання після старту віддає **весь** граф: для дашборда,
    /// що підключився до щойно піднятого хоста, «нічого не змінилось» і
    /// «нічого немає» — різні речі, і початковий знімок їх розрізняє.
    fn state_changes(&mut self, nodes: &[mt_core::TaskNode]) -> Vec<(String, String)> {
        let mut out = Vec::new();
        let mut stack: Vec<&mt_core::TaskNode> = nodes.iter().collect();
        while let Some(node) = stack.pop() {
            let changed = self
                .states
                .get(&node.path)
                .is_none_or(|previous| *previous != node.state);
            if changed {
                self.states.insert(node.path.clone(), node.state.clone());
                out.push((
                    node.path.clone(),
                    serde_json::to_value(&node.state)
                        .ok()
                        .and_then(|value| value.as_str().map(str::to_string))
                        .unwrap_or_default(),
                ));
            }
            stack.extend(node.children.iter());
        }
        out.sort();
        out
    }

    /// Одне прокидання: dispatch → алерти → GC.
    ///
    /// Порядок не випадковий. Dispatch першим, бо саме заради нього хост і
    /// прокидається; алерти після нього, щоб вузол, який щойно став
    /// `unresolvable` у цьому ж прогоні, потрапив у той самий звіт; GC
    /// останнім — він прибирає за тим, що вже завершилось.
    pub fn tick(&mut self) -> TickReport {
        let mut report = TickReport::default();

        match mt_core::orchestrate::run_auto(&self.tasks_dir, self.concurrency) {
            Ok(results) => {
                for r in results {
                    if let Some(error) = r.error {
                        report.errors.push(format!("{}: {error}", r.path));
                    }
                    report.dispatched.push((r.path, r.result));
                }
            }
            Err(error) => report.errors.push(format!("dispatch: {error}")),
        }

        match mt_core::scan_tasks_with_claims(self.tasks_dir.clone(), Vec::new()) {
            Ok(nodes) => {
                report.alerts = self.pending_alerts(&nodes);
                report.state_changes = self.state_changes(&nodes);
                let queue = audit_queue(&nodes);
                for path in queue {
                    match mt_core::audit::run_auditor(
                        &self.tasks_dir,
                        &path,
                        &mt_core::config::agent_cli_env_from_process(),
                    ) {
                        Ok(_) => report.audited.push(path),
                        Err(error) => report.errors.push(format!("audit {path}: {error}")),
                    }
                }
            }
            Err(error) => report.errors.push(format!("scan: {error}")),
        }

        match mt_core::worktree::prune_worktrees(&self.project_root) {
            Ok(output) => {
                report.pruned = output
                    .lines()
                    .map(str::trim)
                    .filter(|l| !l.is_empty())
                    .map(String::from)
                    .collect();
            }
            Err(error) => report.errors.push(format!("gc: {error}")),
        }

        report
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(path: &str, state: mt_core::TaskState) -> mt_core::TaskNode {
        mt_core::TaskNode {
            id: path.rsplit('/').next().unwrap_or(path).to_string(),
            path: path.to_string(),
            state,
            deps: Vec::new(),
            mode: "agent".to_string(),
            budget_sec: None,
            budget_hard_sec: None,
            deadline: None,
            hint: None,
            created_at: None,
            children: Vec::new(),
            is_composite: false,
            warnings: Vec::new(),
        }
    }

    #[test]
    fn first_tick_reports_whole_graph_then_only_changes() {
        // Для дашборда, що підключився до щойно піднятого хоста,
        // «нічого не змінилось» і «нічого немає» — різні речі.
        let mut orch = Orchestrator::new("mt", 1);
        let nodes = vec![
            node("alpha", mt_core::TaskState::Waiting),
            node("beta", mt_core::TaskState::Resolved),
        ];
        assert_eq!(
            orch.state_changes(&nodes),
            [
                ("alpha".to_string(), "waiting".to_string()),
                ("beta".to_string(), "resolved".to_string())
            ]
        );
        // Незмінний граф не шле нічого — інакше стрічка стала б
        // періодичним дампом стану.
        assert!(orch.state_changes(&nodes).is_empty());
    }

    #[test]
    fn state_change_is_reported_once() {
        let mut orch = Orchestrator::new("mt", 1);
        orch.state_changes(&[node("alpha", mt_core::TaskState::Waiting)]);

        let moved = vec![node("alpha", mt_core::TaskState::Running)];
        assert_eq!(
            orch.state_changes(&moved),
            [("alpha".to_string(), "running".to_string())]
        );
        assert!(orch.state_changes(&moved).is_empty());
    }

    #[test]
    fn state_changes_reach_nested_nodes() {
        let mut orch = Orchestrator::new("mt", 1);
        let mut parent = node("parent", mt_core::TaskState::Spawned);
        parent
            .children
            .push(node("parent/child", mt_core::TaskState::Waiting));
        let paths: Vec<String> = orch
            .state_changes(&[parent])
            .into_iter()
            .map(|(path, _)| path)
            .collect();
        assert_eq!(paths, ["parent", "parent/child"]);
    }

    #[test]
    fn alerts_fire_once_per_node() {
        let mut orch = Orchestrator::new("mt", 1);
        let nodes = vec![
            node("solo", mt_core::TaskState::Unresolvable),
            node("other", mt_core::TaskState::Waiting),
        ];
        assert_eq!(orch.pending_alerts(&nodes), ["solo"]);
        // Наступне прокидання не повторює алерт про той самий вузол.
        assert!(orch.pending_alerts(&nodes).is_empty());
    }

    #[test]
    fn alerts_reach_nested_nodes() {
        let mut orch = Orchestrator::new("mt", 1);
        let mut parent = node("parent", mt_core::TaskState::Spawned);
        parent
            .children
            .push(node("parent/child", mt_core::TaskState::Unresolvable));
        assert_eq!(orch.pending_alerts(&[parent]), ["parent/child"]);
    }

    #[test]
    fn audit_queue_collects_open_cycles_deterministically() {
        let mut parent = node("b-parent", mt_core::TaskState::Spawned);
        parent
            .children
            .push(node("b-parent/child", mt_core::TaskState::PendingAudit));
        let nodes = vec![
            node("a-solo", mt_core::TaskState::PendingAudit),
            parent,
            node("c-done", mt_core::TaskState::Resolved),
        ];
        // Лише відкриті цикли, і в стабільному порядку — щоб два хости,
        // які прокинулись разом, бралися за чергу однаково.
        assert_eq!(audit_queue(&nodes), ["a-solo", "b-parent/child"]);
    }

    #[test]
    fn audit_queue_is_empty_without_open_cycles() {
        let nodes = vec![
            node("solo", mt_core::TaskState::Waiting),
            node("done", mt_core::TaskState::Resolved),
        ];
        assert!(audit_queue(&nodes).is_empty());
    }

    #[test]
    fn explicit_signal_wakes_immediately() {
        let tmp = tempfile::tempdir().unwrap();
        let mut wake = Wake::new(tmp.path(), Duration::from_secs(3600));
        assert!(!wake.should_wake_now(), "без події — не будимо");

        wake.signaller().store(true, Ordering::SeqCst);
        assert!(wake.should_wake_now(), "relay push будить негайно");
        // Сигнал спожито: один привід — одне прокидання.
        assert!(!wake.should_wake_now());
    }

    #[test]
    fn touching_wake_file_wakes_host() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".mt")).unwrap();
        let wake_file = tmp.path().join(".mt/wake");
        std::fs::write(&wake_file, "").unwrap();

        let mut wake = Wake::new(tmp.path(), Duration::from_secs(3600));
        assert!(
            !wake.should_wake_now(),
            "наявний файл сам по собі — не подія"
        );

        // git hook торкається файлу після мерджу.
        std::thread::sleep(Duration::from_millis(10));
        filetime_touch(&wake_file);
        assert!(wake.should_wake_now());
        assert!(!wake.should_wake_now(), "повторно за тим самим mtime — ні");
    }

    #[test]
    fn periodic_fallback_returns_without_event() {
        let tmp = tempfile::tempdir().unwrap();
        let mut wake = Wake::new(tmp.path(), Duration::from_millis(50));
        // Relay недоступний, hook мовчить — прокидаємось за таймером.
        assert!(!wake.wait(), "fallback, а не подія");
    }

    fn filetime_touch(path: &Path) {
        let now = std::time::SystemTime::now();
        let file = std::fs::OpenOptions::new().write(true).open(path).unwrap();
        file.set_modified(now + Duration::from_secs(1)).unwrap();
    }
}
