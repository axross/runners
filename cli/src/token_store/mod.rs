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

pub use keyring::{Classified, EntryName, KeyringTokenStore, Naming};
pub use memory::MemoryTokenStore;

/// the most UTF-16 code units a token may have, so an accepted token always fits the
/// 2560 bytes the Credential Manager keeps a password in.
pub const MAX_TOKEN_UTF16_UNITS: usize = 1280;

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

/// what a store failure asks of the caller, the one thing every platform's failures
/// have in common; the set may grow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreErrorKind {
    /// the store needs the user, such as a locked keychain that the user can unlock.
    NeedsUser,
    /// the item refuses access to this program.
    AccessRefused,
    Other,
}

impl StoreErrorKind {
    /// the word the agent names the kind with.
    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Self::NeedsUser => "needs_user",
            Self::AccessRefused => "access_refused",
            Self::Other => "other",
        }
    }
}

/// why the store could not do what was asked: a kind, the platform's own code when it
/// gave one, and words; none of them ever holds a token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreError {
    kind: StoreErrorKind,
    code: Option<i64>,
    message: String,
}

impl StoreError {
    /// a failure of kind [`StoreErrorKind::Other`] with no code.
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self::with(StoreErrorKind::Other, None, message)
    }

    #[must_use]
    pub fn with(kind: StoreErrorKind, code: Option<i64>, message: impl Into<String>) -> Self {
        Self {
            kind,
            code,
            message: message.into(),
        }
    }

    #[must_use]
    pub fn kind(&self) -> StoreErrorKind {
        self.kind
    }

    /// the platform's code, such as a Windows error number.
    #[must_use]
    pub fn code(&self) -> Option<i64> {
        self.code
    }

    /// the words, without the kind and the code.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// the words with the kind and the code, as the agent shows a failure.
    #[must_use]
    pub fn describe(&self) -> String {
        match self.code {
            Some(code) => format!("{} (kind {}, code {code})", self.message, self.kind.word()),
            None => format!("{} (kind {})", self.message, self.kind.word()),
        }
    }
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
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
            Self::TooLong => write!(
                f,
                "the token is longer than {MAX_TOKEN_UTF16_UNITS} UTF-16 code units"
            ),
        }
    }
}

impl std::error::Error for TokenRefusal {}

/// the token as the store keeps it: white space around it removed, then refused when
/// it is empty or longer than [`MAX_TOKEN_UTF16_UNITS`].
pub fn accepted_token(text: &str) -> Result<Secret, TokenRefusal> {
    let token = text.trim();
    if token.is_empty() {
        Err(TokenRefusal::Empty)
    } else if token.encode_utf16().count() > MAX_TOKEN_UTF16_UNITS {
        Err(TokenRefusal::TooLong)
    } else {
        Ok(Secret::new(token.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failure_is_described_with_its_kind_and_code() {
        let error = StoreError::with(StoreErrorKind::AccessRefused, Some(1312), "no");
        assert_eq!(error.describe(), "no (kind access_refused, code 1312)");
        assert_eq!(StoreError::new("x").describe(), "x (kind other)");
        assert_eq!(error.to_string(), "no");
    }

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
    fn an_empty_or_oversize_token_is_refused_and_the_limit_counts_utf16_units() {
        assert_eq!(accepted_token(" \t ").unwrap_err(), TokenRefusal::Empty);
        assert!(accepted_token(&"a".repeat(MAX_TOKEN_UTF16_UNITS)).is_ok());
        assert_eq!(
            accepted_token(&"a".repeat(MAX_TOKEN_UTF16_UNITS + 1)).unwrap_err(),
            TokenRefusal::TooLong
        );
        assert!(accepted_token(&"é".repeat(MAX_TOKEN_UTF16_UNITS)).is_ok());
        // a character outside the basic plane takes two units
        let pairs = MAX_TOKEN_UTF16_UNITS / 2;
        assert!(accepted_token(&"😀".repeat(pairs)).is_ok());
        assert_eq!(
            accepted_token(&"😀".repeat(pairs + 1)).unwrap_err(),
            TokenRefusal::TooLong
        );
    }
}
