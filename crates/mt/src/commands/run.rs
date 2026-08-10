//! `mt run` — один вузол через повний run-wrapper (claim → worktree →
//! виконавець → publish). `mt auto` — batch-автопілот поверх `run_auto`.

use clap::Args;
use mt_core::orchestrate::run_auto;

use crate::context::{resolve_node_path, resolve_tasks_dir};
use crate::output::emit;

#[derive(Args)]
pub struct RunArgs {
    pub name: Option<String>,
    /// `engineer` — ремонтник графа після вичерпання драбини агента.
    #[arg(long)]
    pub actor: Option<String>,
}

pub fn run(args: RunArgs, json: bool) -> Result<(), String> {
    let tasks_dir = resolve_tasks_dir(false)?;
    let node_path = resolve_node_path(args.name, &tasks_dir)?;
    // Аудитор — окремий шлях: без claim і worktree (він нічого не виконує).
    if args.actor.as_deref() == Some("auditor") {
        let run = mt_core::audit::run_auditor(
            &tasks_dir,
            &node_path,
            &mt_core::config::agent_cli_env_from_process(),
        )?;
        emit(json, &run, |r| {
            println!(
                "audit {}: {}",
                node_path,
                r.artifact.as_deref().unwrap_or("аудитор нічого не написав")
            );
        });
        return Ok(());
    }
    let actor = match args.actor.as_deref() {
        None | Some("agent") => mt_core::runner::Actor::Agent,
        Some("engineer") => mt_core::runner::Actor::Engineer,
        Some(other) => {
            return Err(format!(
                "невідомий актор `{other}` — agent | engineer | auditor"
            ))
        }
    };
    let outcome = mt_core::runner::run_node_as(&tasks_dir, &node_path, actor)?;
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
