//! the Windows Credential Manager store: one generic credential per repository named
//! `app.axross.marshal:github.com/<owner>/<name>`, written as UTF-16 with local
//! persistence, because the crate's default persistence is the enterprise one, which
//! roams with the user's profile.

use super::{EntryName, KeyringTokenStore, RepositoryKey, StoreError};

/// the signing identifier that names every Marshal credential.
pub const CREDENTIAL_PREFIX: &str = "app.axross.marshal:github.com/";

/// the credential's target name for `key`.
#[must_use]
pub fn target_name(key: &RepositoryKey) -> String {
    format!("{CREDENTIAL_PREFIX}{key}")
}

fn naming(key: &RepositoryKey) -> EntryName {
    EntryName {
        service: "app.axross.marshal".to_string(),
        user: key.to_string(),
        modifiers: vec![
            ("target", target_name(key)),
            ("persistence", "Local".to_string()),
        ],
    }
}

/// the store of this user's Credential Manager.
pub fn credential_manager() -> Result<KeyringTokenStore, StoreError> {
    let store = windows_native_keyring_store::Store::new().map_err(|error| {
        StoreError::new(format!("the Credential Manager cannot be opened: {error}"))
    })?;
    Ok(KeyringTokenStore::new(store, naming))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_credential_is_named_for_the_lower_case_repository() {
        assert_eq!(
            target_name(&RepositoryKey::of("Axross", "Runners")),
            "app.axross.marshal:github.com/axross/runners"
        );
    }

    #[test]
    fn the_entry_asks_for_local_persistence() {
        let name = naming(&RepositoryKey::of("o", "r"));
        assert!(
            name.modifiers
                .contains(&("persistence", "Local".to_string()))
        );
    }
}
