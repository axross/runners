//! the store the tests use: tokens in memory, as UTF-8, with a switch that makes
//! every call fail.

use std::collections::BTreeMap;
use std::sync::Mutex;

use crate::runtime::secret::Secret;

use super::{StoreError, TokenKey, TokenStore};

#[derive(Default)]
struct Held {
    tokens: BTreeMap<TokenKey, String>,
    failure: Option<StoreError>,
}

/// tokens kept in memory; it never writes a token anywhere, and its `Debug` form
/// names no token.
#[derive(Default)]
pub struct MemoryTokenStore {
    held: Mutex<Held>,
}

impl std::fmt::Debug for MemoryTokenStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MemoryTokenStore(<redacted>)")
    }
}

impl MemoryTokenStore {
    fn held(&self) -> std::sync::MutexGuard<'_, Held> {
        self.held
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// makes every later call fail with `message` until it is called with `None`.
    pub fn fail_with(&self, message: Option<&str>) {
        self.held().failure = message.map(StoreError::new);
    }

    /// makes every later call fail with `error`, of whatever kind, until it is called
    /// with `None`.
    pub fn fail_with_error(&self, error: Option<StoreError>) {
        self.held().failure = error;
    }

    /// the keys that have a token.
    #[must_use]
    pub fn keys(&self) -> Vec<TokenKey> {
        self.held().tokens.keys().cloned().collect()
    }

    fn checked(held: &Held) -> Result<(), StoreError> {
        held.failure.clone().map_or(Ok(()), Err)
    }
}

impl TokenStore for MemoryTokenStore {
    fn get(&self, key: &TokenKey) -> Result<Option<Secret>, StoreError> {
        let held = self.held();
        Self::checked(&held)?;
        Ok(held.tokens.get(key).cloned().map(Secret::new))
    }

    fn set(&self, key: &TokenKey, token: &Secret) -> Result<(), StoreError> {
        let mut held = self.held();
        Self::checked(&held)?;
        held.tokens.insert(key.clone(), token.expose().to_string());
        Ok(())
    }

    fn remove(&self, key: &TokenKey) -> Result<(), StoreError> {
        let mut held = self.held();
        Self::checked(&held)?;
        held.tokens.remove(key);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_token_is_set_replaced_and_removed_by_key() {
        let store = MemoryTokenStore::default();
        let key = TokenKey::of("o", "r");
        assert!(store.get(&key).unwrap().is_none());
        store
            .set(&key, &Secret::new("ghp_fake_one".to_string()))
            .unwrap();
        store
            .set(&key, &Secret::new("ghp_fake_two".to_string()))
            .unwrap();
        assert_eq!(store.get(&key).unwrap().unwrap().expose(), "ghp_fake_two");
        store.remove(&key).unwrap();
        store.remove(&key).unwrap();
        assert!(store.get(&key).unwrap().is_none());
    }

    #[test]
    fn a_repository_and_an_organization_of_one_name_keep_their_own_tokens() {
        let store = MemoryTokenStore::default();
        let repository = TokenKey::of("o", "o");
        let organization = TokenKey::parse_organization("O").unwrap();
        store
            .set(&repository, &Secret::new("ghp_fake_repo".to_string()))
            .unwrap();
        store
            .set(&organization, &Secret::new("ghp_fake_org".to_string()))
            .unwrap();
        assert_eq!(
            store.get(&repository).unwrap().unwrap().expose(),
            "ghp_fake_repo"
        );
        assert_eq!(
            store.get(&organization).unwrap().unwrap().expose(),
            "ghp_fake_org"
        );
        store.remove(&organization).unwrap();
        assert!(store.get(&organization).unwrap().is_none());
        assert_eq!(store.keys(), vec![repository]);
    }

    #[test]
    fn a_failing_store_fails_every_call_and_recovers() {
        let store = MemoryTokenStore::default();
        let key = TokenKey::of("o", "r");
        store.fail_with(Some("the store is locked"));
        assert_eq!(
            store.get(&key).unwrap_err().to_string(),
            "the store is locked"
        );
        assert!(
            store
                .set(&key, &Secret::new("ghp_fake".to_string()))
                .is_err()
        );
        assert!(store.remove(&key).is_err());
        store.fail_with(None);
        assert!(store.get(&key).is_ok());
    }

    #[test]
    fn the_debug_form_names_no_token() {
        let store = MemoryTokenStore::default();
        store
            .set(
                &TokenKey::of("o", "r"),
                &Secret::new("ghp_fake".to_string()),
            )
            .unwrap();
        assert!(!format!("{store:?}").contains("ghp_fake"));
    }
}
