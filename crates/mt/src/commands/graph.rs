//! `mt scan` — сирий скан дерева задач. `mt status` — те саме + cost ledger.

use clap::Args;
use mt_core::ledger::build_cost_ledger;
use mt_core::{discover_worktrees, scan_tasks, TaskNode};

use crate::context::resolve_tasks_dir;
use crate::output::emit;

fn flatten<'a>(nodes: &'a [TaskNode], out: &mut Vec<&'a TaskNode>) {
    for node in nodes {
        out.push(node);
        flatten(&node.children, out);
    }
}

fn find_node<'a>(nodes: &'a [TaskNode], path: &str) -> Option<&'a TaskNode> {
    for node in nodes {
        if node.path == path {
            return Some(node);
        }
        if let Some(found) = find_node(&node.children, path) {
            return Some(found);
        }
    }
    None
}

fn scan_tree(tasks_dir: &str) -> Result<Vec<TaskNode>, String> {
    let worktrees = discover_worktrees(std::path::Path::new(tasks_dir));
    scan_tasks(tasks_dir.to_string(), worktrees)
}

#[derive(Args)]
pub struct ScanArgs {}

pub fn run_scan(_args: ScanArgs, json: bool) -> Result<(), String> {
    let tasks_dir = resolve_tasks_dir(false)?;
    let tree = scan_tree(&tasks_dir)?;
    emit(json, &tree, |tree| {
        let mut all = Vec::new();
        flatten(tree, &mut all);
        for node in all {
            println!("{:?}\t{}", node.state, node.path);
        }
    });
    Ok(())
}

#[derive(Args)]
pub struct StatusArgs {
    /// Показати деталі лише одного вузла.
    pub name: Option<String>,
}

pub fn run_status(args: StatusArgs, json: bool) -> Result<(), String> {
    let tasks_dir = resolve_tasks_dir(false)?;
    let tree = scan_tree(&tasks_dir)?;
    let ledger = build_cost_ledger(&tasks_dir)?;

    if let Some(name) = args.name {
        mt_core::validate_name(&name)?;
        let node = find_node(&tree, &name).ok_or_else(|| format!("вузол не знайдено: {name}"))?;
        let entry = ledger.nodes.iter().find(|e| e.path == name);
        emit(
            json,
            &serde_json::json!({ "node": node, "ledger": entry }),
            |v| {
                let node = &v["node"];
                println!("{}: {:?}", name, node["state"]);
                if let Some(e) = entry {
                    println!(
                        "  runs={} wall_sec={} cost_usd={:.4}",
                        e.runs, e.wall_sec, e.cost_usd
                    );
                }
            },
        );
        return Ok(());
    }

    let mut all = Vec::new();
    flatten(&tree, &mut all);
    let mut counts: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for node in &all {
        *counts.entry(format!("{:?}", node.state)).or_default() += 1;
    }
    emit(
        json,
        &serde_json::json!({ "counts": counts, "ledger": ledger }),
        |_| {
            for (state, count) in &counts {
                println!("{state}: {count}");
            }
            println!(
                "TOTAL runs={} wall_sec={} cost_usd={:.4}",
                ledger.total.runs, ledger.total.wall_sec, ledger.total.cost_usd
            );
        },
    );
    Ok(())
}
