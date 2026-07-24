//! `mt kill` / `mt invalidate` — lifecycle-мутації вузла.

use clap::Args;
use mt_core::lifecycle::{invalidate, kill};

use crate::context::{resolve_node_path, resolve_tasks_dir};
use crate::output::emit;

#[derive(Args)]
pub struct KillArgs {
    pub name: Option<String>,
}

pub fn run_kill(args: KillArgs, json: bool) -> Result<(), String> {
    let tasks_dir = resolve_tasks_dir(false)?;
    let node_path = resolve_node_path(args.name, &tasks_dir)?;
    let archive = kill(&tasks_dir, &node_path)?;
    emit(json, &serde_json::json!({ "archive": archive }), |_| {
        println!("{archive}");
    });
    Ok(())
}

#[derive(Args)]
pub struct InvalidateArgs {
    pub name: Option<String>,
    /// Лише поточний вузол — без каскаду на нащадків.
    #[arg(long = "no-cascade")]
    pub no_cascade: bool,
}

pub fn run_invalidate(args: InvalidateArgs, json: bool) -> Result<(), String> {
    let tasks_dir = resolve_tasks_dir(false)?;
    let node_path = resolve_node_path(args.name, &tasks_dir)?;
    let archived = invalidate(&tasks_dir, &node_path, !args.no_cascade)?;
    emit(json, &archived, |a| {
        println!("invalidated: {a:?}");
    });
    Ok(())
}
