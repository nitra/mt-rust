//! `mt worktree` — керування developer git-worktree: create|remove|list|prune|inventory.

use std::path::Path;

use clap::{Args, Subcommand};
use mt_core::worktree::{
    create_dev_worktree, list_worktrees, prune_worktrees, remove_worktree, worktree_inventory,
};
use mt_core::{discover_worktrees, scan_tasks, TaskNode};

use crate::context::{project_config, repo_root, resolve_tasks_dir};
use crate::output::emit;

#[derive(Args)]
pub struct WorktreeArgs {
    #[command(subcommand)]
    pub action: WorktreeAction,
}

#[derive(Subcommand)]
pub enum WorktreeAction {
    /// Створити гілковий dev-worktree (`mt/<name>`) від базової гілки.
    Create {
        name: String,
        #[arg(long, default_value = "main")]
        base: String,
    },
    /// Прибрати worktree за іменем.
    Remove {
        name: String,
        #[arg(long)]
        force: bool,
    },
    /// Список усіх worktree репо.
    List {},
    /// `git worktree prune` — прибрати адміністративні записи зниклих директорій.
    Prune {},
    /// Список worktree + вік + матч на задачу + stale-прапор.
    Inventory {},
}

fn flatten<'a>(nodes: &'a [TaskNode], out: &mut Vec<&'a TaskNode>) {
    for node in nodes {
        out.push(node);
        flatten(&node.children, out);
    }
}

pub fn run(args: WorktreeArgs, json: bool) -> Result<(), String> {
    let tasks_dir = resolve_tasks_dir(false)?;
    let root = repo_root(&tasks_dir)?;
    let config = project_config(&tasks_dir);

    match args.action {
        WorktreeAction::Create { name, base } => {
            let worktrees_dir = root.join(
                config["worktrees_dir"]
                    .as_str()
                    .unwrap_or("./.worktrees")
                    .trim_start_matches("./"),
            );
            let path = create_dev_worktree(&root, &worktrees_dir, &name, &base)?;
            emit(json, &serde_json::json!({ "path": path }), |_| {
                println!("worktree: {}", path.display());
            });
        }
        WorktreeAction::Remove { name, force } => {
            let entries = list_worktrees(&root)?;
            let entry = entries
                .iter()
                .find(|e| e.name == name)
                .ok_or_else(|| format!("worktree не знайдено: {name}"))?;
            remove_worktree(&root, Path::new(&entry.path), force)?;
            emit(json, &serde_json::json!({ "removed": entry.path }), |_| {
                println!("removed: {}", entry.path);
            });
        }
        WorktreeAction::List {} => {
            let entries = list_worktrees(&root)?;
            emit(json, &entries, |es| {
                for e in es {
                    println!(
                        "{}\t{}\t{}",
                        e.name,
                        e.branch.as_deref().unwrap_or("(detached)"),
                        e.path
                    );
                }
            });
        }
        WorktreeAction::Prune {} => {
            let output = prune_worktrees(&root)?;
            emit(json, &serde_json::json!({ "output": output }), |_| {
                if output.is_empty() {
                    println!("нічого прибирати");
                } else {
                    println!("{output}");
                }
            });
        }
        WorktreeAction::Inventory {} => {
            let worktrees = discover_worktrees(Path::new(&tasks_dir));
            let tree = scan_tasks(tasks_dir.clone(), worktrees)?;
            let mut all = Vec::new();
            flatten(&tree, &mut all);
            let task_paths: Vec<String> = all.iter().map(|n| n.path.clone()).collect();
            let stale_min = config["stale_worktree_min"].as_u64().unwrap_or(30);
            let inventory = worktree_inventory(&root, &task_paths, stale_min)?;
            emit(json, &inventory, |items| {
                for i in items {
                    println!(
                        "{}\t{}\tage={}m\tstale={}\ttask={}",
                        i.entry.name,
                        i.entry.branch.as_deref().unwrap_or("(detached)"),
                        i.age_min,
                        i.stale,
                        i.task_path.as_deref().unwrap_or("-")
                    );
                }
            });
        }
    }
    Ok(())
}
