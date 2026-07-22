//! Герметичні git-фікстури для тестів `claims`/`publish` — bare-репозиторій
//! як "origin" + звичайний клон з одним комітом на `main`. Тільки для тестів
//! (`#[cfg(test)]`), нуль впливу на реальний runtime.
#![cfg(test)]

use std::path::Path;
use std::process::Command;

/// Запускає git-команду в `dir`, панікує з stderr при ненульовому exit-коді.
pub fn run(dir: &Path, args: &[&str]) {
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

/// Як [`run`], але повертає trimmed stdout.
pub fn output(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("git {args:?}: {e}"));
    if !out.status.success() {
        panic!(
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// Bare "origin" + звичайний клон із одним комітом на `main`, віддалений
/// `origin` уже додано і запушено. Робочий клон — контекст для plumbing-команд
/// (claim-коміти пишуться без touching робочого дерева/індексу).
pub struct TestRepo {
    /// Тримає bare-репозиторій живим на диску (remote URL — file-шлях);
    /// поле не читається напряму після конструктора, лише продовжує TempDir.
    #[allow(dead_code)]
    pub origin: tempfile::TempDir,
    pub work: tempfile::TempDir,
}

impl TestRepo {
    pub fn new() -> Self {
        let origin = tempfile::tempdir().unwrap();
        run(origin.path(), &["init", "--bare", "-q", "-b", "main"]);

        let work = tempfile::tempdir().unwrap();
        run(work.path(), &["init", "-q", "-b", "main"]);
        std::fs::write(work.path().join("README.md"), "x").unwrap();
        run(work.path(), &["add", "."]);
        run(work.path(), &["commit", "-q", "-m", "init"]);
        run(
            work.path(),
            &["remote", "add", "origin", origin.path().to_str().unwrap()],
        );
        run(work.path(), &["push", "-q", "origin", "main"]);

        Self { origin, work }
    }

    pub fn main_sha(&self) -> String {
        output(self.work.path(), &["rev-parse", "main"])
    }
}
