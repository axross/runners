//! where the agent keeps each repository's token: an interface that names no
//! operating system, an in-memory store for tests, and an adapter over a
//! `keyring-core` store, which the Windows Credential Manager provides and the Mac's
//! Keychain store can later provide; the agent is the only caller, one call at a
//! time; `docs/operations/cli.md` describes the keys and the Windows entries.

mod keyring;
mod memory;
#[cfg(windows)]
pub mod windows;

use std::fmt;

use crate::runtime::secret::Secret;
use crate::validate::patterns::{is_owner, is_repository};

pub use keyring::{EntryName, KeyringTokenStore, Naming};
pub use memory::MemoryTokenStore;

/// the most characters a token may have: the Credential Manager keeps a password in
/// 2560 bytes of UTF-16.
pub const MAX_TOKEN_CHARS: usize = 1280;

/// a repository as the store names it: `owner/name` in lower case, the way the
/// configuration's checks compare an owner and a repository.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RepositoryKey(String);

/// why a text is not a repository key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotAKey;

impl fmt::Display for NotAKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a repository is written owner/name")
    }
}

impl std::error::Error for NotAKey {}

impl RepositoryKey {
    /// the key of `owner` and `name`, which must already be valid in a
    /// configuration.
    #[must_use]
    pub fn of(owner: &str, name: &str) -> Self {
        Self(format!("{owner}/{name}").to_ascii_lowercase())
    }

    /// reads `owner/name`, in any case, with the patterns a configuration applies to
    /// each part.
    pub fn parse(text: &str) -> Result<Self, NotAKey> {
        let (owner, name) = text.split_once('/').ok_or(NotAKey)?;
        if is_owner(owner) && is_repository(name) {
            Ok(Self::of(owner, name))
        } else {
            Err(NotAKey)
        }
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RepositoryKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// why the store could not do what was asked; the text never holds a token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreError(String);

impl StoreError {
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for StoreError {}

/// reads, sets and removes a token by repository.
pub trait TokenStore: Send + Sync {
    /// the repository's token, or `None` when it has none.
    fn get(&self, key: &RepositoryKey) -> Result<Option<Secret>, StoreError>;

    /// stores the token, replacing any.
    fn set(&self, key: &RepositoryKey, token: &Secret) -> Result<(), StoreError>;

    /// removes the token; a repository without one is not an error.
    fn remove(&self, key: &RepositoryKey) -> Result<(), StoreError>;
}

/// why a token is refused before it reaches the store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenRefusal {
    Empty,
    TooLong,
}

impl fmt::Display for TokenRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("the token is empty"),
            Self::TooLong => write!(f, "the token is longer than {MAX_TOKEN_CHARS} characters"),
        }
    }
}

impl std::error::Error for TokenRefusal {}

/// the token as the store keeps it: white space around it removed, then refused when
/// it is empty or longer than [`MAX_TOKEN_CHARS`].
pub fn accepted_token(text: &str) -> Result<Secret, TokenRefusal> {
    let token = text.trim();
    if token.is_empty() {
        Err(TokenRefusal::Empty)
    } else if token.chars().count() > MAX_TOKEN_CHARS {
        Err(TokenRefusal::TooLong)
    } else {
        Ok(Secret::new(token.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_is_the_owner_and_name_in_lower_case() {
        let key = RepositoryKey::parse("Example-Owner/Repo_One").unwrap();
        assert_eq!(key.as_str(), "example-owner/repo_one");
        assert_eq!(key, RepositoryKey::of("EXAMPLE-owner", "repo_ONE"));
    }

    #[test]
    fn a_text_that_is_not_an_owner_and_a_name_is_not_a_key() {
        for text in [
            "",
            "owner",
            "owner/",
            "/name",
            "a/b/c",
            "o wner/name",
            "o/..",
            "../x",
        ] {
            assert_eq!(RepositoryKey::parse(text), Err(NotAKey), "{text:?}");
        }
    }

    #[test]
    fn a_token_loses_the_white_space_around_it() {
        assert_eq!(
            accepted_token("  ghp_fake \r\n").unwrap().expose(),
            "ghp_fake"
        );
    }

    #[test]
    fn an_empty_or_oversize_token_is_refused_and_the_limit_counts_characters() {
        assert_eq!(accepted_token(" \t ").unwrap_err(), TokenRefusal::Empty);
        assert!(accepted_token(&"a".repeat(MAX_TOKEN_CHARS)).is_ok());
        assert_eq!(
            accepted_token(&"a".repeat(MAX_TOKEN_CHARS + 1)).unwrap_err(),
            TokenRefusal::TooLong
        );
        assert!(accepted_token(&"é".repeat(MAX_TOKEN_CHARS)).is_ok());
    }
}
