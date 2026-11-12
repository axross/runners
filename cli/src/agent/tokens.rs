//! the agent's tokens: every pool's registration reads its repository's token from
//! the store, and a request changes it; a token file is never read, and no text
//! this module makes holds a token.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use tokio::sync::watch;

use crate::runtime::secret::Secret;
use crate::runtime::token::TokenSource;
use crate::token_store::{RepositoryKey, StoreError, TokenStore};

/// why a pool has no token to start with; the snapshot and the log show it.
pub const NO_TOKEN: &str = "no token";

/// what the agent says when it has no store at all, as on macOS and Linux until a
/// store is added behind the interface.
pub const NO_TOKEN_STORE: &str = "no token store";

/// the agent's view of the token store.
pub struct StoreTokens {
    store: Option<Arc<dyn TokenStore>>,
    /// which repositories had a token when the store was last asked.
    known: RefCell<BTreeMap<RepositoryKey, bool>>,
    changed: watch::Sender<u64>,
}

impl StoreTokens {
    #[must_use]
    pub fn new(store: Option<Arc<dyn TokenStore>>) -> Self {
        Self {
            store,
            known: RefCell::new(BTreeMap::new()),
            changed: watch::Sender::new(0),
        }
    }

    /// fires when a token was set, so a pool skipped for want of one may start.
    #[must_use]
    pub fn changes(&self) -> watch::Receiver<u64> {
        self.changed.subscribe()
    }

    /// whether the repository had a token when the store was last asked.
    #[must_use]
    pub fn has_token(&self, key: &RepositoryKey) -> bool {
        self.known.borrow().get(key).copied().unwrap_or(false)
    }

    /// asks the store about each repository, and keeps the answers for the snapshot;
    /// a store that fails leaves the repository without one.
    pub fn refresh<'a>(&self, keys: impl IntoIterator<Item = &'a RepositoryKey>) {
        let mut known = self.known.borrow_mut();
        known.clear();
        for key in keys {
            let present = self
                .store
                .as_ref()
                .is_some_and(|store| matches!(store.get(key), Ok(Some(_))));
            known.insert(key.clone(), present);
        }
    }

    pub fn set(&self, key: &RepositoryKey, token: &Secret) -> Result<(), StoreError> {
        let store = self.store.as_ref().ok_or_else(no_store)?;
        store.set(key, token)?;
        self.known.borrow_mut().insert(key.clone(), true);
        self.changed.send_modify(|count| *count += 1);
        Ok(())
    }

    pub fn remove(&self, key: &RepositoryKey) -> Result<(), StoreError> {
        let store = self.store.as_ref().ok_or_else(no_store)?;
        store.remove(key)?;
        self.known.borrow_mut().insert(key.clone(), false);
        Ok(())
    }

    /// the repository's token for a registration, or why there is none.
    pub fn read(&self, key: &RepositoryKey) -> Result<Secret, String> {
        let Some(store) = &self.store else {
            return Err(NO_TOKEN_STORE.to_string());
        };
        match store.get(key) {
            Ok(Some(token)) => Ok(token),
            Ok(None) => Err(NO_TOKEN.to_string()),
            Err(error) => Err(format!("the token store failed: {}", error.describe())),
        }
    }
}

fn no_store() -> StoreError {
    StoreError::new(NO_TOKEN_STORE)
}

impl TokenSource for StoreTokens {
    fn token(&self, owner: &str, repository: &str, _file: Option<&Path>) -> Result<Secret, String> {
        self.read(&RepositoryKey::of(owner, repository))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::token_store::MemoryTokenStore;

    fn key() -> RepositoryKey {
        RepositoryKey::of("Owner", "Repo")
    }

    fn tokens() -> (StoreTokens, Arc<MemoryTokenStore>) {
        let store = Arc::new(MemoryTokenStore::default());
        (StoreTokens::new(Some(store.clone())), store)
    }

    #[test]
    fn a_registration_reads_the_stored_token_by_the_lower_case_key_and_never_the_file() {
        let (tokens, _) = tokens();
        tokens
            .set(&key(), &Secret::new("fake-token".to_string()))
            .unwrap();
        let token = tokens
            .token("OWNER", "repo", Some(Path::new("/no/such/file")))
            .unwrap();
        assert_eq!(token.expose(), "fake-token");
    }

    #[test]
    fn a_missing_token_and_a_missing_store_have_their_own_words() {
        let (tokens, _) = tokens();
        assert_eq!(tokens.read(&key()).unwrap_err(), NO_TOKEN);
        let none = StoreTokens::new(None);
        assert_eq!(none.read(&key()).unwrap_err(), NO_TOKEN_STORE);
        assert!(none.set(&key(), &Secret::new("x".to_string())).is_err());
    }

    #[test]
    fn setting_and_removing_update_what_the_snapshot_knows_but_only_setting_fires_a_change() {
        let (tokens, _) = tokens();
        let mut changes = tokens.changes();
        assert!(!tokens.has_token(&key()));
        tokens
            .set(&key(), &Secret::new("fake".to_string()))
            .unwrap();
        assert!(tokens.has_token(&key()));
        assert!(changes.has_changed().unwrap());
        changes.borrow_and_update();
        tokens.remove(&key()).unwrap();
        assert!(!tokens.has_token(&key()));
        assert!(!changes.has_changed().unwrap());
    }

    #[test]
    fn a_failing_store_is_an_error_whose_text_holds_no_token() {
        let (tokens, store) = tokens();
        tokens
            .set(&key(), &Secret::new("fake-secret".to_string()))
            .unwrap();
        store.fail_with(Some("the vault is locked"));
        let error = tokens.read(&key()).unwrap_err();
        assert!(error.contains("the vault is locked"), "{error}");
        assert!(!error.contains("fake-secret"));
        assert!(
            tokens
                .set(&key(), &Secret::new("fake-secret".to_string()))
                .is_err()
        );
    }

    #[test]
    fn refresh_asks_the_store_for_each_repository() {
        let (tokens, store) = tokens();
        store.set(&key(), &Secret::new("fake".to_string())).unwrap();
        let other = RepositoryKey::of("o", "other");
        tokens.refresh([&key(), &other]);
        assert!(tokens.has_token(&key()));
        assert!(!tokens.has_token(&other));
    }
}
