//! Єдина межа взаємодії `mt-core` з Git.

mod error;
mod refs;

use std::path::Path;

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

    /// Повертає native `gix` handle тільки всередині `mt-core`.
    #[allow(dead_code)] // Публічні native-операції додаються поетапно.
    pub(crate) fn inner(&self) -> &gix::Repository {
        &self.repo
    }
}
