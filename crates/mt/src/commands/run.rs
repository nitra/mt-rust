//! `mt run` — один вузол через повний run-wrapper (claim → worktree →
//! виконавець → publish). `mt auto` — batch-автопілот поверх `run_auto`.

use clap::Args;
use mt_core::orchestrate::run_auto;
use mt_core::runner::run_node;

use crate::context::{resolve_node_path, resolve_tasks_dir};
use crate::output::emit;

#[derive(Args)]
pub struct RunArgs {
    pub name: Option<String>,
}

pub fn run(args: RunArgs, json: bool) -> Result<(), String> {
    let tasks_dir = resolve_tasks_dir(false)?;
    let node_path = resolve_node_path(args.name, &tasks_dir)?;
    let outcome = run_node(&tasks_dir, &node_path)?;
    emit(json, &outcome, |o| {
        println!("{}: {} ({})", node_path, o.result, o.run_file);
    });
    Ok(())
}

#[derive(Args)]
pub struct AutoArgs {
    /// Скільки вузлів виконувати одночасно за один батч.
    #[arg(long, default_value_t = 5)]
    pub concurrency: usize,
}

pub fn auto(args: AutoArgs, json: bool) -> Result<(), String> {
    let tasks_dir = resolve_tasks_dir(false)?;
    let results = run_auto(&tasks_dir, args.concurrency)?;
    emit(json, &results, |rs| {
        if rs.is_empty() {
            println!("немає waiting agent-вузлів для запуску");
        }
        for r in rs {
            println!("{}: {}", r.path, r.result);
        }
    });
    Ok(())
}
