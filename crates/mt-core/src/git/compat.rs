//! Тимчасова межа Git porcelain для операцій, яких немає у pinned `gix`.
//!
//! Дозволені лише CAS push/delete custom refs. Worktree lifecycle, rebase та
//! atomic publish додаватимуться тут окремими contract-tested операціями.

use std::{path::Path, process::Command};

use super::{GitError, SignaturePolicy};

/// Додає усі зміни до index і створює commit за заданою identity policy.
/// Повертає `false`, коли worktree чистий після staging.
pub fn commit_all_if_changed(
    repo: &Path,
    message: &str,
    signature: SignaturePolicy,
) -> Result<bool, GitError> {
    run(repo, ["add", "-A"])?;
    commit_staged_if_changed(repo, message, signature)
}

/// Створює commit з уже staged index змінами.
pub fn commit_staged_if_changed(
    repo: &Path,
    message: &str,
    signature: SignaturePolicy,
) -> Result<bool, GitError> {
    let staged = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["diff", "--cached", "--quiet"])
        .status()
        .map_err(|error| GitError::from_error(format!("git diff --cached: {error}")))?;
    if staged.success() {
        return Ok(false);
    }
    if staged.code() != Some(1) {
        return Err(GitError::from_error("git diff --cached failed"));
    }

    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(repo)
        .args(["commit", "-q", "-m", message]);
    if signature == SignaturePolicy::Runner {
        command
            .env("GIT_AUTHOR_NAME", "mt-runner")
            .env("GIT_AUTHOR_EMAIL", "mt-runner@localhost")
            .env("GIT_COMMITTER_NAME", "mt-runner")
            .env("GIT_COMMITTER_EMAIL", "mt-runner@localhost");
    }
    let out = command
        .output()
        .map_err(|error| GitError::from_error(format!("git commit: {error}")))?;
    if out.status.success() {
        Ok(true)
    } else {
        Err(GitError::from_error(format!(
            "git commit: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )))
    }
}

/// Прибирає шлях лише з index, не видаляючи його з worktree.
pub fn remove_from_index(repo: &Path, path: &str) -> Result<bool, GitError> {
    run(
        repo,
        ["rm", "-r", "-q", "--cached", "--ignore-unmatch", path],
    )?;
    let staged = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["diff", "--cached", "--quiet"])
        .status()
        .map_err(|error| GitError::from_error(format!("git diff --cached: {error}")))?;
    Ok(staged.code() == Some(1))
}

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

fn run<const N: usize>(repo: &Path, args: [&str; N]) -> Result<(), GitError> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .map_err(|error| GitError::from_error(format!("git: {error}")))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(GitError::from_error(format!(
            "git: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )))
    }
}
