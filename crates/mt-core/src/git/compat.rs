//! Вузька межа Git porcelain для операцій, яких немає у pinned `gix` 0.86.
//!
//! Дозволені capability: staging/commit, custom-ref CAS, worktree lifecycle,
//! branch config, rebase, atomic publish і локальний fast-forward. Це не
//! загальний API для виконання довільних Git-команд.

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

/// Виконує allow-listed worktree lifecycle porcelain (`add`, `remove`, `list`, `prune`).
pub fn worktree(repo: &Path, args: &[&str]) -> Result<String, GitError> {
    if !matches!(args.first(), Some(&"add" | &"remove" | &"list" | &"prune")) {
        return Err(GitError::from_error(
            "unsupported compat worktree operation",
        ));
    }
    output(repo, "worktree", args)
}

/// Видаляє гілку після успішного видалення її worktree.
pub fn delete_branch(repo: &Path, branch: &str) -> Result<(), GitError> {
    run(repo, ["branch", "-D", branch])
}

/// Зберігає локальний опис developer branch.
pub fn set_branch_description(
    repo: &Path,
    branch: &str,
    description: &str,
) -> Result<(), GitError> {
    run(
        repo,
        [
            "config",
            "--local",
            &format!("branch.{branch}.description"),
            description,
        ],
    )
}

/// Читає локальний опис developer branch.
pub fn branch_description(repo: &Path, branch: &str) -> Result<Option<String>, GitError> {
    let key = format!("branch.{branch}.description");
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["config", "--local", "--get", &key])
        .output()
        .map_err(|error| GitError::from_error(format!("git config: {error}")))?;
    if out.status.success() {
        return Ok(Some(
            String::from_utf8_lossy(&out.stdout).trim().to_string(),
        ));
    }
    if out.status.code() == Some(1) {
        return Ok(None);
    }
    Err(GitError::from_error(format!(
        "git config: {}",
        String::from_utf8_lossy(&out.stderr).trim()
    )))
}

/// Rebase-ить worktree на задану базу та прибирає незавершений rebase при помилці.
pub fn rebase_onto(repo: &Path, base: &str) -> Result<(), GitError> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["rebase", base])
        .output()
        .map_err(|error| GitError::from_error(format!("git rebase: {error}")))?;
    if out.status.success() {
        return Ok(());
    }
    let message = String::from_utf8_lossy(&out.stderr).trim().to_string();
    let _ = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["rebase", "--abort"])
        .output();
    Err(GitError::from_error(format!("git rebase: {message}")))
}

/// Виконує fenced atomic push; `false` означає очікувану lease rejection.
pub fn push_atomic(repo: &Path, args: &[&str]) -> Result<bool, GitError> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .map_err(|error| GitError::from_error(format!("git push --atomic: {error}")))?;
    if out.status.success() {
        return Ok(true);
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    if is_lease_rejection(&stderr) {
        return Ok(false);
    }
    Err(GitError::from_error(format!(
        "git push --atomic: {}",
        stderr.trim()
    )))
}

/// Best-effort fast-forward локальної `main` після успішного publish.
pub fn fast_forward_main(repo: &Path, result_sha: &str) -> Result<(), GitError> {
    let branch = output(repo, "rev-parse", &["--abbrev-ref", "HEAD"])?;
    if branch != "main" {
        return Ok(());
    }
    let _ = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["merge", "--ff-only", result_sha])
        .output()
        .map_err(|error| GitError::from_error(format!("git merge --ff-only: {error}")))?;
    Ok(())
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

fn output(repo: &Path, command: &str, args: &[&str]) -> Result<String, GitError> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .arg(command)
        .args(args)
        .output()
        .map_err(|error| GitError::from_error(format!("git {command}: {error}")))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(GitError::from_error(format!(
            "git {command}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )))
    }
}
