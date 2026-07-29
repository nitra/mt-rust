//! Типізовані custom refs протоколу виконання `mt`.

use super::GitError;

const CLAIM_PREFIX: &str = "refs/mt/claims/";
const RUN_PREFIX: &str = "refs/mt/runs/";

/// Custom ref, що серіалізує володіння вузлом задачі.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimRef(String);

impl ClaimRef {
    /// Створює claim ref лише з lower-hex node hash.
    pub fn new(node_hash: &str) -> Result<Self, GitError> {
        if node_hash.is_empty()
            || !node_hash
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        {
            return Err(invalid_ref_component("node hash"));
        }
        Ok(Self(format!("{CLAIM_PREFIX}{node_hash}")))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Custom ref, що зберігає checkpoint окремого запуску.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunRef(String);

impl RunRef {
    /// Створює run ref для валідного node hash і непрозорого token без `/`.
    pub fn new(node_hash: &str, token: &str) -> Result<Self, GitError> {
        let claim = ClaimRef::new(node_hash)?;
        if token.is_empty()
            || token.contains('/')
            || token.bytes().any(|byte| byte.is_ascii_control())
        {
            return Err(invalid_ref_component("run token"));
        }
        let node_hash = claim
            .as_str()
            .strip_prefix(CLAIM_PREFIX)
            .expect("known prefix");
        Ok(Self(format!("{RUN_PREFIX}{node_hash}/{token}")))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn invalid_ref_component(name: &str) -> GitError {
    GitError::from_error(format!("invalid {name}"))
}
