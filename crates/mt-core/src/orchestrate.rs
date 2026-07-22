//! Оркестратор `run --auto` — локальний одноразовий прохід (спека mt.md,
//! «Оркестрація»): знаходить усі `waiting` агентські вузли, сортує (leaf-и що
//! розблоковують найбільше нащадків — першими, потім nearest deadline, потім
//! `created_at`) і прогонить чергами по `agent_concurrency` через
//! [`crate::runner::run_node`].
//!
//! **Спрощення проти повної спеки:** батчинг замість continuous backfill —
//! один прохід бере до `concurrency` вузлів, чекає завершення всієї партії,
//! пересканує і формує наступну. Достатньо для solo-машини (Фаза 3, крок 2);
//! remote claims і `mt watch` periodic rescan — окрема фаза.

use std::collections::HashSet;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::runner::run_node;
use crate::{discover_worktrees, scan_tasks, TaskNode, TaskState};

/// Підсумок однієї спроби в межах `run_auto`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoResult {
    pub path: String,
    /// success | failed | budget-exceeded | progress-timeout | error
    pub result: String,
    /// Заповнено лише для `result: "error"` (preflight відмовив запуск).
    pub error: Option<String>,
}

fn walk<'a>(nodes: &'a [TaskNode], out: &mut Vec<&'a TaskNode>) {
    for node in nodes {
        out.push(node);
        walk(&node.children, out);
    }
}

/// Плаский список усіх вузлів воркспейсу (для підрахунку залежностей і
/// вибірки waiting-агентських).
fn flatten(nodes: &[TaskNode]) -> Vec<&TaskNode> {
    let mut out = Vec::new();
    walk(nodes, &mut out);
    out
}

/// Сортує `waiting`-вузли: більше "розблокованих" нащадків (dependents
/// count по всьому графу) — раніше; далі nearest `deadline` (без нього —
/// в кінець групи); далі `created_at` (без нього — в кінець).
pub fn sort_for_auto<'a>(all: &[&'a TaskNode], waiting: Vec<&'a TaskNode>) -> Vec<&'a TaskNode> {
    let dependents_count = |path: &str| -> usize {
        all.iter()
            .filter(|n| n.deps.iter().any(|d| d == path))
            .count()
    };
    let mut sorted = waiting;
    sorted.sort_by(|a, b| {
        let by_dependents = dependents_count(&b.path).cmp(&dependents_count(&a.path));
        if by_dependents != std::cmp::Ordering::Equal {
            return by_dependents;
        }
        let by_deadline = match (&a.deadline, &b.deadline) {
            (Some(x), Some(y)) => x.cmp(y),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        };
        if by_deadline != std::cmp::Ordering::Equal {
            return by_deadline;
        }
        match (&a.created_at, &b.created_at) {
            (Some(x), Some(y)) => x.cmp(y),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => a.path.cmp(&b.path),
        }
    });
    sorted
}

