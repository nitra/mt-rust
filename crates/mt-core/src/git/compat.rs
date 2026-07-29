//! Тимчасова межа Git porcelain для операцій, яких немає у pinned `gix`.
//!
//! Дозволені лише CAS push/delete custom refs. Worktree lifecycle, rebase та
//! atomic publish додаватимуться тут окремими contract-tested операціями.

use std::{path::Path, process::Command};

use super::GitError;

/// Публікує `new_target` у `refname` лише за очікуваного remote target.
/// `None` означає create-only ref.
pub fn push_with_expected(
    repo: &Path,
    refname: &str,
    new_target: &str,
    expected_target: Option<&str>,
) -> Result<bool, GitError> {
    let lease = format!(
        "--force-with-lease={refname}:{}",
        expected_target.unwrap_or("")
    );
    let refspec = format!("{new_target}:{refname}");
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["push", &lease, "origin", &refspec])
        .output()
        .map_err(|error| GitError::from_error(format!("git push: {error}")))?;
    if out.status.success() {
        return Ok(true);
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    if is_lease_rejection(&stderr) {
        return Ok(false);
    }
    Err(GitError::from_error(format!("git push: {}", stderr.trim())))
}

/// Видаляє custom ref лише якщо remote ще вказує на `expected_target`.
pub fn delete_with_expected(
    repo: &Path,
    refname: &str,
    expected_target: &str,
) -> Result<bool, GitError> {
    let lease = format!("--force-with-lease={refname}:{expected_target}");
    let refspec = format!(":{refname}");
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["push", &lease, "origin", &refspec])
        .output()
        .map_err(|error| GitError::from_error(format!("git push --delete: {error}")))?;
    if out.status.success() {
        return Ok(true);
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    if is_lease_rejection(&stderr) {
        return Ok(false);
    }
    Err(GitError::from_error(format!(
        "git push --delete: {}",
        stderr.trim()
    )))
}

/// Публікує поточний `HEAD` у custom ref без CAS, для run checkpoint.
pub fn push_head(repo: &Path, refname: &str) -> Result<(), GitError> {
    let refspec = format!("HEAD:{refname}");
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["push", "origin", &refspec])
        .output()
        .map_err(|error| GitError::from_error(format!("git push: {error}")))?;
    if out.status.success() {
        return Ok(());
    }
    Err(GitError::from_error(format!(
        "git push: {}",
        String::from_utf8_lossy(&out.stderr).trim()
    )))
}

fn is_lease_rejection(stderr: &str) -> bool {
    stderr.contains("stale info")
        || stderr.contains("[rejected]")
        || stderr.contains("already exists")
        || stderr.contains("fetch first")
}
