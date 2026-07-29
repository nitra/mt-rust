//! Єдина межа взаємодії `mt-core` з Git.

mod error;
mod refs;

/// Вузькі Git CLI capability, яких pinned `gix` ще не надає.
pub mod compat;

use std::{
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};

pub use error::GitError;
pub use refs::{ClaimRef, RunRef};

/// Policy identity для commit-ів, створених runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignaturePolicy {
    /// Детермінований identity автономного runner-а.
    Runner,
    /// Identity з Git config активного worktree.
    Configured,
}

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

    /// Комітить усі зміни worktree або повертає `None`, якщо змін немає.
    pub fn commit_all_if_changed(
        &self,
        message: &str,
        signature: SignaturePolicy,
    ) -> Result<Option<String>, GitError> {
        let worktree = self.repo_root()?;
        if !compat::commit_all_if_changed(&worktree, message, signature)? {
            return Ok(None);
        }
        self.repo
            .head_id()
            .map_err(GitError::from_error)
            .map(|id| Some(id.to_string()))
    }

    /// Прибирає шлях лише з index, залишаючи його у worktree.
    pub fn remove_from_index(&self, path: &str) -> Result<bool, GitError> {
        compat::remove_from_index(&self.repo_root()?, path)
    }

    /// Комітить вже staged зміни або повертає `None`, якщо index чистий.
    pub fn commit_staged_if_changed(
        &self,
        message: &str,
        signature: SignaturePolicy,
    ) -> Result<Option<String>, GitError> {
        let worktree = self.repo_root()?;
        if !compat::commit_staged_if_changed(&worktree, message, signature)? {
            return Ok(None);
        }
        self.repo
            .head_id()
            .map_err(GitError::from_error)
            .map(|id| Some(id.to_string()))
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

    /// Повертає повне ім'я поточної гілки або `None` для detached `HEAD`.
    pub fn head_ref_name(&self) -> Result<Option<String>, GitError> {
        self.repo
            .head_name()
            .map_err(GitError::from_error)
            .map(|name| name.map(|name| name.to_string()))
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

    /// Створює commit з одним `.mt-claim.yml` без checkout або index.
    pub fn write_claim_commit(
        &self,
        parent: &str,
        yaml: &str,
        message: &str,
    ) -> Result<String, GitError> {
        let blob = self.repo.write_blob(yaml).map_err(GitError::from_error)?;
        let tree = gix::objs::Tree {
            entries: vec![gix::objs::tree::Entry {
                mode: gix::objs::tree::EntryKind::Blob.into(),
                filename: ".mt-claim.yml".into(),
                oid: blob.detach(),
            }],
        };
        let tree = self.repo.write_object(tree).map_err(GitError::from_error)?;
        let parent = gix::ObjectId::from_hex(parent.as_bytes()).map_err(GitError::from_error)?;
        let signature = gix::actor::SignatureRef {
            name: b"mt".as_slice().into(),
            email: b"mt@localhost".as_slice().into(),
            time: "0 +0000",
        };
        let commit = self
            .repo
            .new_commit_as(signature, signature, message, tree, [parent])
            .map_err(GitError::from_error)?;
        Ok(commit.id.to_string())
    }

    /// Завантажує custom claim refs з `origin` і повертає їх advertised object IDs.
    pub fn fetch_claim_refs(&self) -> Result<Vec<(String, String)>, GitError> {
        let refspec_text = "+refs/mt/claims/*:refs/mt/claims/*";
        let refspec =
            gix::refspec::parse(refspec_text.into(), gix::refspec::parse::Operation::Fetch)
                .map_err(GitError::from_error)?
                .to_owned();
        let remote = self
            .repo
            .find_remote("origin")
            .map_err(GitError::from_error)?;
        let mut options = gix::remote::ref_map::Options::default();
        options.extra_refspecs.push(refspec);
        let connection = remote
            .connect(gix::remote::Direction::Fetch)
            .map_err(GitError::from_error)?;
        let prepared = connection
            .prepare_fetch(gix::progress::Discard, options)
            .map_err(GitError::from_error)?;
        let refs = prepared
            .ref_map()
            .remote_refs
            .iter()
            .filter_map(|reference| match reference {
                gix::protocol::handshake::Ref::Direct {
                    full_ref_name,
                    object,
                } => {
                    let name = String::from_utf8_lossy(full_ref_name.as_ref()).into_owned();
                    name.strip_prefix("refs/mt/claims/")
                        .filter(|name| !name.is_empty() && !name.contains('/'))
                        .map(|name| (name.to_string(), object.to_string()))
                }
                _ => None,
            })
            .collect::<Vec<_>>();

        if refs.is_empty() {
            return Ok(refs);
        }
        prepared
            .receive(gix::progress::Discard, &AtomicBool::new(false))
            .map_err(GitError::from_error)?;
        Ok(refs)
    }

    /// Завантажує exact remote ref у вказаний локальний ref через native gix transport.
    pub fn fetch_refspec(&self, refspec_text: &str) -> Result<(), GitError> {
        let refspec =
            gix::refspec::parse(refspec_text.into(), gix::refspec::parse::Operation::Fetch)
                .map_err(GitError::from_error)?
                .to_owned();
        let remote = self
            .repo
            .find_remote("origin")
            .map_err(GitError::from_error)?;
        let mut options = gix::remote::ref_map::Options::default();
        options.extra_refspecs.push(refspec);
        let connection = remote
            .connect(gix::remote::Direction::Fetch)
            .map_err(GitError::from_error)?;
        let prepared = connection
            .prepare_fetch(gix::progress::Discard, options)
            .map_err(GitError::from_error)?;
        prepared
            .receive(gix::progress::Discard, &AtomicBool::new(false))
            .map_err(GitError::from_error)?;
        Ok(())
    }

    /// Перевіряє, чи remote `origin` рекламує точний ref, без запису локального ref.
    pub fn remote_has_ref(&self, reference: &str) -> Result<bool, GitError> {
        let refspec_text = format!("+{reference}:{reference}");
        let refspec = gix::refspec::parse(
            refspec_text.as_str().into(),
            gix::refspec::parse::Operation::Fetch,
        )
        .map_err(GitError::from_error)?
        .to_owned();
        let remote = self
            .repo
            .find_remote("origin")
            .map_err(GitError::from_error)?;
        let mut options = gix::remote::ref_map::Options::default();
        options.extra_refspecs.push(refspec);
        let connection = remote
            .connect(gix::remote::Direction::Fetch)
            .map_err(GitError::from_error)?;
        let prepared = connection
            .prepare_fetch(gix::progress::Discard, options)
            .map_err(GitError::from_error)?;
        Ok(prepared
            .ref_map()
            .remote_refs
            .iter()
            .any(|remote_ref| match remote_ref {
                gix::protocol::handshake::Ref::Direct { full_ref_name, .. } => {
                    String::from_utf8_lossy(&full_ref_name[..]) == reference
                }
                _ => false,
            }))
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
    use crate::test_support::TestRepo;

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
        gix::init(fixture.path()).unwrap();

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
