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
        let off = "the Keychain's prompts cannot be turned off";
        let no_prompts = SecKeychain::disable_user_interaction()
            .map_err(|error| failure(off, || off.to_string(), error))?;
        Ok(Self {
            service: service.to_string(),
            _no_prompts: no_prompts,
        })
    }

    fn options(&self, key: &TokenKey) -> PasswordOptions {
        PasswordOptions::new_generic_password(&self.service, &account(key))
    }
}

/// what a code that needs the user may mean: the codes a locked keychain returns may
/// also be the ones an item another build of Marshal added returns, which only the Mac
/// run tells apart, so the words name both.
const LOCKED_OR_OTHER_BUILD: &str =
    "the login keychain is locked, or another build of Marshal stored this token";

/// the words for a delete the Keychain refused: unlock it, or delete the item by hand,
/// the one command the window gives.
fn delete_by_hand(service: &str, key: &TokenKey) -> String {
    format!(
        "the stored token cannot be deleted: {LOCKED_OR_OTHER_BUILD}. Unlock the login keychain in Keychain Access, or delete the item with security delete-generic-password -s {service} -a {} or in Keychain Access, then enter the token again",
        account(key)
    )
}

/// the store error for a Keychain failure, with Apple's code; a code that needs the
/// user gets `needs_user`'s words, its cause and its fix, instead of `words`.
fn failure(words: &str, needs_user: impl FnOnce() -> String, error: Error) -> StoreError {
    let kind = kind_of(error.code());
    let code = Some(i64::from(error.code()));
    if kind == StoreErrorKind::NeedsUser {
        StoreError::with(kind, code, needs_user())
    } else {
        StoreError::with(kind, code, words)
    }
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
            Err(error) => Err(failure(
                "the token cannot be read from the Keychain",
                || {
                    format!(
                        "the token cannot be read: {LOCKED_OR_OTHER_BUILD}. Unlock the login keychain in Keychain Access, or enter the token again in Tokens"
                    )
                },
                error,
            )),
        }
    }

    fn set(&self, key: &TokenKey, token: &Secret) -> Result<(), StoreError> {
        self.remove(key)?;
        let mut options = self.options(key);
        options.set_label(&label(key));
        set_generic_password_options(token.expose().as_bytes(), options)
            .map_err(|error| {
                failure(
                    "the token cannot be added to the Keychain",
                    || {
                        "the token cannot be added: the login keychain is locked. Unlock it in Keychain Access".to_string()
                    },
                    error,
                )
            })
    }

    fn remove(&self, key: &TokenKey) -> Result<(), StoreError> {
        match delete_generic_password_options(self.options(key)) {
            Ok(()) => Ok(()),
            Err(error) if error.code() == ITEM_NOT_FOUND => Ok(()),
            Err(error) => Err(failure(
                "the token cannot be deleted from the Keychain",
                || delete_by_hand(&self.service, key),
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

    #[test]
    fn a_refusal_that_needs_the_user_names_both_causes_and_the_fix() {
        let key = TokenKey::of("Axross", "Runners");
        let refused = failure(
            "the token cannot be deleted from the Keychain",
            || delete_by_hand(SERVICE, &key),
            Error::from_code(-25308),
        );
        assert_eq!(refused.kind(), StoreErrorKind::NeedsUser);
        let words = refused.describe();
        assert!(words.contains("locked, or another build"), "{words}");
        assert!(
            words.contains(
                "security delete-generic-password -s app.axross.marshal -a github.com/axross/runners"
            ),
            "{words}"
        );
        assert!(words.ends_with("(kind needs_user, code -25308)"), "{words}");

        let other = failure(
            "the token cannot be deleted from the Keychain",
            || unreachable!("only a code that needs the user words its cause"),
            Error::from_code(-25299),
        );
        assert_eq!(
            other.describe(),
            "the token cannot be deleted from the Keychain (kind other, code -25299)"
        );
    }
}
