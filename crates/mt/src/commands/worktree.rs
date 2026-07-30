//! `mt worktree` — керування developer git-worktree: create|remove|list|prune|inventory.

use std::path::Path;

use clap::{Args, Subcommand};
use mt_core::worktree::{
    create_dev_worktree, list_worktrees, prune_worktrees, remove_dev_worktree,
    set_branch_description, worktree_inventory, WorktreeEntry,
};
use mt_core::{discover_worktrees, scan_tasks, TaskNode};

use crate::context::{git_root, project_config_at, resolve_tasks_dir};
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
        #[arg(long)]
        description: Option<String>,
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
    // `create|remove|list|prune` потребують лише Git root — MT task graph
    // (`.mt.json`/`mt/`) не є передумовою (лише `inventory` збагачує його
    // task-асоціацією, і то опційно, якщо граф присутній).
    let root = git_root()?;
    let config = project_config_at(&root);

    match args.action {
        WorktreeAction::Create {
            name,
            base,
            description,
        } => {
            let worktrees_dir = root.join(
                config["worktrees_dir"]
                    .as_str()
                    .unwrap_or("./.worktrees")
                    .trim_start_matches("./"),
            );
            let created = create_dev_worktree(&root, &worktrees_dir, &name, &base)?;
            if let Some(description) = description {
                if let Err(err) = set_branch_description(&root, &created.branch, &description) {
                    // Транзакційний rollback: щойно створений clean worktree/branch
                    // не повинен лишатись напівготовим після невдалого metadata-кроку.
                    let entry = WorktreeEntry {
                        path: created.path.to_string_lossy().into_owned(),
                        name: created.name.clone(),
                        head: String::new(),
                        branch: Some(format!("refs/heads/{}", created.branch)),
                        locked: false,
                        prunable: false,
                    };
                    let _ = remove_dev_worktree(&root, &entry, &name, true);
                    return Err(err);
                }
            }
            emit(
                json,
                &serde_json::json!({
                    "name": created.name,
                    "branch": created.branch,
                    "path": created.path,
                }),
                |_| {
                    println!("worktree: {} ({})", created.path.display(), created.branch);
                },
            );
        }
        WorktreeAction::Remove { name, force } => {
            let entries = list_worktrees(&root)?;
            let entry = entries
                .iter()
                .find(|e| e.name == name)
                .ok_or_else(|| format!("worktree не знайдено: {name}"))?;
            remove_dev_worktree(&root, entry, &name, force)?;
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
            // Task-асоціація — опційне збагачення: без графа задач інвентар
            // усе одно повертає worktree-список (просто без `task_path`).
            let task_paths: Vec<String> = match resolve_tasks_dir(false) {
                Ok(tasks_dir) => {
                    let worktrees = discover_worktrees(Path::new(&tasks_dir));
                    let tree = scan_tasks(tasks_dir.clone(), worktrees)?;
                    let mut all = Vec::new();
                    flatten(&tree, &mut all);
                    all.iter().map(|n| n.path.clone()).collect()
                }
                Err(_) => Vec::new(),
            };
            let stale_min = config["stale_worktree_min"].as_u64().unwrap_or(30);
            let inventory = worktree_inventory(&root, &task_paths, stale_min)?;
            emit(json, &inventory, |items| {
                for i in items {
                    let description = i
                        .description
                        .as_deref()
                        .map(|value| format!("\tdescription={value}"))
                        .unwrap_or_default();
                    println!(
                        "{}\t{}\tage={}m\tstale={}\ttask={}{}",
                        i.entry.name,
                        i.entry.branch.as_deref().unwrap_or("(detached)"),
                        i.age_min,
                        i.stale,
                        i.task_path.as_deref().unwrap_or("-"),
                        description,
                    );
                }
            });
        }
    }
    Ok(())
}
