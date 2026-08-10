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

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::Utc;

use crate::validate_name;

/// `mt stop` — зупиняє виконання вузла й нащадків (graph.md, «Протокол
/// патчу вузла»: `mt stop` наступників **від листів**, далі `invalidate`).
///
/// Порядок від листів принциповий: зупинений батько інакше встиг би
/// підхопити ще живого нащадка як завершеного. Знімається локальний
/// running-маркер; claim лишається протухати за lease — забирати чужий
/// claim силою тут не можна, це зробить takeover за штатними правилами.
///
/// Повертає шляхи вузлів, з яких маркер справді знято.
pub fn stop(tasks_dir: &str, node_path: &str) -> Result<Vec<String>, String> {
    validate_name(node_path)?;
    let dir = Path::new(tasks_dir).join(node_path);
    if !dir.join("task.md").is_file() {
        return Err(format!("node not found: {node_path}"));
    }
    let mut stopped = Vec::new();
    stop_rec(&dir, node_path, &mut stopped)?;
    Ok(stopped)
}

fn stop_rec(dir: &Path, node_path: &str, stopped: &mut Vec<String>) -> Result<(), String> {
    // Спершу нащадки — від листів угору.
    for child in crate::lifecycle::child_nodes(dir) {
        let child_path = format!("{node_path}/{child}");
        stop_rec(&dir.join(&child), &child_path, stopped)?;
    }
    let mut removed = false;
    for entry in fs::read_dir(dir).map_err(|e| e.to_string())?.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("running_") {
            fs::remove_file(entry.path()).map_err(|e| e.to_string())?;
            removed = true;
        }
    }
    if removed {
        stopped.push(node_path.to_string());
    }
    Ok(())
}

/// Порівняння hash нового fact із заархівованим (graph.md, «Протокол патчу
/// вузла»): після re-run інвалідованого вузла — **однаковий → нащадки
/// розблоковуються; різний → cascade invalidate вниз**.
///
/// Сенс: інвалідація батька не мусить коштувати перевиконання всього
/// піддерева. Якщо повторний прогін дав той самий результат, робота
/// нащадків лишається чинною; змінився — їхні входи застаріли.
///
/// Повертає список каскадно інвалідованих нащадків (порожній — результат
/// збігся або порівнювати нема з чим).
pub fn reconcile_after_rerun(tasks_dir: &str, node_path: &str) -> Result<Vec<String>, String> {
    validate_name(node_path)?;
    let dir = Path::new(tasks_dir).join(node_path);
    let nnn = crate::accepted_fact_nnn(&dir);
    if nnn == 0 {
        return Ok(Vec::new()); // ще немає прийнятого результату
    }
    let Some(previous) = latest_archived_fact(&dir) else {
        return Ok(Vec::new()); // вузол не інвалідували — нема з чим звіряти
    };
    let current =
        fs::read_to_string(dir.join(format!("fact_{nnn:03}.md"))).map_err(|e| e.to_string())?;
    if fact_digest(&current) == fact_digest(&previous) {
        return Ok(Vec::new()); // результат той самий — нащадки чинні
    }

    let mut cascaded = Vec::new();
    let ts = timestamp();
    for child in child_nodes(&dir) {
        let child_path = format!("{node_path}/{child}");
        invalidate_rec(&dir.join(&child), &child_path, &ts, true, &mut cascaded)?;
    }
    if !cascaded.is_empty() {
        let repo_root = repo_root_for(tasks_dir);
        if let Some(root) = &repo_root {
            let after = snapshot(root, &dir);
            // `before` тут — стан після каскаду плюс архіви; публікуємо
            // поточний зріз піддерева цілком.
            publish_mutation(
                tasks_dir,
                &BTreeSet::new(),
                &after,
                &format!("mt: cascade invalidate під {node_path} (fact змінився)"),
            )?;
        }
    }
    Ok(cascaded)
}

/// Тіло fact-у без frontmatter — порівнюємо зміст результату, а не
/// час створення чи актора, які змінюються щоразу.
fn fact_digest(content: &str) -> String {
    crate::frontmatter::get_body(content).trim().to_string()
}

/// Найсвіжіший заархівований `fact_*` із `history/*-invalidate/`.
fn latest_archived_fact(dir: &Path) -> Option<String> {
    let history = dir.join("history");
    let mut archives: Vec<PathBuf> = fs::read_dir(&history)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.is_dir()
                && p.file_name()
                    .map(|n| n.to_string_lossy().ends_with("-invalidate"))
                    .unwrap_or(false)
        })
        .collect();
    archives.sort();
    let archive = archives.last()?;
    let mut facts: Vec<PathBuf> = fs::read_dir(archive)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .map(|n| n.to_string_lossy().starts_with("fact_"))
                .unwrap_or(false)
        })
        .collect();
    facts.sort();
    fs::read_to_string(facts.last()?).ok()
}

