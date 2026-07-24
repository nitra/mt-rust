//! `mt fact` / `mt verify` / `mt done` / `mt audit` / `mt failed` — сигнали
//! виконавця (файловий рівень, спека mt.md «Два етапи виконання вузла»).

use clap::Args;
use mt_core::signal;

use crate::context::{resolve_node_path, resolve_tasks_dir};
use crate::output::emit;

#[derive(Args)]
pub struct FactArgs {
    pub name: Option<String>,
    /// Обов'язковий короткий підсумок (`## Summary`).
    #[arg(long)]
    pub summary: String,
    /// Додаткові секції (сирий markdown) після `## Summary`.
    #[arg(long)]
    pub body: Option<String>,
}

pub fn run_fact(args: FactArgs, json: bool) -> Result<(), String> {
    let tasks_dir = resolve_tasks_dir(false)?;
    let node_path = resolve_node_path(args.name, &tasks_dir)?;
    let file = signal::write_fact(&tasks_dir, &node_path, &args.summary, args.body.as_deref())?;
    emit(json, &serde_json::json!({ "fact_file": file }), |_| {
        println!("fact: {file}");
    });
    Ok(())
}

#[derive(Args)]
pub struct VerifyArgs {
    pub name: Option<String>,
}

pub fn run_verify(args: VerifyArgs, json: bool) -> Result<(), String> {
    let tasks_dir = resolve_tasks_dir(false)?;
    let node_path = resolve_node_path(args.name, &tasks_dir)?;
    let results = signal::run_check(&tasks_dir, &node_path)?;
    emit(json, &results, |rs| {
        if rs.is_empty() {
            println!("## Check: немає команд — структурно OK");
        }
        for r in rs {
            println!("✓ `{}` (exit {})", r.command, r.exit_code);
        }
    });
    Ok(())
}

fn actor_default(explicit: Option<String>, default: &str) -> String {
    explicit.unwrap_or_else(|| default.to_string())
}

#[derive(Args)]
pub struct DoneArgs {
    pub name: Option<String>,
    #[arg(long)]
    pub actor: Option<String>,
}

pub fn run_done(args: DoneArgs, json: bool) -> Result<(), String> {
    let tasks_dir = resolve_tasks_dir(false)?;
    let node_path = resolve_node_path(args.name, &tasks_dir)?;
    let actor = actor_default(args.actor, "agent");
    let outcome = signal::done(&tasks_dir, &node_path, &actor)?;
    emit(json, &outcome, |o| {
        println!("done: {} (fact {})", o.run_file, o.fact_file);
        if !o.propagated.is_empty() {
            println!("  агрегація вгору: {:?}", o.propagated);
        }
    });
    Ok(())
}

#[derive(Args)]
pub struct AuditArgs {
    pub name: Option<String>,
    #[arg(long)]
    pub actor: Option<String>,
}

pub fn run_audit(args: AuditArgs, json: bool) -> Result<(), String> {
    let tasks_dir = resolve_tasks_dir(false)?;
    let node_path = resolve_node_path(args.name, &tasks_dir)?;
    let actor = actor_default(args.actor, "auditor");
    let outcome = signal::audit(&tasks_dir, &node_path, &actor)?;
    emit(json, &outcome, |o| {
        println!(
            "audit opened: {} (fact {})",
            o.pending_audit_file.as_deref().unwrap_or("?"),
            o.fact_file
        );
    });
    Ok(())
}

#[derive(Args)]
pub struct FailedArgs {
    pub name: Option<String>,
    #[arg(long)]
    pub actor: Option<String>,
    /// `## Completed` — що вдалося зробити до провалу.
    #[arg(long)]
    pub completed: String,
    /// `## Blockers` — що заблокувало виконання.
    #[arg(long)]
    pub blockers: String,
    /// `## Next Attempt` — що спробувати іншим разом.
    #[arg(long = "next-attempt")]
    pub next_attempt: String,
}

pub fn run_failed(args: FailedArgs, json: bool) -> Result<(), String> {
    let tasks_dir = resolve_tasks_dir(false)?;
    let node_path = resolve_node_path(args.name, &tasks_dir)?;
    let actor = actor_default(args.actor, "agent");
    let run_file = signal::failed(
        &tasks_dir,
        &node_path,
        &actor,
        &args.completed,
        &args.blockers,
        &args.next_attempt,
    )?;
    emit(json, &serde_json::json!({ "run_file": run_file }), |_| {
        println!("failed: {run_file}");
    });
    Ok(())
}