/// Один прохід оркестратора: до вичерпання waiting-агентських вузлів прогонить
/// їх чергами по `concurrency` (кожен вузол — окремий потік через
/// [`run_node`]). Вузол, що впав на preflight (гонка/зникла умова), більше не
/// підбирається в межах цього виклику — гарантує термінацію.
pub fn run_auto(tasks_dir: &str, concurrency: usize) -> Result<Vec<AutoResult>, String> {
    let mut results = Vec::new();
    let mut skip: HashSet<String> = HashSet::new();
    let concurrency = concurrency.max(1);

    loop {
        let worktrees = discover_worktrees(Path::new(tasks_dir));
        let tree = scan_tasks(tasks_dir.to_string(), worktrees)?;
        let all = flatten(&tree);
        let waiting: Vec<&TaskNode> = all
            .iter()
            .copied()
            .filter(|n| {
                n.mode == "agent" && n.state == TaskState::Waiting && !skip.contains(&n.path)
            })
            .collect();
        if waiting.is_empty() {
            break;
        }

        let batch: Vec<String> = sort_for_auto(&all, waiting)
            .into_iter()
            .take(concurrency)
            .map(|n| n.path.clone())
            .collect();
        if batch.is_empty() {
            break;
        }

        let handles: Vec<_> = batch
            .into_iter()
            .map(|path| {
                let dir = tasks_dir.to_string();
                std::thread::spawn(move || (path.clone(), run_node(&dir, &path)))
            })
            .collect();

        for handle in handles {
            let (path, outcome) = handle
                .join()
                .unwrap_or_else(|_| (String::new(), Err("run thread panicked".to_string())));
            match outcome {
                Ok(o) => results.push(AutoResult {
                    path,
                    result: o.result,
                    error: None,
                }),
                Err(e) => {
                    skip.insert(path.clone());
                    results.push(AutoResult {
                        path,
                        result: "error".to_string(),
                        error: Some(e),
                    });
                }
            }
        }
    }
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn node(path: &str, mode: &str, state: TaskState, deps: &[&str]) -> TaskNode {
        TaskNode {
            id: path.rsplit('/').next().unwrap_or(path).to_string(),
            path: path.to_string(),
            state,
            deps: deps.iter().map(|s| s.to_string()).collect(),
            mode: mode.to_string(),
            budget_sec: None,
            budget_hard_sec: None,
            deadline: None,
            hint: None,
            created_at: None,
            children: Vec::new(),
            is_composite: false,
        }
    }

    #[test]
    fn sorts_by_dependents_then_deadline_then_created_at() {
        let mut c = node("c", "agent", TaskState::Waiting, &[]);
        c.deadline = Some("2026-07-10T00:00:00Z".to_string());
        let mut a = node("a", "agent", TaskState::Waiting, &[]); // 2 dependents
        a.created_at = Some("2026-07-01T00:00:00Z".to_string());
        let b = node("b", "agent", TaskState::Waiting, &[]); // 1 dependent, no deadline/created_at
        let dependent1 = node("d1", "agent", TaskState::Blocked, &["a"]);
        let dependent2 = node("d2", "agent", TaskState::Blocked, &["a"]);
        let dependent3 = node("d3", "agent", TaskState::Blocked, &["b"]);

        let all_owned = [
            a.clone(),
            b.clone(),
            c.clone(),
            dependent1,
            dependent2,
            dependent3,
        ];
        let all_refs: Vec<&TaskNode> = all_owned.iter().collect();
        let waiting = vec![&all_owned[2], &all_owned[1], &all_owned[0]]; // c, b, a (shuffled)

        let sorted = sort_for_auto(&all_refs, waiting);
        let paths: Vec<&str> = sorted.iter().map(|n| n.path.as_str()).collect();
        // a: 2 dependents → перший; b: 1 dependent → другий; c: 0 dependents → останній.
        assert_eq!(paths, ["a", "b", "c"]);
    }

    #[test]
    fn deadline_breaks_tie_before_created_at() {
        let mut x = node("x", "agent", TaskState::Waiting, &[]);
        x.created_at = Some("2026-01-01T00:00:00Z".to_string());
        let mut y = node("y", "agent", TaskState::Waiting, &[]);
        y.deadline = Some("2026-01-01T00:00:00Z".to_string());
        let all = [x, y];
        let refs: Vec<&TaskNode> = all.iter().collect();
        let sorted = sort_for_auto(&refs, refs.clone());
        // y має deadline (навіть пізніший created_at за замовчуванням None) → раніше x.
        assert_eq!(sorted[0].path, "y");
    }

    #[test]
    fn run_auto_terminates_on_repeated_preflight_error() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("mt");
        // Scan бачить Waiting (deps не враховують budget), але runner::preflight
        // відмовляє на budget_hard_sec: 0 (validation error) — розбіжність
        // між derived-станом і preflight, яку має покривати skip-set.
        let dir = root.join("stuck");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("task.md"),
            "---\nschema_version: 1\ncreated_at: 2026-06-06T10:00:00Z\nbudget_hard_sec: 0\n---\n\n## Task\n",
        )
        .unwrap();
        fs::write(dir.join("a.md"), "schema_version: 1\n").unwrap();

        let root_s = root.to_string_lossy().into_owned();
        let results = run_auto(&root_s, 5).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].path, "stuck");
        assert_eq!(results[0].result, "error");
        assert!(results[0]
            .error
            .as_deref()
            .unwrap()
            .contains("budget_hard_sec"));
    }

    #[test]
    fn run_auto_empty_workspace_returns_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("mt");
        fs::create_dir_all(&root).unwrap();
        let results = run_auto(&root.to_string_lossy(), 5).unwrap();
        assert!(results.is_empty());
    }
}
