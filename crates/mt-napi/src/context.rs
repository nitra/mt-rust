//! Резолв tasks-директорії/кореня репо — той самий тонкий glue-шар, що й
//! `crates/mt/src/context.rs` для CLI: жодної бізнес-логіки, лише виклики
//! `mt-core` (спека `docs/specs/2026-07-27-mt-napi-binding.md`, рішення В).

use std::path::{Path, PathBuf};

use napi::bindgen_prelude::*;

/// Якщо задано `root` — переходить у нього (як `--root` у CLI), потім
/// резолвить tasks-директорію. Проєкт має бути вже ініціалізований
/// (`mt init`) — на відміну від CLI, napi-шар не бутстрапить `.mt.json`.
pub fn resolve_tasks_dir(root: Option<&str>) -> Result<String> {
    if let Some(dir) = root {
        std::env::set_current_dir(dir)
            .map_err(|e| Error::from_reason(format!("root {dir}: {e}")))?;
    }
    mt_core::find_tasks_dir().map_err(Error::from_reason)
}

/// Корінь **головного** worktree репозиторію, незалежно від того, чи
/// `tasks_dir` лежить у linked dev-worktree (той самий нюанс, що й CLI
/// `crate::context::git_root` — див. `crates/mt/src/context.rs`). Для
/// worktree lifecycle-операцій (`create/remove/status`), де фізичне
/// розташування worktree має бути repo-wide, а не прив'язане до checkout-у,
/// звідки викликано binding.
pub fn main_worktree_root(tasks_dir: &str) -> Result<PathBuf> {
    mt_core::claims::discover_main_worktree_root(Path::new(tasks_dir)).map_err(Error::from_reason)
}

pub fn project_config(tasks_dir: &str) -> serde_json::Value {
    let project_root = Path::new(tasks_dir)
        .parent()
        .unwrap_or_else(|| Path::new(tasks_dir));
    let raw = std::fs::read_to_string(project_root.join(".mt.json")).ok();
    mt_core::config::merge_config(raw.as_deref())
}

/// Явний `name`, або (як у CLI) шлях вузла з поточної директорії відносно
/// `tasks_dir`.
pub fn resolve_node_path(explicit: Option<String>, tasks_dir: &str) -> Result<String> {
    if let Some(name) = explicit {
        mt_core::validate_name(&name).map_err(Error::from_reason)?;
        return Ok(name);
    }
    let cwd = std::env::current_dir().map_err(|e| Error::from_reason(e.to_string()))?;
    node_path_from_cwd(&cwd, tasks_dir)
}

fn node_path_from_cwd(cwd: &Path, tasks_dir: &str) -> Result<String> {
    let tasks_dir_abs = std::fs::canonicalize(tasks_dir)
        .map_err(|e| Error::from_reason(format!("tasks-директорія {tasks_dir}: {e}")))?;
    let cwd_abs = std::fs::canonicalize(cwd).map_err(|e| Error::from_reason(e.to_string()))?;
    let rel = cwd_abs.strip_prefix(&tasks_dir_abs).map_err(|_| {
        Error::from_reason("не вдалося визначити задачу з поточної директорії — вкажи name явно")
    })?;
    if rel.as_os_str().is_empty() {
        return Err(Error::from_reason(
            "вкажи name задачі явно (запущено в корені tasks-директорії)",
        ));
    }
    let node_path = rel.to_string_lossy().replace('\\', "/");
    mt_core::validate_name(&node_path).map_err(Error::from_reason)?;
    Ok(node_path)
}
