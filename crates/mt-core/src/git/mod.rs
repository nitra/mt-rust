//! Єдина межа взаємодії `mt-core` з Git.

mod error;
mod refs;

use std::path::{Path, PathBuf};

pub use error::GitError;
pub use refs::{ClaimRef, RunRef};

/// Відкритий Git-репозиторій для наступних native `gix` операцій.
pub struct GitRepository {
    #[allow(dead_code)] // Наступні facade-операції Task 2 споживатимуть handle.
    repo: gix::Repository,
}

impl GitRepository {
    /// Відкриває репозиторій, якому належить переданий шлях.
    pub fn open(path: &Path) -> Result<Self, GitError> {
        let repo = gix::discover(path).map_err(GitError::from_error)?;
        Ok(Self { repo })
    }

    /// Повертає абсолютний шлях робочого дерева репозиторію.
    pub fn repo_root(&self) -> Result<PathBuf, GitError> {
        self.repo
            .workdir()
            .map(Path::to_path_buf)
            .ok_or_else(|| GitError::from_error("bare repositories have no worktree"))
    }

    /// Повертає fetch URL віддаленого `origin`, якщо він налаштований.
    pub fn origin_url(&self) -> Result<Option<String>, GitError> {
        let remote = match self.repo.try_find_remote("origin") {
            Some(Ok(remote)) => remote,
            None => return Ok(None),
            Some(Err(error)) => return Err(GitError::from_error(error)),
        };

        Ok(remote
            .url(gix::remote::Direction::Fetch)
            .map(ToString::to_string))
    }

    /// Повертає object ID, на який вказує повний ref.
    pub fn resolve_ref(&self, reference: &str) -> Result<String, GitError> {
        self.repo
            .find_reference(reference)
            .map_err(GitError::from_error)
            .map(|reference| reference.id().to_string())
    }

    /// Читає blob за шляхом у дереві заданого коміту.
    pub fn read_blob_at_commit(&self, commit: &str, path: &str) -> Result<Vec<u8>, GitError> {
        let commit = gix::ObjectId::from_hex(commit.as_bytes()).map_err(GitError::from_error)?;
        let tree = self
            .repo
            .find_commit(commit)
            .map_err(GitError::from_error)?
            .tree()
            .map_err(GitError::from_error)?;
        let entry = tree
            .lookup_entry(path.split('/'))
            .map_err(GitError::from_error)?
            .ok_or_else(|| GitError::from_error(format!("path not found in commit: {path}")))?;

        entry
            .object()
            .map_err(GitError::from_error)?
            .try_into_blob()
            .map_err(GitError::from_error)
            .map(|mut blob| blob.take_data())
    }

    /// Повертає імена головного та всіх доступних linked worktree.
    pub fn linked_worktrees(&self) -> Result<Vec<String>, GitError> {
        let main_repo = self.repo.main_repo().map_err(GitError::from_error)?;
        let mut worktrees = main_repo
            .workdir()
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
            .map(String::from)
            .into_iter()
            .collect::<Vec<_>>();

        for worktree in main_repo.worktrees().map_err(GitError::from_error)? {
            let path = worktree.base().map_err(GitError::from_error)?;
            if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
                worktrees.push(name.to_string());
            }
        }
        Ok(worktrees)
    }

    /// Повертає native `gix` handle тільки всередині `mt-core`.
    #[allow(dead_code)] // Публічні native-операції додаються поетапно.
    pub(crate) fn inner(&self) -> &gix::Repository {
        &self.repo
    }
}

#[cfg(test)]
mod tests {
    use super::GitRepository;
    use crate::test_support::{run, TestRepo};

    #[test]
    fn discovers_root_origin_ref_and_blob_without_git_cli() {
        let fixture = TestRepo::new();
        let repository = GitRepository::open(fixture.work.path()).unwrap();
        let main = fixture.main_sha();

        assert_eq!(repository.repo_root().unwrap(), fixture.work.path());
        assert!(repository.origin_url().unwrap().is_some());
        assert_eq!(repository.resolve_ref("refs/heads/main").unwrap(), main);
        assert_eq!(
            repository.read_blob_at_commit(&main, "README.md").unwrap(),
            b"x"
        );
    }

    #[test]
    fn origin_is_none_when_no_remote_is_configured() {
        let fixture = tempfile::tempdir().unwrap();
        run(fixture.path(), &["init", "-q", "-b", "main"]);

        let repository = GitRepository::open(fixture.path()).unwrap();

        assert_eq!(repository.origin_url().unwrap(), None);
    }

    #[test]
    fn linked_worktrees_include_the_main_worktree() {
        let fixture = TestRepo::new();
        let repository = GitRepository::open(fixture.work.path()).unwrap();

        assert_eq!(repository.linked_worktrees().unwrap().len(), 1);
        assert!(repository.linked_worktrees().unwrap().contains(
            &fixture
                .work
                .path()
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .to_string()
        ));
    }
}
