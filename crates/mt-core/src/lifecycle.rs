//! Lifecycle-мутації вузла: `mt invalidate` та `mt kill` (спека mt.md).
//!
//! Файловий рівень (без git-протоколу — fenced publish прийде з фазою git):
//! - invalidate: архівує version chain у `history/<ts>-invalidate/`, нова
//!   chain стартує з NNN=001; каскад вниз по нащадках; без sentinel-файлів —
//!   стан derived з відсутності `fact_*.md`.
//! - kill: якщо піддерево вузла (сам вузол + нащадки) не має жодного
//!   run-артефакту (chain-файли, `run-summary.md`, `history/`) — вузол
//!   видаляється назавжди (не було що архівувати, помилково створений
//!   вузол); інакше архівується у `<tasks-root>/.history/<ts>-kill-<path>/`
//!   і прибирається директорія; каскад повний за визначенням (піддерево).

use std::fs;
use std::path::Path;

use chrono::Utc;

use crate::validate_name;

/// Префікси файлів version chain, які archive-ує invalidate (§ mt invalidate).
const CHAIN_PREFIXES: [&str; 6] = [
    "fact_",
    "run_",
    "pending-audit_",
    "audit-result_",
    "clarification_",
    "amended_",
];

fn is_chain_file(name: &str) -> bool {
    if name == "unresolvable.md" {
        return true; // термінальний маркер — частина chain, архівується разом
    }
    CHAIN_PREFIXES
        .iter()
        .any(|p| name.strip_prefix(p).is_some_and(|r| r.ends_with(".md")))
}

fn timestamp() -> String {
    Utc::now().format("%Y%m%d-%H%M%S").to_string()
}

/// Архівує version chain одного вузла (без рекурсії). Повертає `true`,
/// якщо було що архівувати.
fn archive_chain(dir: &Path, ts: &str) -> Result<bool, String> {
    let mut chain = Vec::new();
    for entry in fs::read_dir(dir).map_err(|e| e.to_string())?.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if entry.file_type().map(|t| t.is_file()).unwrap_or(false) && is_chain_file(&name) {
            chain.push(name);
        }
    }
    // run-summary.md видаляється (нова chain — нова історія), не архівується.
    let _ = fs::remove_file(dir.join("run-summary.md"));
    if chain.is_empty() {
        return Ok(false);
    }
    let archive = dir.join("history").join(format!("{ts}-invalidate"));
    fs::create_dir_all(&archive).map_err(|e| e.to_string())?;
    for name in &chain {
        fs::rename(dir.join(name), archive.join(name)).map_err(|e| e.to_string())?;
    }
    Ok(true)
}

/// Дочірні вузли (директорії з `task.md`); `history/` і приховані — пропуск.
pub(crate) fn child_nodes(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || name == "history" || name == "deps" {
            continue;
        }
        let path = entry.path();
        if path.is_dir() && path.join("task.md").is_file() {
            out.push(name);
        }
    }
    out
}

/// `mt invalidate <path>`: архівує chain вузла і (cascade) всіх нащадків.
/// Повертає шляхи вузлів (відносно tasks root), де chain було архівовано.
pub fn invalidate(tasks_dir: &str, node_path: &str, cascade: bool) -> Result<Vec<String>, String> {
    validate_name(node_path)?;
    let dir = Path::new(tasks_dir).join(node_path);
    if !dir.join("task.md").is_file() {
        return Err(format!("node not found: {node_path}"));
    }
    let ts = timestamp();
    let mut archived = Vec::new();
    invalidate_rec(&dir, node_path, &ts, cascade, &mut archived)?;
    Ok(archived)
}

fn invalidate_rec(
    dir: &Path,
    node_path: &str,
    ts: &str,
    cascade: bool,
    archived: &mut Vec<String>,
) -> Result<(), String> {
    if archive_chain(dir, ts)? {
        archived.push(node_path.to_string());
    }
    if !cascade {
        return Ok(());
    }
    for child in child_nodes(dir) {
        invalidate_rec(
            &dir.join(&child),
            &format!("{node_path}/{child}"),
            ts,
            cascade,
            archived,
        )?;
    }
    Ok(())
}

/// Чи має вузол (без рекурсії в нащадків) артефакти запуску: chain-файли,
/// `run-summary.md`, або `history/` (архів попередніх invalidate).
fn has_run_artifacts_here(dir: &Path) -> bool {
    if dir.join("run-summary.md").is_file() || dir.join("history").is_dir() {
        return true;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return false;
    };
    entries.flatten().any(|entry| {
        entry.file_type().map(|t| t.is_file()).unwrap_or(false)
            && is_chain_file(&entry.file_name().to_string_lossy())
    })
}

/// Чи має піддерево вузла (сам вузол + всі нащадки) бодай один run-артефакт.
fn has_run_artifacts(dir: &Path) -> bool {
    has_run_artifacts_here(dir)
        || child_nodes(dir)
            .iter()
            .any(|c| has_run_artifacts(&dir.join(c)))
}

