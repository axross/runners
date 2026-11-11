//! the Windows Credential Manager store: one generic credential per repository named
//! `app.axross.marshal:github.com/<owner>/<name>`, written as UTF-16 with local
//! persistence, because the crate's default persistence is the enterprise one, which
//! roams with the user's profile.

use keyring_core::Error;

use super::{Classified, EntryName, KeyringTokenStore, RepositoryKey, StoreError, StoreErrorKind};

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
    Ok(KeyringTokenStore::new(store, naming).classifying(classify))
}

/// the Windows error numbers the crate names in its messages.
const NAMED_CODES: [(&str, i64); 5] = [
    ("ERROR_NO_SUCH_LOGON_SESSION", 1312),
    ("ERROR_NOT_FOUND", 1168),
    ("ERROR_BAD_USERNAME", 2202),
    ("ERROR_INVALID_FLAGS", 1004),
    ("ERROR_INVALID_PARAMETER", 87),
];

/// the Windows error number in the crate's message for a failure: it names a few
/// codes and numbers the rest as `Windows error code <n>`.
fn code_of(message: &str) -> Option<i64> {
    if let Some((_, code)) = NAMED_CODES.iter().find(|(name, _)| message.contains(name)) {
        return Some(*code);
    }
    let digits = message.split("Windows error code ").nth(1)?;
    let number: String = digits.chars().take_while(char::is_ascii_digit).collect();
    number.parse().ok()
}

/// every Credential Manager failure is of kind other, with its Windows error number.
fn classify(error: &Error) -> Classified {
    let code = match error {
        Error::PlatformFailure(inner) | Error::NoStorageAccess(inner) => {
            code_of(&inner.to_string())
        }
        _ => None,
    };
    (StoreErrorKind::Other, code)
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
    fn the_windows_error_number_is_read_from_the_crates_message() {
        assert_eq!(code_of("Windows ERROR_NO_SUCH_LOGON_SESSION"), Some(1312));
        assert_eq!(code_of("Windows error code 5"), Some(5));
        assert_eq!(code_of("something else"), None);
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