/// Знімок файлів піддерева шляхами відносно кореня репо. Відсутня
/// директорія — порожній знімок (kill лишає саме такий стан).
fn snapshot(repo_root: &Path, dir: &Path) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(snapshot(repo_root, &path));
        } else if let Ok(rel) = path.strip_prefix(repo_root) {
            out.insert(rel.to_string_lossy().replace('\\', "/"));
        }
    }
    out
}

/// Публікує наслідки lifecycle-мутації одним atomic commit: файли, що
/// зникли — видаленнями, наявні — поточним вмістом (graph.md, «Протокол
/// патчу вузла»: правка вузла завершується fenced publish).
///
/// Fail-open поза git-репозиторієм — як і `spawn`: на голому дереві
/// публікувати нікуди, мутація лишається локальною.
fn publish_mutation(
    tasks_dir: &str,
    before: &BTreeSet<String>,
    after: &BTreeSet<String>,
    message: &str,
) -> Result<(), String> {
    let Ok(repo_root) = crate::claims::discover_main_worktree_root(Path::new(tasks_dir)) else {
        return Ok(());
    };
    let mut changes: Vec<crate::publish::FileChange> = Vec::new();
    for gone in before.difference(after) {
        changes.push((gone.clone(), None));
    }
    for present in after {
        let content = fs::read_to_string(repo_root.join(present)).map_err(|e| e.to_string())?;
        changes.push((present.clone(), Some(content)));
    }
    if changes.is_empty() {
        return Ok(());
    }

    let config = crate::config::merge_config(
        fs::read_to_string(repo_root.join(".mt.json"))
            .ok()
            .as_deref(),
    );
    let outcome = crate::publish::publish_lifecycle(
        &repo_root,
        &crate::runner::worktrees_dir_path(&repo_root, &config),
        &changes,
        message,
        config
            .get("publish_retry_max")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(8) as u32,
        config
            .get("publish_retry_base_ms")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(250),
    )?;
    if !outcome.published {
        return Err(
            "lifecycle: вичерпано publish retry — мутація лишилась локальною, повторіть пізніше"
                .to_string(),
        );
    }
    Ok(())
}

