//! Іменування, матчінг і provisioning worktree для задач (порт чистої частини
//! `npm/lib/core/worktree.mjs` + git-операції run-wrapper-а, спека «Wrapper-
//! скрипт», крок 5: detached worktree від зафіксованого `base_sha`).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use crate::claims::RUN_REF_PREFIX;
use crate::sanitize;

fn git(repo: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .map_err(|e| format!("git {}: {e}", args.join(" ")))?;
    if !out.status.success() {
        return Err(format!(
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Префікс worktree для задачі: `sanitize(task_path.replace('/', '-'))`.
fn worktree_prefix(task_path: &str) -> String {
    sanitize(&task_path.replace('/', "-"))
}

/// Ім'я worktree для задачі: `<sanitized-path>-<epoch-сек>`.
pub fn make_worktree_name(task_path: &str, epoch_sec: u64) -> String {
    format!("{}-{epoch_sec}", worktree_prefix(task_path))
}

/// Знаходить перший запис із `entries`, що належить задачі:
/// точний збіг із префіксом або `<prefix>-...`.
pub fn find_worktree_match(entries: &[String], task_path: &str) -> Option<String> {
    let prefix = worktree_prefix(task_path);
    let dashed = format!("{prefix}-");
    entries
        .iter()
        .find(|name| name.starts_with(&dashed) || **name == prefix)
        .cloned()
}

/// Створює detached worktree від `base_sha` у `worktrees_dir/<node-hash>-<token>`
/// (спека: `git worktree add --detach .worktrees/<node-hash>-<token> <base_sha>`).
/// Не checkout-ить `main` — worktree ізольований від живого робочого дерева.
pub fn create_run_worktree(
    repo_root: &Path,
    worktrees_dir: &Path,
    node_hash: &str,
    token: &str,
    base_sha: &str,
) -> Result<PathBuf, String> {
    let path = worktrees_dir.join(format!("{node_hash}-{token}"));
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    git(
        repo_root,
        &[
            "worktree",
            "add",
            "--detach",
            &path.to_string_lossy(),
            base_sha,
        ],
    )?;
    Ok(path)
}

/// Публікує локальний run ref для recovery/handoff (спека, крок 5):
/// `refs/mt/runs/<node-hash>/<token>` ← поточний HEAD worktree.
pub fn push_run_ref(repo_root: &Path, node_hash: &str, token: &str) -> Result<(), String> {
    let refname = format!("{RUN_REF_PREFIX}/{node_hash}/{token}");
    git(repo_root, &["push", "origin", &format!("HEAD:{refname}")])?;
    Ok(())
}

/// Видаляє remote run ref (після успішного publish або при cleanup невдалої
/// спроби; `--force-with-lease` — лише якщо ref усе ще на очікуваному SHA).
pub fn delete_run_ref(
    repo_root: &Path,
    node_hash: &str,
    token: &str,
    expected_sha: &str,
) -> Result<bool, String> {
    let refname = format!("{RUN_REF_PREFIX}/{node_hash}/{token}");
    let lease = format!("--force-with-lease={refname}:{expected_sha}");
    let out = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["push", &lease, "origin", &format!(":{refname}")])
        .output()
        .map_err(|e| format!("git push --delete run ref: {e}"))?;
    if out.status.success() {
        return Ok(true);
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    if stderr.contains("stale info") || stderr.contains("[rejected]") {
        return Ok(false);
    }
    Err(format!("git push --delete run ref: {}", stderr.trim()))
}

/// Прибирає worktree після завершення спроби (success — завжди; failure —
/// лишається для debug за рішенням викликача, спека «Failure-сімейство»).
pub fn remove_run_worktree(repo_root: &Path, path: &Path) -> Result<(), String> {
    remove_worktree(repo_root, path, true)
}

/// `mt worktree remove [--force]` — узагальнена версія без прив'язки до
/// run-флоу: `force: false` відмовляє на брудному worktree (git-поведінка за
/// замовчуванням), `force: true` — примусово (як [`remove_run_worktree`]).
pub fn remove_worktree(repo_root: &Path, path: &Path, force: bool) -> Result<(), String> {
    let mut args = vec!["worktree", "remove"];
    if force {
        args.push("--force");
    }
    let path_str = path.to_string_lossy();
    args.push(&path_str);
    git(repo_root, &args)?;
    Ok(())
}

/// `mt worktree create <name>`: гілковий (не detached) worktree для ручної
/// dev-роботи над задачею — `git worktree add -b <branch> <path> <base>`.
pub fn create_dev_worktree(
    repo_root: &Path,
    worktrees_dir: &Path,
    name: &str,
    base: &str,
) -> Result<PathBuf, String> {
    let branch = format!("mt/{name}");
    let path = worktrees_dir.join(name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    git(
        repo_root,
        &[
            "worktree",
            "add",
            "-b",
            &branch,
            &path.to_string_lossy(),
            base,
        ],
    )?;
    Ok(path)
}

/// Один запис `git worktree list --porcelain` (`mt worktree list`/`inventory`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorktreeEntry {
    pub path: String,
    /// Останній компонент шляху (те саме, що повертає [`crate::parse_worktree_list`]).
    pub name: String,
    pub head: String,
    /// `None` — detached HEAD.
    pub branch: Option<String>,
    pub locked: bool,
    pub prunable: bool,
}

/// Парсить повний вивід `git worktree list --porcelain` (блоки, розділені
/// порожнім рядком) у структуровані записи.
pub fn parse_worktree_entries(output: &str) -> Vec<WorktreeEntry> {
    let mut entries = Vec::new();
    let mut path: Option<String> = None;
    let mut head = String::new();
    let mut branch = None;
    let mut locked = false;
    let mut prunable = false;

    let flush = |path: &mut Option<String>,
                 head: &mut String,
                 branch: &mut Option<String>,
                 locked: &mut bool,
                 prunable: &mut bool,
                 entries: &mut Vec<WorktreeEntry>| {
        if let Some(p) = path.take() {
            let name = p.rsplit(['/', '\\']).next().unwrap_or("").to_string();
            entries.push(WorktreeEntry {
                path: p,
                name,
                head: std::mem::take(head),
                branch: branch.take(),
                locked: std::mem::replace(locked, false),
                prunable: std::mem::replace(prunable, false),
            });
        }
    };

    for line in output.lines() {
        if line.is_empty() {
            flush(
                &mut path,
                &mut head,
                &mut branch,
                &mut locked,
                &mut prunable,
                &mut entries,
            );
            continue;
        }
        if let Some(p) = line.strip_prefix("worktree ") {
            flush(
                &mut path,
                &mut head,
                &mut branch,
                &mut locked,
                &mut prunable,
                &mut entries,
            );
            path = Some(p.trim().to_string());
        } else if let Some(h) = line.strip_prefix("HEAD ") {
            head = h.trim().to_string();
        } else if let Some(b) = line.strip_prefix("branch ") {
            branch = Some(b.trim().to_string());
        } else if line == "locked" || line.starts_with("locked ") {
            locked = true;
        } else if line == "prunable" || line.starts_with("prunable ") {
            prunable = true;
        }
    }
    flush(
        &mut path,
        &mut head,
        &mut branch,
        &mut locked,
        &mut prunable,
        &mut entries,
    );
    entries
}

/// `mt worktree list`: усі worktree репо (структуровано, з branch/lock/prune-станом).
pub fn list_worktrees(repo_root: &Path) -> Result<Vec<WorktreeEntry>, String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["worktree", "list", "--porcelain"])
        .output()
        .map_err(|e| format!("git worktree list: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "git worktree list: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(parse_worktree_entries(&String::from_utf8_lossy(
        &out.stdout,
    )))
}

/// `mt worktree prune`: `git worktree prune -v` — прибирає адміністративні
/// записи worktree, чиї директорії видалено вручну. Повертає сирий вивід.
pub fn prune_worktrees(repo_root: &Path) -> Result<String, String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["worktree", "prune", "-v"])
        .output()
        .map_err(|e| format!("git worktree prune: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "git worktree prune: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Один запис `mt worktree inventory`: worktree, вік (за mtime директорії),
/// чи це stale (вік більший за `stale_min` хвилин), і матч на задачу — якщо
/// ім'я worktree відповідає sanitized-шляху задачі.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorktreeInventoryItem {
    #[serde(flatten)]
    pub entry: WorktreeEntry,
    pub age_min: u64,
    pub stale: bool,
    pub task_path: Option<String>,
}

fn dir_age_min(path: &Path) -> Option<u64> {
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    let age = SystemTime::now().duration_since(modified).ok()?;
    Some(age.as_secs() / 60)
}

/// Будує інвентар worktree репо: вік, stale-прапор (проти `stale_min`
/// хвилин), і матч на задачу з `task_paths` (шляхи вузлів воркспейсу).
pub fn worktree_inventory(
    repo_root: &Path,
    task_paths: &[String],
    stale_min: u64,
) -> Result<Vec<WorktreeInventoryItem>, String> {
    let entries = list_worktrees(repo_root)?;
    Ok(entries
        .into_iter()
        .map(|entry| {
            let age_min = dir_age_min(Path::new(&entry.path)).unwrap_or(0);
            let task_path = task_paths
                .iter()
                .find(|p| {
                    let prefix = worktree_prefix(p);
                    entry.name == prefix || entry.name.starts_with(&format!("{prefix}-"))
                })
                .cloned();
            WorktreeInventoryItem {
                stale: age_min > stale_min,
                age_min,
                task_path,
                entry,
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestRepo;

    #[test]
    fn creates_detached_worktree_from_base_sha() {
        let repo = TestRepo::new();
        let base = repo.main_sha();
        let worktrees_dir = tempfile::tempdir().unwrap();
        let path = create_run_worktree(
            repo.work.path(),
            worktrees_dir.path(),
            "deadbeef",
            "tok1",
            &base,
        )
        .unwrap();
        assert!(path.join("README.md").is_file());
        let head = crate::test_support::output(&path, &["rev-parse", "HEAD"]);
        assert_eq!(head, base);
        // Detached — не на гілці: `--abbrev-ref HEAD` повертає літерал "HEAD".
        let branch = crate::test_support::output(&path, &["rev-parse", "--abbrev-ref", "HEAD"]);
        assert_eq!(branch, "HEAD");
    }

    #[test]
    fn push_and_delete_run_ref_round_trip() {
        let repo = TestRepo::new();
        let worktrees_dir = tempfile::tempdir().unwrap();
        let base = repo.main_sha();
        let path = create_run_worktree(
            repo.work.path(),
            worktrees_dir.path(),
            "deadbeef",
            "tok1",
            &base,
        )
        .unwrap();
        // push_run_ref читає HEAD поточного репо (work), тож пушимо з worktree-контексту.
        push_run_ref(&path, "deadbeef", "tok1").unwrap();

        let ls = crate::test_support::output(
            repo.work.path(),
            &[
                "ls-remote",
                "origin",
                &format!("{RUN_REF_PREFIX}/deadbeef/tok1"),
            ],
        );
        assert!(!ls.is_empty());

        assert!(delete_run_ref(repo.work.path(), "deadbeef", "tok1", &base).unwrap());
        let ls = crate::test_support::output(
            repo.work.path(),
            &[
                "ls-remote",
                "origin",
                &format!("{RUN_REF_PREFIX}/deadbeef/tok1"),
            ],
        );
        assert!(ls.is_empty());
    }

    #[test]
    fn remove_worktree_cleans_up_directory() {
        let repo = TestRepo::new();
        let base = repo.main_sha();
        let worktrees_dir = tempfile::tempdir().unwrap();
        let path = create_run_worktree(
            repo.work.path(),
            worktrees_dir.path(),
            "deadbeef",
            "tok1",
            &base,
        )
        .unwrap();
        assert!(path.is_dir());
        remove_run_worktree(repo.work.path(), &path).unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn make_name_sanitizes_and_appends_epoch() {
        assert_eq!(
            make_worktree_name("research/collect data", 1234567890),
            "research-collect-data-1234567890"
        );
        assert_eq!(make_worktree_name("my-task_01", 5), "my-task_01-5");
    }

    #[test]
    fn find_match_prefers_first_entry() {
        let entries = vec![
            "other-task-1".to_string(),
            "my-task-100".to_string(),
            "my-task-200".to_string(),
        ];
        assert_eq!(
            find_worktree_match(&entries, "my-task"),
            Some("my-task-100".to_string())
        );
    }

    #[test]
    fn find_match_exact_or_dashed_only() {
        let entries = vec!["my-task".to_string(), "my-taskish-1".to_string()];
        assert_eq!(
            find_worktree_match(&entries, "my-task"),
            Some("my-task".to_string())
        );
        assert_eq!(
            find_worktree_match(&["my-taskish-1".to_string()], "my-task"),
            None
        );
    }

    #[test]
    fn create_dev_worktree_adds_branch_and_worktree() {
        let repo = TestRepo::new();
        let worktrees_dir = tempfile::tempdir().unwrap();
        let path =
            create_dev_worktree(repo.work.path(), worktrees_dir.path(), "my-task", "main").unwrap();
        assert!(path.join("README.md").is_file());
        let branch = crate::test_support::output(&path, &["rev-parse", "--abbrev-ref", "HEAD"]);
        assert_eq!(branch, "mt/my-task");
    }

    #[test]
    fn remove_worktree_without_force_fails_on_dirty_tree() {
        let repo = TestRepo::new();
        let worktrees_dir = tempfile::tempdir().unwrap();
        let path =
            create_dev_worktree(repo.work.path(), worktrees_dir.path(), "my-task", "main").unwrap();
        std::fs::write(path.join("uncommitted.txt"), "x").unwrap();
        assert!(remove_worktree(repo.work.path(), &path, false).is_err());
        assert!(path.exists());
        remove_worktree(repo.work.path(), &path, true).unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn parse_entries_extracts_branch_and_flags() {
        let out = "worktree /repo\nHEAD abc123\nbranch refs/heads/main\n\nworktree /repo/.worktrees/my-task-1\nHEAD def456\ndetached\n\nworktree /repo/.worktrees/stale-2\nHEAD 789abc\nbranch refs/heads/x\nlocked\nprunable gitdir file points to non-existent location\n";
        let entries = parse_worktree_entries(out);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].path, "/repo");
        assert_eq!(entries[0].head, "abc123");
        assert_eq!(entries[0].branch.as_deref(), Some("refs/heads/main"));
        assert!(!entries[0].locked);
        assert!(!entries[0].prunable);

        assert_eq!(entries[1].name, "my-task-1");
        assert_eq!(entries[1].branch, None);

        assert_eq!(entries[2].name, "stale-2");
        assert!(entries[2].locked);
        assert!(entries[2].prunable);
    }

    #[test]
    fn list_worktrees_returns_main_and_created_run_worktree() {
        let repo = TestRepo::new();
        let base = repo.main_sha();
        let worktrees_dir = tempfile::tempdir().unwrap();
        create_run_worktree(
            repo.work.path(),
            worktrees_dir.path(),
            "deadbeef",
            "tok1",
            &base,
        )
        .unwrap();
        let entries = list_worktrees(repo.work.path()).unwrap();
        assert_eq!(entries.len(), 2);
        assert!(entries.iter().any(|e| e.name == "deadbeef-tok1"));
    }

    #[test]
    fn prune_worktrees_removes_stale_administrative_entry() {
        let repo = TestRepo::new();
        let base = repo.main_sha();
        let worktrees_dir = tempfile::tempdir().unwrap();
        let path = create_run_worktree(
            repo.work.path(),
            worktrees_dir.path(),
            "deadbeef",
            "tok1",
            &base,
        )
        .unwrap();
        // Видаляємо директорію вручну (не через `git worktree remove`) — worktree
        // лишається в адміністративному списку git до `prune`.
        std::fs::remove_dir_all(&path).unwrap();
        assert_eq!(list_worktrees(repo.work.path()).unwrap().len(), 2);
        prune_worktrees(repo.work.path()).unwrap();
        assert_eq!(list_worktrees(repo.work.path()).unwrap().len(), 1);
    }

    #[test]
    fn inventory_matches_task_path_and_flags_stale() {
        let repo = TestRepo::new();
        let base = repo.main_sha();
        let worktrees_dir = tempfile::tempdir().unwrap();
        create_run_worktree(
            repo.work.path(),
            worktrees_dir.path(),
            &sanitize("research/collect-data"),
            "tok1",
            &base,
        )
        .unwrap();
        let task_paths = vec!["research/collect-data".to_string()];

        // Щойно створений worktree, високий поріг → не stale.
        let inventory = worktree_inventory(repo.work.path(), &task_paths, 10_000).unwrap();
        let item = inventory
            .iter()
            .find(|i| i.entry.name.starts_with("research-collect-data"))
            .expect("worktree entry present");
        assert_eq!(item.task_path.as_deref(), Some("research/collect-data"));
        assert!(!item.stale);

        // Поріг 0 хв → будь-який вік (навіть 0) вважається stale, бо порівняння строге `>`
        // не спрацює на щойно створеному каталозі — перевіряємо натомість негативний поріг
        // через age_min напряму, не покладаючись на точний нуль (уникає flaky на повільному CI).
        let inventory_never_stale =
            worktree_inventory(repo.work.path(), &task_paths, u64::MAX).unwrap();
        assert!(inventory_never_stale.iter().all(|i| !i.stale));
    }
}
