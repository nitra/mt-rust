//! Помилки native Git facade.

use std::fmt::{Display, Formatter};

/// Помилка відкриття репозиторію або native Git операції.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitError {
    message: String,
}

impl GitError {
    pub(crate) fn from_error(error: impl Display) -> Self {
        Self {
            message: error.to_string(),
        }
    }
}

impl Display for GitError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for GitError {}
