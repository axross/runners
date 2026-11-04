//! a [`TokenStore`] over a `keyring-core` credential store, so a platform that has
//! such a store adds only the naming of its entries.

use std::collections::HashMap;
use std::sync::Arc;

use keyring_core::api::CredentialStore;
use keyring_core::{Entry, Error};

use crate::runtime::secret::Secret;

use super::{RepositoryKey, StoreError, StoreErrorKind, TokenStore};

/// how one repository's entry is named in a store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryName {
    pub service: String,
    pub user: String,
    /// the store's entry modifiers, such as a target name or a persistence.
    pub modifiers: Vec<(&'static str, String)>,
}

/// names a repository's entry.
pub type Naming = fn(&RepositoryKey) -> EntryName;

/// what a store's failure means to the caller: its kind and the platform's code.
pub type Classified = (StoreErrorKind, Option<i64>);

/// sorts a failure; a store with codes of its own gives its own.
pub type Classifier = fn(&Error) -> Classified;

fn unclassified(_: &Error) -> Classified {
    (StoreErrorKind::Other, None)
}

/// tokens in a `keyring-core` store, one entry per repository.
pub struct KeyringTokenStore {
    store: Arc<CredentialStore>,
    naming: Naming,
    classifier: Classifier,
}

impl KeyringTokenStore {
    #[must_use]
    pub fn new(store: Arc<CredentialStore>, naming: Naming) -> Self {
        Self {
            store,
            naming,
            classifier: unclassified,
        }
    }

    /// reads each failure's kind and code with `classifier`.
    #[must_use]
    pub fn classifying(mut self, classifier: Classifier) -> Self {
        self.classifier = classifier;
        self
    }

    fn entry(&self, key: &RepositoryKey) -> Result<Entry, StoreError> {
        let name = (self.naming)(key);
        let modifiers: HashMap<&str, &str> = name
            .modifiers
            .iter()
            .map(|(modifier, value)| (*modifier, value.as_str()))
            .collect();
        self.store
            .build(&name.service, &name.user, Some(&modifiers))
            .map_err(|error| self.failed(&error))
    }

    /// the attributes the store keeps for the repository's existing entry, such as the
    /// Credential Manager's `persistence`; they hold no token.
    pub fn attributes(&self, key: &RepositoryKey) -> Result<HashMap<String, String>, StoreError> {
        self.entry(key)?
            .get_attributes()
            .map_err(|error| self.failed(&error))
    }

    fn failed(&self, error: &Error) -> StoreError {
        let (kind, code) = (self.classifier)(error);
        StoreError::with(kind, code, format!("the credential store failed: {error}"))
    }
}

impl TokenStore for KeyringTokenStore {
    fn get(&self, key: &RepositoryKey) -> Result<Option<Secret>, StoreError> {
        match self.entry(key)?.get_password() {
            Ok(token) => Ok(Some(Secret::new(token))),
            Err(Error::NoEntry) => Ok(None),
            Err(error) => Err(self.failed(&error)),
        }
    }

    fn set(&self, key: &RepositoryKey, token: &Secret) -> Result<(), StoreError> {
        self.entry(key)?
            .set_password(token.expose())
            .map_err(|error| self.failed(&error))
    }

    fn remove(&self, key: &RepositoryKey) -> Result<(), StoreError> {
        match self.entry(key)?.delete_credential() {
            Ok(()) | Err(Error::NoEntry) => Ok(()),
            Err(error) => Err(self.failed(&error)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn by_key(key: &RepositoryKey) -> EntryName {
        EntryName {
            service: "app.axross.marshal".to_string(),
            user: key.to_string(),
            modifiers: Vec::new(),
        }
    }

    fn store() -> KeyringTokenStore {
        KeyringTokenStore::new(keyring_core::mock::Store::new().unwrap(), by_key)
    }

    #[test]
    fn a_token_round_trips_and_a_missing_entry_is_none_and_removing_it_is_fine() {
        let store = store();
        let key = RepositoryKey::of("o", "r");
        assert!(store.get(&key).unwrap().is_none());
        store.remove(&key).unwrap();
        store
            .set(&key, &Secret::new("ghp_fake".to_string()))
            .unwrap();
        assert_eq!(store.get(&key).unwrap().unwrap().expose(), "ghp_fake");
        store.remove(&key).unwrap();
        assert!(store.get(&key).unwrap().is_none());
    }

    #[test]
    fn the_attributes_of_a_missing_entry_are_a_failure_and_hold_no_token() {
        let store = store();
        let key = RepositoryKey::of("o", "r");
        store
            .set(&key, &Secret::new("ghp_fake".to_string()))
            .unwrap();
        let attributes = store.attributes(&key).unwrap_or_default();
        assert!(attributes.values().all(|value| !value.contains("ghp_fake")));
        store.remove(&key).unwrap();
        assert!(store.attributes(&key).is_err());
    }

    #[test]
    fn a_modifier_the_store_does_not_know_is_a_failure_that_names_no_token() {
        let store =
            KeyringTokenStore::new(keyring_core::mock::Store::new().unwrap(), |key| EntryName {
                modifiers: vec![("persistence", "Local".to_string())],
                ..by_key(key)
            });
        let error = store
            .set(
                &RepositoryKey::of("o", "r"),
                &Secret::new("ghp_fake".to_string()),
            )
            .unwrap_err();
        assert!(!error.to_string().contains("ghp_fake"), "{error}");
    }

    #[test]
    fn a_failure_carries_the_kind_and_code_the_classifier_gives() {
        let store =
            KeyringTokenStore::new(keyring_core::mock::Store::new().unwrap(), |key| EntryName {
                modifiers: vec![("persistence", "Local".to_string())],
                ..by_key(key)
            })
            .classifying(|_| (StoreErrorKind::NeedsUser, Some(-25308)));
        let error = store
            .set(
                &RepositoryKey::of("o", "r"),
                &Secret::new("ghp_fake".to_string()),
            )
            .unwrap_err();
        assert_eq!(error.kind(), StoreErrorKind::NeedsUser);
        assert_eq!(error.code(), Some(-25308));
        assert!(!error.describe().contains("ghp_fake"));
    }

    #[test]
    fn a_store_without_a_classifier_reports_other_with_no_code() {
        let store =
            KeyringTokenStore::new(keyring_core::mock::Store::new().unwrap(), |key| EntryName {
                modifiers: vec![("persistence", "Local".to_string())],
                ..by_key(key)
            });
        let error = store.get(&RepositoryKey::of("o", "r")).unwrap_err();
        assert_eq!((error.kind(), error.code()), (StoreErrorKind::Other, None));
    }
}
