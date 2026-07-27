//! `scan` — napi-еквівалент `mt scan`: викликає той самий `mt_core::scan_tasks`,
//! що й `crates/mt/src/commands/graph.rs::run_scan`.

use napi::bindgen_prelude::*;
use napi_derive::napi;

use crate::context::resolve_tasks_dir;

#[napi(object)]
pub struct TaskNodeJs {
    pub id: String,
    pub path: String,
    /// snake_case-рядок (той самий контракт, що й `--json` CLI: `serde(rename_all = "snake_case")`).
    pub state: String,
    pub deps: Vec<String>,
    pub mode: String,
    pub budget_sec: Option<i64>,
    pub budget_hard_sec: Option<i64>,
    pub deadline: Option<String>,
    pub hint: Option<String>,
    pub created_at: Option<String>,
    pub children: Vec<TaskNodeJs>,
    pub is_composite: bool,
}

impl From<mt_core::TaskNode> for TaskNodeJs {
    fn from(n: mt_core::TaskNode) -> Self {
        let state = serde_json::to_value(&n.state)
            .ok()
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_else(|| "unknown".to_string());
        Self {
            id: n.id,
            path: n.path,
            state,
            deps: n.deps,
            mode: n.mode,
            budget_sec: n.budget_sec.map(|v| v as i64),
            budget_hard_sec: n.budget_hard_sec.map(|v| v as i64),
            deadline: n.deadline,
            hint: n.hint,
            created_at: n.created_at,
            children: n.children.into_iter().map(TaskNodeJs::from).collect(),
            is_composite: n.is_composite,
        }
    }
}

/// napi-еквівалент `mt scan [--root <path>]`.
#[napi]
pub fn scan(root: Option<String>) -> Result<Vec<TaskNodeJs>> {
    let tasks_dir = resolve_tasks_dir(root.as_deref())?;
    let worktrees = mt_core::discover_worktrees(std::path::Path::new(&tasks_dir));
    let tree = mt_core::scan_tasks(tasks_dir, worktrees).map_err(Error::from_reason)?;
    Ok(tree.into_iter().map(TaskNodeJs::from).collect())
}