/// `mt kill <path>` (файловий рівень): якщо піддерево вузла ще не мало
/// жодного запуску — видаляє його назавжди; інакше архівує весь вузол
/// з нащадками у `<tasks-root>/.history/<ts>-kill-<path>/` і прибирає
/// директорію. Повертає `.history/<archive>` (архівовано) або
/// `deleted:<node_path>` (видалено без історії).
pub fn kill(tasks_dir: &str, node_path: &str) -> Result<String, String> {
    validate_name(node_path)?;
    let root = Path::new(tasks_dir);
    let dir = root.join(node_path);
    if !dir.join("task.md").is_file() {
        return Err(format!("node not found: {node_path}"));
    }
    if !has_run_artifacts(&dir) {
        fs::remove_dir_all(&dir).map_err(|e| e.to_string())?;
        return Ok(format!("deleted:{node_path}"));
    }
    let archive_name = format!("{}-kill-{}", timestamp(), node_path.replace('/', "-"));
    let history = root.join(".history");
    fs::create_dir_all(&history).map_err(|e| e.to_string())?;
    let target = history.join(&archive_name);
    fs::rename(&dir, &target).map_err(|e| e.to_string())?;
    Ok(format!(".history/{archive_name}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        let node = tmp.path().join("research");
        let child = node.join("analyze");
        fs::create_dir_all(&child).unwrap();
        for (dir, files) in [
            (
                &node,
                vec![
                    "task.md",
                    "a.md",
                    "plan_001.md",
                    "run_001.md",
                    "fact_001.md",
                    "run-summary.md",
                ],
            ),
            (
                &child,
                vec![
                    "task.md",
                    "a.md",
                    "run_001.md",
                    "fact_001.md",
                    "audit-result_001.md",
                ],
            ),
        ] {
            for f in files {
                fs::write(dir.join(f), "x").unwrap();
            }
        }
        tmp
    }

    #[test]
    fn invalidate_archives_chain_and_cascades() {
        let tmp = fixture();
        let root = tmp.path().to_string_lossy().into_owned();
        let archived = invalidate(&root, "research", true).unwrap();
        assert_eq!(archived, ["research", "research/analyze"]);

        let node = tmp.path().join("research");
        // task/plan/прапор лишаються; chain-файли поїхали в history/.
        assert!(node.join("task.md").is_file());
        assert!(node.join("plan_001.md").is_file());
        assert!(node.join("a.md").is_file());
        assert!(!node.join("fact_001.md").exists());
        assert!(!node.join("run_001.md").exists());
        assert!(!node.join("run-summary.md").exists());
        let hist = fs::read_dir(node.join("history"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap();
        assert!(hist.path().join("fact_001.md").is_file());
        // Дитина теж: audit-файл у архіві.
        assert!(!node.join("analyze/audit-result_001.md").exists());
    }

    #[test]
    fn invalidate_no_cascade_keeps_children() {
        let tmp = fixture();
        let root = tmp.path().to_string_lossy().into_owned();
        let archived = invalidate(&root, "research", false).unwrap();
        assert_eq!(archived, ["research"]);
        assert!(tmp.path().join("research/analyze/fact_001.md").is_file());
    }

    #[test]
    fn kill_moves_subtree_to_history() {
        let tmp = fixture();
        let root = tmp.path().to_string_lossy().into_owned();
        let archive = kill(&root, "research").unwrap();
        assert!(archive.starts_with(".history/"));
        assert!(archive.ends_with("-kill-research"));
        assert!(!tmp.path().join("research").exists());
        let archived_root = tmp.path().join(&archive);
        assert!(archived_root.join("task.md").is_file());
        assert!(archived_root.join("analyze/fact_001.md").is_file());
    }

    #[test]
    fn kill_missing_node_errors() {
        let tmp = fixture();
        let root = tmp.path().to_string_lossy().into_owned();
        assert!(kill(&root, "nope").is_err());
        assert!(kill(&root, "../escape").is_err());
    }

    #[test]
    fn kill_deletes_fresh_node_without_run_history() {
        let tmp = tempfile::tempdir().unwrap();
        let node = tmp.path().join("draft");
        fs::create_dir_all(&node).unwrap();
        fs::write(node.join("task.md"), "x").unwrap();
        fs::write(node.join("plan_001.md"), "x").unwrap();

        let root = tmp.path().to_string_lossy().into_owned();
        let result = kill(&root, "draft").unwrap();
        assert_eq!(result, "deleted:draft");
        assert!(!node.exists());
        assert!(!tmp.path().join(".history").exists());
    }

    #[test]
    fn kill_archives_when_only_a_descendant_has_run_history() {
        let tmp = tempfile::tempdir().unwrap();
        let node = tmp.path().join("draft");
        let child = node.join("sub");
        fs::create_dir_all(&child).unwrap();
        fs::write(node.join("task.md"), "x").unwrap();
        fs::write(child.join("task.md"), "x").unwrap();
        fs::write(child.join("run_001.md"), "x").unwrap();

        let root = tmp.path().to_string_lossy().into_owned();
        let archive = kill(&root, "draft").unwrap();
        assert!(archive.starts_with(".history/"));
        assert!(!node.exists());
        assert!(tmp.path().join(&archive).join("sub/run_001.md").is_file());
    }
}
