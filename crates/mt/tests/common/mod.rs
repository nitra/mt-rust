//! Мінімальна git-фікстура для black-box e2e тестів `mt`-бінарника — bare
//! "origin" + звичайний клон з одним комітом на `main`. Аналог
//! `mt_core::test_support::TestRepo`, продубльований тут навмисно: той
//! модуль — `#[cfg(test)]`-приватний усередині `mt-core` і недоступний
//! іншим крейтам навіть у тестових білдах.

use std::path::{Path, PathBuf};
use std::process::Command;

pub fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_AUTHOR_NAME", "test")
        .env("GIT_AUTHOR_EMAIL", "test@test.local")
        .env("GIT_COMMITTER_NAME", "test")
        .env("GIT_COMMITTER_EMAIL", "test@test.local")
        .output()
        .unwrap_or_else(|e| panic!("git {args:?}: {e}"));
    if !out.status.success() {
        panic!(
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

pub struct TestRepo {
    #[allow(dead_code)]
    pub origin: tempfile::TempDir,
    pub work: tempfile::TempDir,
}

impl TestRepo {
    pub fn new() -> Self {
        let origin = tempfile::tempdir().unwrap();
        git(origin.path(), &["init", "--bare", "-q", "-b", "main"]);

        let work = tempfile::tempdir().unwrap();
        git(work.path(), &["init", "-q", "-b", "main"]);
        std::fs::write(work.path().join("README.md"), "x").unwrap();
        git(work.path(), &["add", "."]);
        git(work.path(), &["commit", "-q", "-m", "init"]);
        git(
            work.path(),
            &["remote", "add", "origin", origin.path().to_str().unwrap()],
        );
        git(work.path(), &["push", "-q", "origin", "main"]);

        Self { origin, work }
    }
}

/// Шлях до скомпільованого бінарника `mt` цього тестового прогону.
pub fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_mt"))
}

pub fn run(repo: &Path, args: &[&str]) -> std::process::Output {
    Command::new(bin())
        .current_dir(repo)
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("mt {args:?}: {e}"))
}

pub fn stdout(out: &std::process::Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}
