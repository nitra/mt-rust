//! `mt check` (колишній `mt watch`) — one-shot attention/CI-гейт: pending-audit
//! без результату, stale worktrees, вузли на plan-review, провалені вузли.
//! exit 0 — чисто; exit 1 — є що переглянути.

use clap::Args;
use mt_core::worktree::worktree_inventory;
use mt_core::{discover_worktrees, scan_tasks, TaskNode, TaskState};

use crate::context::{project_config, repo_root, resolve_tasks_dir};
use crate::output::json;

fn flatten<'a>(nodes: &'a [TaskNode], out: &mut Vec<&'a TaskNode>) {
    for node in nodes {
        out.push(node);
        flatten(&node.children, out);
    }
}

#[derive(Args)]
pub struct CheckArgs {}

pub fn run(_args: CheckArgs, as_json: bool) -> Result<(), String> {
    let tasks_dir = resolve_tasks_dir(false)?;
    let worktrees = discover_worktrees(std::path::Path::new(&tasks_dir));
    let tree = scan_tasks(tasks_dir.clone(), worktrees)?;
    let mut all = Vec::new();
    flatten(&tree, &mut all);

    let pending_audit: Vec<&str> = all
        .iter()
        .filter(|n| n.state == TaskState::PendingAudit)
        .map(|n| n.path.as_str())
        .collect();
    let plan_review: Vec<&str> = all
        .iter()
        .filter(|n| n.state == TaskState::PlanReview)
        .map(|n| n.path.as_str())
        .collect();
    let failed: Vec<&str> = all
        .iter()
        .filter(|n| n.state == TaskState::Failed)
        .map(|n| n.path.as_str())
        .collect();

    let config = project_config(&tasks_dir);
    let stale_min = config["stale_worktree_min"].as_u64().unwrap_or(30);
    let task_paths: Vec<String> = all.iter().map(|n| n.path.clone()).collect();
    let stale_worktrees: Vec<String> = match repo_root(&tasks_dir) {
        Ok(root) => worktree_inventory(&root, &task_paths, stale_min)
            .unwrap_or_default()
            .into_iter()
            .filter(|w| w.stale)
            .map(|w| w.entry.name)
            .collect(),
        Err(_) => Vec::new(),
    };

    let needs_attention =
        !pending_audit.is_empty() || !plan_review.is_empty() || !stale_worktrees.is_empty();

    if as_json {
        json(&serde_json::json!({
            "pending_audit": pending_audit,
            "plan_review": plan_review,
            "failed": failed,
            "stale_worktrees": stale_worktrees,
            "needs_attention": needs_attention,
        }));
    } else if !needs_attention && failed.is_empty() {
        println!("чисто — уваги не потребує");
    } else {
        for p in &pending_audit {
            println!("pending-audit: {p}");
        }
        for p in &plan_review {
            println!("plan-review: {p}");
        }
        for p in &failed {
            println!("failed: {p}");
        }
        for w in &stale_worktrees {
            println!("stale worktree: {w}");
        }
    }
    std::process::exit(if needs_attention { 1 } else { 0 });
}
