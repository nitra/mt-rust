//! `mt plan` — записує нову чернетку плану.
//! `mt spawn` — читає/схвалює/відхиляє plan-review (`## Children`).

use clap::Args;
use mt_core::write_plan_draft;

use crate::context::{resolve_node_path, resolve_tasks_dir};
use crate::output::emit;

#[derive(Args)]
pub struct PlanArgs {
    /// Задача (за замовчуванням — з поточної директорії).
    pub name: Option<String>,
    #[arg(long, value_parser = ["agent", "human"])]
    pub mode: Option<String>,
}

pub fn run_plan(args: PlanArgs, json: bool) -> Result<(), String> {
    let tasks_dir = resolve_tasks_dir(false)?;
    let node_path = resolve_node_path(args.name, &tasks_dir)?;
    let file = write_plan_draft(&tasks_dir, &node_path, args.mode.as_deref())?;
    emit(json, &serde_json::json!({ "plan_file": file }), |_| {
        println!("plan: {file}");
    });
    Ok(())
}

#[derive(Args)]
pub struct SpawnArgs {
    /// Задача (за замовчуванням — з поточної директорії).
    pub name: Option<String>,
    /// Схвалити актуальний план і матеріалізувати `## Children`.
    #[arg(long, conflicts_with = "reject")]
    pub approve: bool,
    /// Відхилити актуальний план із причиною.
    #[arg(long, conflicts_with = "approve")]
    pub reject: Option<String>,
}

pub fn run_spawn(args: SpawnArgs, json: bool) -> Result<(), String> {
    let tasks_dir = resolve_tasks_dir(false)?;
    let node_path = resolve_node_path(args.name, &tasks_dir)?;

    if args.approve {
        let outcome = mt_core::spawn::spawn_approve(&tasks_dir, &node_path)?;
        emit(json, &outcome, |o| {
            println!("approved: {} → {:?}", o.approved_file, o.children);
        });
        return Ok(());
    }
    if let Some(reason) = args.reject {
        let file = mt_core::spawn::spawn_reject(&tasks_dir, &node_path, &reason)?;
        emit(json, &serde_json::json!({ "rejected_file": file }), |_| {
            println!("rejected: {file}");
        });
        return Ok(());
    }

    // Без флагів — read-only перегляд plan-review.
    let review = mt_core::spawn::plan_review(&tasks_dir, &node_path)?;
    emit(json, &review, |r| {
        println!(
            "{} — decision: {:?}, decided: {}, children: {}",
            r.plan_file,
            r.decision,
            r.decided,
            r.children.len()
        );
        for child in &r.children {
            println!("  - {} ({:?})", child.id, child.mode);
        }
    });
    Ok(())
}
