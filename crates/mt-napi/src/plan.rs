//! napi-еквіваленти `mt plan` / `mt spawn` — той самий `mt_core::write_plan_draft`
//! і `mt_core::spawn::*`, що й `crates/mt/src/commands/plan.rs`.

use napi::bindgen_prelude::*;
use napi_derive::napi;

use crate::context::{resolve_node_path, resolve_tasks_dir};

#[napi(object)]
pub struct PlanResult {
    pub plan_file: String,
}

/// napi-еквівалент `mt plan [name] [--mode agent|human]`.
#[napi]
pub fn plan(
    name: Option<String>,
    mode: Option<String>,
    root: Option<String>,
) -> Result<PlanResult> {
    let tasks_dir = resolve_tasks_dir(root.as_deref())?;
    let node_path = resolve_node_path(name, &tasks_dir)?;
    let file = mt_core::write_plan_draft(&tasks_dir, &node_path, mode.as_deref())
        .map_err(Error::from_reason)?;
    Ok(PlanResult { plan_file: file })
}

#[napi(object)]
pub struct ChildSpecJs {
    pub id: String,
    pub mode: Option<String>,
    pub model_tier: Option<String>,
    pub skills: Vec<String>,
    pub qualification: Option<String>,
    pub budget_sec: Option<i64>,
    pub export: bool,
    pub deps: Vec<String>,
    pub task: Option<String>,
}

impl From<mt_core::spawn::ChildSpec> for ChildSpecJs {
    fn from(c: mt_core::spawn::ChildSpec) -> Self {
        Self {
            id: c.id,
            mode: c.mode,
            model_tier: c.model_tier,
            skills: c.skills,
            qualification: c.qualification,
            budget_sec: c.budget_sec.map(|v| v as i64),
            export: c.export,
            deps: c.deps,
            task: c.task,
        }
    }
}

#[napi(object)]
pub struct PlanReviewJs {
    pub plan_file: String,
    pub nnn: i64,
    pub decision: Option<String>,
    pub decided: bool,
    pub children: Vec<ChildSpecJs>,
}

/// napi-еквівалент `mt spawn [name]` без флагів — read-only перегляд plan-review.
#[napi]
pub fn spawn_review(name: Option<String>, root: Option<String>) -> Result<PlanReviewJs> {
    let tasks_dir = resolve_tasks_dir(root.as_deref())?;
    let node_path = resolve_node_path(name, &tasks_dir)?;
    let review =
        mt_core::spawn::plan_review(&tasks_dir, &node_path).map_err(Error::from_reason)?;
    Ok(PlanReviewJs {
        plan_file: review.plan_file,
        nnn: review.nnn as i64,
        decision: review.decision,
        decided: review.decided,
        children: review.children.into_iter().map(ChildSpecJs::from).collect(),
    })
}

#[napi(object)]
pub struct SpawnApproveResult {
    pub approved_file: String,
    pub children: Vec<String>,
}

/// napi-еквівалент `mt spawn [name] --approve`.
#[napi]
pub fn spawn_approve(name: Option<String>, root: Option<String>) -> Result<SpawnApproveResult> {
    let tasks_dir = resolve_tasks_dir(root.as_deref())?;
    let node_path = resolve_node_path(name, &tasks_dir)?;
    let outcome =
        mt_core::spawn::spawn_approve(&tasks_dir, &node_path).map_err(Error::from_reason)?;
    Ok(SpawnApproveResult {
        approved_file: outcome.approved_file,
        children: outcome.children,
    })
}

#[napi(object)]
pub struct SpawnRejectResult {
    pub rejected_file: String,
}

/// napi-еквівалент `mt spawn [name] --reject <reason>`.
#[napi]
pub fn spawn_reject(
    name: Option<String>,
    reason: String,
    root: Option<String>,
) -> Result<SpawnRejectResult> {
    let tasks_dir = resolve_tasks_dir(root.as_deref())?;
    let node_path = resolve_node_path(name, &tasks_dir)?;
    let file = mt_core::spawn::spawn_reject(&tasks_dir, &node_path, &reason)
        .map_err(Error::from_reason)?;
    Ok(SpawnRejectResult { rejected_file: file })
}
