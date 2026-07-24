//! Резолв tasks-директорії та шляху вузла — спільна логіка для всіх команд.

use std::path::{Path, PathBuf};

/// Знаходить tasks-директорію (як [`mt_core::find_tasks_dir`]), або, якщо
/// `.mt.json`/`mt/` ще не існують у поточному корені — бутстрапить їх
/// (спец. «setup злито в init»: тільки `mt init` викликає це з `bootstrap:
/// true`, решта команд — `false` і повертають помилку як є).
pub fn resolve_tasks_dir(bootstrap: bool) -> Result<String, String> {
    match mt_core::find_tasks_dir() {
        Ok(dir) => Ok(dir),
        Err(_) if bootstrap => bootstrap_tasks_dir(),
        Err(err) => Err(err),
    }
}

fn bootstrap_tasks_dir() -> Result<String, String> {
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let mt_json = cwd.join(".mt.json");
    if !mt_json.exists() {
        // Явний `mt_dir` — `find_tasks_dir()` інакше вимагає, щоб `mt/` уже мала
        // задачу-безпосереднього нащадка (евристика за назвою директорії), що
        // не виконується для щойно створеного порожнього/вкладеного графу.
        std::fs::write(&mt_json, "{\"mt_dir\": \"./mt\"}\n").map_err(|e| e.to_string())?;
    }
    let tasks_dir = cwd.join("mt");
    std::fs::create_dir_all(&tasks_dir).map_err(|e| e.to_string())?;
    Ok(tasks_dir.to_string_lossy().into_owned())
}

/// Резолвить шлях вузла: явний аргумент (валідується) або, якщо відсутній,
/// шлях поточної директорії відносно `tasks_dir` (агент запускає команду
/// зсередини директорії задачі — контракт старого `mt verify`).
pub fn resolve_node_path(explicit: Option<String>, tasks_dir: &str) -> Result<String, String> {
    if let Some(name) = explicit {
        mt_core::validate_name(&name)?;
        return Ok(name);
    }
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    node_path_from_cwd(&cwd, tasks_dir)
}

/// Чиста частина [`resolve_node_path`] — `cwd` ін'єктується для тестованості
/// (без мутації глобального `std::env::set_current_dir`).
fn node_path_from_cwd(cwd: &Path, tasks_dir: &str) -> Result<String, String> {
    let tasks_dir_abs = std::fs::canonicalize(tasks_dir)
        .map_err(|e| format!("tasks-директорія {tasks_dir}: {e}"))?;
    let cwd_abs = std::fs::canonicalize(cwd).map_err(|e| e.to_string())?;
    let rel = cwd_abs.strip_prefix(&tasks_dir_abs).map_err(|_| {
        "не вдалося визначити задачу з поточної директорії — вкажи <name> явно".to_string()
    })?;
    if rel.as_os_str().is_empty() {
        return Err("вкажи <name> задачі явно (запущено в корені tasks-директорії)".to_string());
    }
    let node_path = rel.to_string_lossy().replace('\\', "/");
    mt_core::validate_name(&node_path)?;
    Ok(node_path)
}

/// Корінь git-репо (для worktree/claim-операцій), від `tasks_dir`.
pub fn repo_root(tasks_dir: &str) -> Result<PathBuf, String> {
    mt_core::claims::discover_repo_root(Path::new(tasks_dir))
}

/// Ефективний `.mt.json` проєкту (без per-node override-шарів — для
/// команд, що не прив'язані до конкретного вузла: `check`/`worktree`/`auto`).
pub fn project_config(tasks_dir: &str) -> serde_json::Value {
    let project_root = Path::new(tasks_dir)
        .parent()
        .unwrap_or(Path::new(tasks_dir));
    let raw = std::fs::read_to_string(project_root.join(".mt.json")).ok();
    mt_core::config::merge_config(raw.as_deref())
}

/// Якщо задано `--root`, переходить у нього перед резолвом tasks-директорії
/// (глобальна опція старого CLI: «Виконати команду в іншому корені проекту»).
pub fn apply_root(root: &Option<PathBuf>) -> Result<(), String> {
    if let Some(dir) = root {
        std::env::set_current_dir(dir).map_err(|e| format!("--root {}: {e}", dir.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_name_bypasses_cwd_and_is_validated() {
        assert_eq!(
            resolve_node_path(Some("a/b".to_string()), "/nonexistent").unwrap(),
            "a/b"
        );
        assert!(resolve_node_path(Some("../escape".to_string()), "/nonexistent").is_err());
    }

    #[test]
    fn node_path_from_cwd_computes_relative_path() {
        let tasks_dir = tempfile::tempdir().unwrap();
        let node_dir = tasks_dir.path().join("research/collect-data");
        std::fs::create_dir_all(&node_dir).unwrap();
        assert_eq!(
            node_path_from_cwd(&node_dir, tasks_dir.path().to_str().unwrap()).unwrap(),
            "research/collect-data"
        );
    }

    #[test]
    fn node_path_from_cwd_rejects_tasks_root_itself() {
        let tasks_dir = tempfile::tempdir().unwrap();
        assert!(node_path_from_cwd(tasks_dir.path(), tasks_dir.path().to_str().unwrap()).is_err());
    }

    #[test]
    fn node_path_from_cwd_rejects_outside_tasks_dir() {
        let tasks_dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        assert!(node_path_from_cwd(outside.path(), tasks_dir.path().to_str().unwrap()).is_err());
    }

    #[test]
    fn project_config_merges_mt_json_with_defaults() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join(".mt.json"), r#"{"stale_worktree_min": 5}"#).unwrap();
        let tasks_dir = root.path().join("mt");
        std::fs::create_dir_all(&tasks_dir).unwrap();
        let config = project_config(tasks_dir.to_str().unwrap());
        assert_eq!(config["stale_worktree_min"], 5);
        assert_eq!(config["mt_dir"], "./mt"); // дефолт лишається
    }

    #[test]
    fn project_config_defaults_when_mt_json_absent() {
        let root = tempfile::tempdir().unwrap();
        let tasks_dir = root.path().join("mt");
        std::fs::create_dir_all(&tasks_dir).unwrap();
        let config = project_config(tasks_dir.to_str().unwrap());
        assert_eq!(config["stale_worktree_min"], 30);
    }
}