/// Корінь репо для знімків, якщо ми в git-дереві.
fn repo_root_for(tasks_dir: &str) -> Option<PathBuf> {
    crate::claims::discover_main_worktree_root(Path::new(tasks_dir)).ok()
}

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
    let repo_root = repo_root_for(tasks_dir);
    let before = repo_root
        .as_ref()
        .map(|root| snapshot(root, &dir))
        .unwrap_or_default();

    let ts = timestamp();
    let mut archived = Vec::new();
    invalidate_rec(&dir, node_path, &ts, cascade, &mut archived)?;

    if let Some(root) = &repo_root {
        publish_mutation(
            tasks_dir,
            &before,
            &snapshot(root, &dir),
            &format!("mt: invalidate {node_path}"),
        )?;
    }
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
    let repo_root = repo_root_for(tasks_dir);
    let before = repo_root
        .as_ref()
        .map(|r| snapshot(r, &dir))
        .unwrap_or_default();

    // `mt kill` — «остаточне видалення піддерева з topology» (graph.md).
    // У main публікується саме зникнення піддерева; локальний архів у
    // `<tasks-root>/.history/` — страхувальна копія на машині, не частина
    // топології, тому в коміт не йде.
    let outcome = if has_run_artifacts(&dir) {
        let archive_name = format!("{}-kill-{}", timestamp(), node_path.replace('/', "-"));
        let history = root.join(".history");
        fs::create_dir_all(&history).map_err(|e| e.to_string())?;
        fs::rename(&dir, history.join(&archive_name)).map_err(|e| e.to_string())?;
        format!(".history/{archive_name}")
    } else {
        fs::remove_dir_all(&dir).map_err(|e| e.to_string())?;
        format!("deleted:{node_path}")
    };

    if repo_root.is_some() {
        publish_mutation(
            tasks_dir,
            &before,
            &BTreeSet::new(),
            &format!("mt: kill {node_path}"),
        )?;
    }
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── mt stop і звірка після re-run ──

    #[test]
    fn stop_clears_markers_from_leaves_up() {
        let tmp = fixture();
        let root = tmp.path().to_string_lossy().into_owned();
        let node = tmp.path().join("research");
        fs::write(node.join("running_1_until_9999999999"), "").unwrap();
        fs::write(node.join("analyze/running_2_until_9999999999"), "").unwrap();

        let stopped = stop(&root, "research").unwrap();
        // Порядок від листів: дитина перед батьком.
        assert_eq!(stopped, ["research/analyze", "research"]);
        assert!(!crate::has_running_marker(&node));
        assert!(!crate::has_running_marker(&node.join("analyze")));
    }

    #[test]
    fn stop_is_quiet_when_nothing_runs() {
        let tmp = fixture();
        let root = tmp.path().to_string_lossy().into_owned();
        assert!(stop(&root, "research").unwrap().is_empty());
    }

    #[test]
    fn same_fact_after_rerun_keeps_descendants() {
        let tmp = fixture();
        let root = tmp.path().to_string_lossy().into_owned();
        let node = tmp.path().join("research");
        let fact = "---\nschema_version: 1\n---\n\n## Summary\n\nті самі 42 рядки\n";
        fs::write(node.join("fact_001.md"), fact).unwrap();

        invalidate(&root, "research", false).unwrap();
        // Повторний прогін дав той самий зміст (інший created_at — не рахується).
        fs::write(
            node.join("fact_001.md"),
            "---\nschema_version: 1\ncreated_at: 2026-08-10T00:00:00Z\n---\n\n## Summary\n\nті самі 42 рядки\n",
        )
        .unwrap();

        assert!(
            reconcile_after_rerun(&root, "research").unwrap().is_empty(),
            "нащадків не чіпаємо — їхні входи не змінились"
        );
        assert!(node.join("analyze/fact_001.md").is_file());
    }

    #[test]
    fn changed_fact_after_rerun_cascades_down() {
        let tmp = fixture();
        let root = tmp.path().to_string_lossy().into_owned();
        let node = tmp.path().join("research");
        fs::write(
            node.join("fact_001.md"),
            "---\n---\n\n## Summary\n\nстарий результат\n",
        )
        .unwrap();

        invalidate(&root, "research", false).unwrap();
        fs::write(
            node.join("fact_001.md"),
            "---\n---\n\n## Summary\n\nНОВИЙ результат\n",
        )
        .unwrap();

        let cascaded = reconcile_after_rerun(&root, "research").unwrap();
        assert_eq!(cascaded, ["research/analyze"]);
        // Chain нащадка заархівовано — його входи застаріли.
        assert!(!node.join("analyze/fact_001.md").is_file());
    }

    #[test]
    fn reconcile_is_noop_without_invalidation_history() {
        let tmp = fixture();
        let root = tmp.path().to_string_lossy().into_owned();
        // Вузол не інвалідували — звіряти нема з чим.
        assert!(reconcile_after_rerun(&root, "research").unwrap().is_empty());
    }

    // ── git-протокол (graph.md, «Протокол патчу вузла») ──

    /// Вузол із chain-файлами в git-репо з origin; повертає repo і tasks_dir.
    fn repo_fixture() -> (crate::test_support::TestRepo, String) {
        let repo = crate::test_support::TestRepo::new();
        let tasks_root = repo.work.path().join("mt");
        let node = tasks_root.join("research");
        fs::create_dir_all(&node).unwrap();
        for name in ["task.md", "a.md", "run_001.md", "fact_001.md"] {
            fs::write(node.join(name), "---\nschema_version: 1\n---\n").unwrap();
        }
        crate::test_support::commit_all(repo.work.path(), "add node");
        crate::test_support::push_head(repo.work.path(), "refs/heads/main");
        let tasks_dir = tasks_root.to_string_lossy().into_owned();
        (repo, tasks_dir)
    }

    fn in_main(repo: &crate::test_support::TestRepo, path: &str) -> bool {
        let sha = crate::git::GitRepository::open(repo.work.path())
            .unwrap()
            .resolve_ref("refs/remotes/origin/main")
            .unwrap();
        crate::git::GitRepository::open(repo.work.path())
            .unwrap()
            .read_blob_at_commit(&sha, path)
            .is_ok()
    }

    #[test]
    fn invalidate_publishes_archive_move_to_main() {
        let (repo, tasks) = repo_fixture();
        assert!(in_main(&repo, "mt/research/fact_001.md"));

        invalidate(&tasks, "research", false).unwrap();

        // Chain зник із топології, але лишився в history/ — і те, і те в main.
        assert!(!in_main(&repo, "mt/research/fact_001.md"), "chain прибрано");
        assert!(in_main(&repo, "mt/research/task.md"), "контракт лишився");
        let sha = crate::git::GitRepository::open(repo.work.path())
            .unwrap()
            .resolve_ref("refs/remotes/origin/main")
            .unwrap();
        let archived = crate::test_support::remote_refs(repo.work.path());
        assert!(!archived.is_empty(), "main існує: {sha}");
        // Архів під history/<ts>-invalidate/ — знаходимо перебором дерева.
        let hist = repo.work.path().join("mt/research/history");
        let any_archived = fs::read_dir(&hist)
            .unwrap()
            .flatten()
            .any(|e| e.path().join("fact_001.md").is_file());
        assert!(any_archived, "архів на диску");
    }

    #[test]
    fn kill_publishes_subtree_removal_to_main() {
        let (repo, tasks) = repo_fixture();
        kill(&tasks, "research").unwrap();

        assert!(!in_main(&repo, "mt/research/task.md"), "вузол зник із main");
        assert!(!in_main(&repo, "mt/research/fact_001.md"));
        // Локальний архів лишається на машині, але топології не засмічує.
        assert!(repo.work.path().join("mt/.history").is_dir());
    }

    #[test]
    fn lifecycle_works_without_git_repo() {
        // Fail-open, як і spawn: на голому дереві мутація лишається локальною.
        let tmp = fixture();
        let root = tmp.path().to_string_lossy().into_owned();
        assert!(invalidate(&root, "research", true).is_ok());
    }

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
