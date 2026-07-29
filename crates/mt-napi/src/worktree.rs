//! napi-еквіваленти `mt worktree create|remove|inventory` — той самий
//! `mt_core::worktree` API, що й `crates/mt/src/commands/worktree.rs`.

use std::path::Path;

use napi::bindgen_prelude::*;
use napi_derive::napi;

use crate::context::{project_config, repo_root, resolve_tasks_dir};

#[napi(object)]
pub struct WorktreeCreateResult {
    pub path: String,
}

/// napi-еквівалент `mt worktree create <name> [--base <base>] [--description <text>]`.
#[napi]
pub fn worktree_create(
    name: String,
    base: Option<String>,
    root: Option<String>,
    description: Option<String>,
) -> Result<WorktreeCreateResult> {
    let tasks_dir = resolve_tasks_dir(root.as_deref())?;
    let repo = repo_root(&tasks_dir)?;
    let config = project_config(&tasks_dir);
    let worktrees_dir = repo.join(
        config["worktrees_dir"]
            .as_str()
            .unwrap_or("./.worktrees")
            .trim_start_matches("./"),
    );
    let base = base.unwrap_or_else(|| "main".to_string());
    let path = mt_core::worktree::create_dev_worktree(&repo, &worktrees_dir, &name, &base)
        .map_err(Error::from_reason)?;
    if let Some(description) = description {
        mt_core::worktree::set_branch_description(&repo, &format!("mt/{name}"), &description)
            .map_err(Error::from_reason)?;
    }
    Ok(WorktreeCreateResult {
        path: path.to_string_lossy().into_owned(),
    })
}

#[napi(object)]
pub struct WorktreeRemoveResult {
    pub removed: String,
}

/// napi-еквівалент `mt worktree remove <name> [--force]`.
#[napi]
pub fn worktree_remove(
    name: String,
    force: Option<bool>,
    root: Option<String>,
) -> Result<WorktreeRemoveResult> {
    let tasks_dir = resolve_tasks_dir(root.as_deref())?;
    let repo = repo_root(&tasks_dir)?;
    let entries = mt_core::worktree::list_worktrees(&repo).map_err(Error::from_reason)?;
    let entry = entries
        .iter()
        .find(|e| e.name == name)
        .ok_or_else(|| Error::from_reason(format!("worktree не знайдено: {name}")))?;
    mt_core::worktree::remove_worktree(&repo, Path::new(&entry.path), force.unwrap_or(false))
        .map_err(Error::from_reason)?;
    Ok(WorktreeRemoveResult {
        removed: entry.path.clone(),
    })
}

#[napi(object)]
pub struct WorktreeStatusItem {
    pub path: String,
    pub name: String,
    pub head: String,
    pub branch: Option<String>,
    pub locked: bool,
    pub prunable: bool,
    pub age_min: i64,
    pub stale: bool,
    pub task_path: Option<String>,
    pub description: Option<String>,
}

impl From<mt_core::worktree::WorktreeInventoryItem> for WorktreeStatusItem {
    fn from(i: mt_core::worktree::WorktreeInventoryItem) -> Self {
        Self {
            path: i.entry.path,
            name: i.entry.name,
            head: i.entry.head,
            branch: i.entry.branch,
            locked: i.entry.locked,
            prunable: i.entry.prunable,
            age_min: i.age_min as i64,
            stale: i.stale,
            task_path: i.task_path,
            description: i.description,
        }
    }
}

fn flatten<'a>(nodes: &'a [mt_core::TaskNode], out: &mut Vec<&'a mt_core::TaskNode>) {
    for node in nodes {
        out.push(node);
        flatten(&node.children, out);
    }
}

/// napi-еквівалент `mt worktree inventory` (worktree + вік + stale + матч задачі).
#[napi]
pub fn worktree_status(root: Option<String>) -> Result<Vec<WorktreeStatusItem>> {
    let tasks_dir = resolve_tasks_dir(root.as_deref())?;
    let repo = repo_root(&tasks_dir)?;
    let config = project_config(&tasks_dir);
    let worktrees = mt_core::discover_worktrees(Path::new(&tasks_dir));
    let tree = mt_core::scan_tasks(tasks_dir.clone(), worktrees).map_err(Error::from_reason)?;
    let mut all = Vec::new();
    flatten(&tree, &mut all);
    let task_paths: Vec<String> = all.iter().map(|n| n.path.clone()).collect();
    let stale_min = config["stale_worktree_min"].as_u64().unwrap_or(30);
    let inventory = mt_core::worktree::worktree_inventory(&repo, &task_paths, stale_min)
        .map_err(Error::from_reason)?;
    Ok(inventory
        .into_iter()
        .map(WorktreeStatusItem::from)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_item_preserves_branch_description() {
        let item = WorktreeStatusItem::from(mt_core::worktree::WorktreeInventoryItem {
            entry: mt_core::worktree::WorktreeEntry {
                path: "/repo/.worktrees/demo".to_string(),
                name: "demo".to_string(),
                head: "deadbeef".to_string(),
                branch: Some("refs/heads/mt/demo".to_string()),
                locked: false,
                prunable: false,
            },
            age_min: 1,
            stale: false,
            task_path: None,
            description: Some("lint isolation".to_string()),
        });

        assert_eq!(item.description.as_deref(), Some("lint isolation"));
    }
}
