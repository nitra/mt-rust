//! Іменування, матчінг і provisioning worktree для задач (порт чистої частини
//! `npm/lib/core/worktree.mjs` + git-операції run-wrapper-а, спека «Wrapper-
//! скрипт», крок 5: detached worktree від зафіксованого `base_sha`).

use std::path::{Path, PathBuf};
use std::process::Command;

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
    git(
        repo_root,
        &["worktree", "remove", "--force", &path.to_string_lossy()],
    )?;
    Ok(())
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
}
