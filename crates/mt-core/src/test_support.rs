//! Герметичні git-фікстури для тестів `claims`/`publish` — bare-репозиторій
//! як "origin" + звичайний клон з одним комітом на `main`. Тільки для тестів
//! (`#[cfg(test)]`), нуль впливу на реальний runtime.
#![cfg(test)]

use std::{io::Write, path::Path};

use crate::git::{compat, GitRepository, SignaturePolicy};

/// Комітить усі поточні зміни з детермінованою test identity.
pub fn commit_all(dir: &Path, message: &str) {
    GitRepository::open(dir)
        .unwrap()
        .commit_all_if_changed(message, SignaturePolicy::Runner)
        .unwrap();
}

/// Публікує поточний `HEAD` у `origin/<refname>` через вузький compat API.
pub fn push_head(dir: &Path, refname: &str) {
    compat::push_head(dir, refname).unwrap();
}

/// Повертає SHA поточного `HEAD`.
pub fn head(dir: &Path) -> String {
    GitRepository::open(dir)
        .unwrap()
        .resolve_ref("HEAD")
        .unwrap()
}

/// Повертає повну назву гілки або `None` для detached `HEAD`.
pub fn head_branch(dir: &Path) -> Option<String> {
    GitRepository::open(dir).unwrap().head_ref_name().unwrap()
}

/// Повертає SHA першого parent commit-а.
pub fn first_parent(dir: &Path, commit: &str) -> String {
    let repository = GitRepository::open(dir).unwrap();
    let commit = repository
        .resolve_ref(commit)
        .unwrap_or_else(|_| commit.into());
    let commit = gix::ObjectId::from_hex(commit.as_bytes()).unwrap();
    let parent = repository
        .inner()
        .find_commit(commit)
        .unwrap()
        .parent_ids()
        .next()
        .unwrap()
        .to_string();
    parent
}

/// Читає blob із заданого commit/ref без porcelain `git show`.
#[allow(dead_code)]
pub fn blob(dir: &Path, commit: &str, path: &str) -> String {
    let repository = GitRepository::open(dir).unwrap();
    let commit = repository
        .resolve_ref(commit)
        .unwrap_or_else(|_| commit.into());
    String::from_utf8(repository.read_blob_at_commit(&commit, path).unwrap()).unwrap()
}

/// Перевіряє наявність remote ref через native advertisement, без local cache.
pub fn remote_ref_exists(dir: &Path, reference: &str) -> bool {
    GitRepository::open(dir)
        .unwrap()
        .remote_has_ref(reference)
        .unwrap()
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
        gix::init_bare(origin.path()).unwrap();
        std::fs::write(origin.path().join("HEAD"), "ref: refs/heads/main\n").unwrap();

        let work = tempfile::tempdir().unwrap();
        gix::init(work.path()).unwrap();
        std::fs::write(work.path().join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
        let config = work.path().join(".git/config");
        std::fs::OpenOptions::new()
            .append(true)
            .open(&config)
            .unwrap()
            .write_all(
                format!(
                    "\n[remote \"origin\"]\n\turl = {}\n\tfetch = +refs/heads/*:refs/remotes/origin/*\n",
                    origin.path().display()
                )
                .as_bytes(),
            )
            .unwrap();
        std::fs::write(work.path().join("README.md"), "x").unwrap();
        commit_all(work.path(), "init");
        push_head(work.path(), "refs/heads/main");

        Self { origin, work }
    }

    pub fn main_sha(&self) -> String {
        GitRepository::open(self.work.path())
            .unwrap()
            .resolve_ref("refs/heads/main")
            .unwrap()
    }
}
