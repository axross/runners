//! the Mac's Keychain store: one generic password per repository or organization in
//! the user's login keychain, with the service `app.axross.marshal`, the account
//! `github.com/<owner>/<name>` or `github.com/<org>`, and the label
//! `Marshal: <owner>/<name>` or `Marshal: <org> (organisation)`. The agent adds
//! each item itself, so macOS gives it the default access of an item a signed program
//! adds, and the store turns the Keychain's user interaction off for as long as it
//! lives, so a call that would prompt fails instead; `docs/operations/cli.md` and
//! `docs/conventions/security.md` describe the item and its trust.

use security_framework::base::Error;
use security_framework::os::macos::keychain::{KeychainUserInteractionLock, SecKeychain};
use security_framework::passwords::{
    PasswordOptions, delete_generic_password_options, generic_password,
    set_generic_password_options,
};

use crate::runtime::secret::Secret;

use super::{StoreError, StoreErrorKind, TokenKey, TokenStore};

/// the service every Marshal item carries: F0's signing identifier.
pub const SERVICE: &str = "app.axross.marshal";

const ITEM_NOT_FOUND: i32 = -25300;

/// the codes that mean the keychain needs the user: `errSecInteractionNotAllowed`,
/// `errSecInteractionRequired` and `errSecAuthFailed`, which a locked keychain returns
/// on the macOS releases Apple's sources and forums describe.
const NEEDS_USER: [i32; 3] = [-25308, -25315, -25293];

/// the item's account for `key`.
#[must_use]
pub fn account(key: &TokenKey) -> String {
    format!("github.com/{key}")
}

/// the label Keychain Access shows for `key`'s item.
#[must_use]
pub fn label(key: &TokenKey) -> String {
    // only a repository's key holds a slash
    if key.as_str().contains('/') {
        format!("Marshal: {key}")
    } else {
        format!("Marshal: {key} (organisation)")
    }
}

/// the store of this user's login keychain.
pub struct KeychainTokenStore {
    service: String,
    _no_prompts: KeychainUserInteractionLock,
}

impl std::fmt::Debug for KeychainTokenStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "KeychainTokenStore({})", self.service)
    }
}

/// the login keychain's store under [`SERVICE`].
pub fn login_keychain() -> Result<KeychainTokenStore, StoreError> {
    KeychainTokenStore::with_service(SERVICE)
}

impl KeychainTokenStore {
    /// a store whose items carry `service`; tests use their own service so they never
    /// touch Marshal's items.
    pub fn with_service(service: &str) -> Result<Self, StoreError> {
        let no_prompts = SecKeychain::disable_user_interaction()
            .map_err(|error| failure("the Keychain's prompts cannot be turned off", error))?;
        Ok(Self {
            service: service.to_string(),
            _no_prompts: no_prompts,
        })
    }

    fn options(&self, key: &TokenKey) -> PasswordOptions {
        PasswordOptions::new_generic_password(&self.service, &account(key))
    }
}

/// the store error for a Keychain failure, with Apple's code.
fn failure(words: &str, error: Error) -> StoreError {
    StoreError::with(kind_of(error.code()), Some(i64::from(error.code())), words)
}

fn kind_of(code: i32) -> StoreErrorKind {
    if NEEDS_USER.contains(&code) {
        StoreErrorKind::NeedsUser
    } else {
        StoreErrorKind::Other
    }
}

impl TokenStore for KeychainTokenStore {
    fn get(&self, key: &TokenKey) -> Result<Option<Secret>, StoreError> {
        match generic_password(self.options(key)) {
            Ok(bytes) => String::from_utf8(bytes)
                .map(|token| Some(Secret::new(token)))
                .map_err(|_| StoreError::new("the stored token is not UTF-8")),
            Err(error) if error.code() == ITEM_NOT_FOUND => Ok(None),
            Err(error) => Err(failure("the token cannot be read from the Keychain", error)),
        }
    }

    fn set(&self, key: &TokenKey, token: &Secret) -> Result<(), StoreError> {
        self.remove(key)?;
        let mut options = self.options(key);
        options.set_label(&label(key));
        set_generic_password_options(token.expose().as_bytes(), options)
            .map_err(|error| failure("the token cannot be added to the Keychain", error))
    }

    fn remove(&self, key: &TokenKey) -> Result<(), StoreError> {
        match delete_generic_password_options(self.options(key)) {
            Ok(()) => Ok(()),
            Err(error) if error.code() == ITEM_NOT_FOUND => Ok(()),
            Err(error) => Err(failure(
                "the token cannot be deleted from the Keychain",
                error,
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_item_is_named_for_the_lower_case_repository() {
        let key = TokenKey::of("Axross", "Runners");
        assert_eq!(account(&key), "github.com/axross/runners");
        assert_eq!(label(&key), "Marshal: axross/runners");
    }

    #[test]
    fn an_item_is_named_for_the_lower_case_organization() {
        let key = TokenKey::parse_organization("Axross").unwrap();
        assert_eq!(account(&key), "github.com/axross");
        assert_eq!(label(&key), "Marshal: axross (organisation)");
    }

    #[test]
    fn a_locked_keychain_needs_the_user_and_other_codes_do_not() {
        for code in NEEDS_USER {
            assert_eq!(kind_of(code), StoreErrorKind::NeedsUser);
        }
        assert_eq!(kind_of(-25299), StoreErrorKind::Other);
        assert_eq!(kind_of(ITEM_NOT_FOUND), StoreErrorKind::Other);
    }
}
