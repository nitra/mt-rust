//! Cost/time ledger (спека mt.md, run-frontmatter `wall_sec`/`tokens_in`/
//! `tokens_out`/`cost_usd` — «сировина для звітності»). Агрегує всі
//! `run_NNN.md` по графу воркспейсу: per-node і сумарний підсумок для
//! GUI-аналітики (Фаза 4).

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::artifacts::{list_node_artifacts, ArtifactKind};
use crate::{discover_worktrees, scan_tasks, TaskNode};

/// Агреговані метрики одного вузла (сума по всіх його `run_NNN.md`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LedgerEntry {
    pub path: String,
    pub runs: u64,
    pub wall_sec: u64,
    pub cost_usd: f64,
    pub tokens_in: u64,
    pub tokens_out: u64,
}

/// Підсумок воркспейсу: per-node записи (найдорожчі за `wall_sec` — першими)
/// + сумарний рядок `total` по всьому графу.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CostLedger {
    pub nodes: Vec<LedgerEntry>,
    pub total: LedgerEntry,
}

fn flatten<'a>(nodes: &'a [TaskNode], out: &mut Vec<&'a TaskNode>) {
    for node in nodes {
        out.push(node);
        flatten(&node.children, out);
    }
}

fn add(acc: &mut LedgerEntry, entry: &LedgerEntry) {
    acc.runs += entry.runs;
    acc.wall_sec += entry.wall_sec;
    acc.cost_usd += entry.cost_usd;
    acc.tokens_in += entry.tokens_in;
    acc.tokens_out += entry.tokens_out;
}

/// Будує ledger сканом усього дерева воркспейсу + `run_NNN.md` кожного вузла.
/// Вузли без жодного run-артефакту (ще не запускались) — пропускаються.
pub fn build_cost_ledger(tasks_dir: &str) -> Result<CostLedger, String> {
    let worktrees = discover_worktrees(Path::new(tasks_dir));
    let tree = scan_tasks(tasks_dir.to_string(), worktrees)?;
    let mut all = Vec::new();
    flatten(&tree, &mut all);

    let mut nodes = Vec::new();
    let mut total = LedgerEntry::default();
    for node in all {
        let artifacts = list_node_artifacts(tasks_dir, &node.path).unwrap_or_default();
        let mut entry = LedgerEntry {
            path: node.path.clone(),
            ..Default::default()
        };
        for artifact in &artifacts {
            if artifact.kind != ArtifactKind::Run {
                continue;
            }
            entry.runs += 1;
            entry.wall_sec += artifact.wall_sec.unwrap_or(0);
            entry.cost_usd += artifact.cost_usd.unwrap_or(0.0);
            entry.tokens_in += artifact.tokens_in.unwrap_or(0);
            entry.tokens_out += artifact.tokens_out.unwrap_or(0);
        }
        if entry.runs > 0 {
            add(&mut total, &entry);
            nodes.push(entry);
        }
    }
    nodes.sort_by(|a, b| {
        b.wall_sec
            .cmp(&a.wall_sec)
            .then(b.cost_usd.total_cmp(&a.cost_usd))
    });
    total.path = "TOTAL".to_string();

    Ok(CostLedger { nodes, total })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write_run(dir: &std::path::Path, nnn: &str, wall_sec: u64, cost_usd: Option<f64>) {
        let cost_line = cost_usd
            .map(|c| format!("cost_usd: {c}\n"))
            .unwrap_or_default();
        fs::write(
            dir.join(format!("run_{nnn}.md")),
            format!(
                "---\nschema_version: 1\nactor: agent\nresult: success\nwall_sec: {wall_sec}\n{cost_line}tokens_in: 100\ntokens_out: 20\n---\n"
            ),
        )
        .unwrap();
    }

    fn node(tmp: &std::path::Path, path: &str) -> std::path::PathBuf {
        let dir = tmp.join(path);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("task.md"),
            "---\nschema_version: 1\ncreated_at: 2026-06-06T10:00:00Z\n---\n\n## Task\n",
        )
        .unwrap();
        fs::write(dir.join("a.md"), "schema_version: 1\n").unwrap();
        dir
    }

    #[test]
    fn aggregates_runs_per_node_and_total() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("mt");
        let a = node(&root, "a");
        write_run(&a, "001", 100, Some(0.5));
        write_run(&a, "002", 50, Some(0.1));
        let b = node(&root, "b");
        write_run(&b, "001", 300, None);

        let ledger = build_cost_ledger(&root.to_string_lossy()).unwrap();
        assert_eq!(ledger.nodes.len(), 2);
        // b дорожчий за wall_sec (300 > 150) — сортування спадне.
        assert_eq!(ledger.nodes[0].path, "b");
        assert_eq!(ledger.nodes[0].wall_sec, 300);
        assert_eq!(ledger.nodes[0].runs, 1);
        assert_eq!(ledger.nodes[1].path, "a");
        assert_eq!(ledger.nodes[1].runs, 2);
        assert_eq!(ledger.nodes[1].wall_sec, 150);
        assert!((ledger.nodes[1].cost_usd - 0.6).abs() < 1e-9);

        assert_eq!(ledger.total.runs, 3);
        assert_eq!(ledger.total.wall_sec, 450);
        assert!((ledger.total.cost_usd - 0.6).abs() < 1e-9);
        assert_eq!(ledger.total.tokens_in, 300);
        assert_eq!(ledger.total.tokens_out, 60);
    }

    #[test]
    fn node_without_runs_is_excluded() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("mt");
        node(&root, "untouched");
        let ledger = build_cost_ledger(&root.to_string_lossy()).unwrap();
        assert!(ledger.nodes.is_empty());
        assert_eq!(ledger.total.runs, 0);
    }

    #[test]
    fn composite_children_included_recursively() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("mt");
        let parent = node(&root, "parent");
        write_run(&parent, "001", 10, None);
        let child = node(&root, "parent/child");
        write_run(&child, "001", 20, None);

        let ledger = build_cost_ledger(&root.to_string_lossy()).unwrap();
        let paths: Vec<&str> = ledger.nodes.iter().map(|e| e.path.as_str()).collect();
        assert!(paths.contains(&"parent"));
        assert!(paths.contains(&"parent/child"));
        assert_eq!(ledger.total.wall_sec, 30);
    }
}
