//! Закриття аудит-циклу: `mt verdict` / `mt clarify` / `mt amend`
//! (graph.md, «Аудит (async черга)»). Відкриває цикл — `mt audit`.

use clap::Args;
use mt_core::audit;

use crate::context::{resolve_node_path, resolve_tasks_dir};
use crate::output::emit;

#[derive(Args)]
pub struct VerdictArgs {
    pub name: Option<String>,
    /// Хто виніс вердикт; дефолт — `auditor`.
    #[arg(long)]
    pub actor: Option<String>,
    /// Прийняти результат (без прапорця — відхилити на rework).
    #[arg(long)]
    pub success: bool,
    /// `## Reasoning` — обґрунтування вердикту, обов'язкове.
    #[arg(long)]
    pub reason: String,
}

pub fn run_verdict(args: VerdictArgs, json: bool) -> Result<(), String> {
    let tasks_dir = resolve_tasks_dir(false)?;
    let node_path = resolve_node_path(args.name, &tasks_dir)?;
    let actor = args.actor.unwrap_or_else(|| "auditor".to_string());
    let file = audit::verdict(
        &tasks_dir,
        &node_path,
        &actor,
        args.success,
        &args.reason,
        false,
    )?;
    emit(
        json,
        &serde_json::json!({ "audit_result_file": file }),
        |_| {
            let verdict = if args.success { "success" } else { "failed" };
            println!("audit verdict ({verdict}): {file}");
        },
    );
    Ok(())
}

#[derive(Args)]
pub struct ClarifyArgs {
    pub name: Option<String>,
    #[arg(long)]
    pub actor: Option<String>,
    /// Питання до виконавця; цикл лишається відкритим.
    #[arg(long)]
    pub question: String,
}

pub fn run_clarify(args: ClarifyArgs, json: bool) -> Result<(), String> {
    let tasks_dir = resolve_tasks_dir(false)?;
    let node_path = resolve_node_path(args.name, &tasks_dir)?;
    let actor = args.actor.unwrap_or_else(|| "auditor".to_string());
    let file = audit::clarification(&tasks_dir, &node_path, &actor, &args.question)?;
    emit(
        json,
        &serde_json::json!({ "clarification_file": file }),
        |_| {
            println!("clarification: {file}");
        },
    );
    Ok(())
}

#[derive(Args)]
pub struct AmendArgs {
    pub name: Option<String>,
    #[arg(long)]
    pub actor: Option<String>,
    /// Відповідь на уточнення аудитора.
    #[arg(long)]
    pub answer: String,
}

pub fn run_amend(args: AmendArgs, json: bool) -> Result<(), String> {
    let tasks_dir = resolve_tasks_dir(false)?;
    let node_path = resolve_node_path(args.name, &tasks_dir)?;
    let actor = args.actor.unwrap_or_else(|| "agent".to_string());
    let file = audit::amend(&tasks_dir, &node_path, &actor, &args.answer)?;
    emit(json, &serde_json::json!({ "amended_file": file }), |_| {
        println!("amended: {file}");
    });
    Ok(())
}
