//! the Keychain store against this user's login keychain, under a service of its own
//! so it never touches Marshal's items.
#![cfg(target_os = "macos")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::OnceLock;

use marshal_cli::runtime::secret::Secret;
use marshal_cli::token_store::macos::KeychainTokenStore;
use marshal_cli::token_store::{TokenKey, TokenStore};

const SERVICE: &str = "app.axross.marshal.test-keychain-store";

/// one store for every test: each store turns the Keychain's prompts off for the whole
/// process and back on when dropped, so a store per test would let a test that ends
/// turn them on while another still runs.
fn store() -> &'static KeychainTokenStore {
    static STORE: OnceLock<KeychainTokenStore> = OnceLock::new();
    STORE.get_or_init(|| KeychainTokenStore::with_service(SERVICE).unwrap())
}

#[test]
fn a_token_is_set_read_replaced_and_removed() {
    let store = store();
    let key = TokenKey::of("Owner", "Repository");
    store.remove(&key).unwrap();

    assert!(store.get(&key).unwrap().is_none());
    store.set(&key, &Secret::new("first-token".into())).unwrap();
    assert_eq!(store.get(&key).unwrap().unwrap().expose(), "first-token");

    store
        .set(&key, &Secret::new("second-token".into()))
        .unwrap();
    assert_eq!(store.get(&key).unwrap().unwrap().expose(), "second-token");

    let found = attributes_of("github.com/owner/repository");
    store.remove(&key).unwrap();
    assert!(store.get(&key).unwrap().is_none());
    // removing a repository without a token is not an error
    store.remove(&key).unwrap();

    assert!(found.contains("0x00000007 <blob>=\"Marshal: owner/repository\""));
    assert!(!found.contains("second-token"));
}

#[test]
fn an_organizations_token_is_an_item_apart_from_its_repositories() {
    let store = store();
    let organization = TokenKey::parse_organization("Owner").unwrap();
    let repository = TokenKey::of("Owner", "Other");
    store.remove(&organization).unwrap();
    store.remove(&repository).unwrap();

    store
        .set(&organization, &Secret::new("organization-token".into()))
        .unwrap();
    store
        .set(&repository, &Secret::new("repository-token".into()))
        .unwrap();
    let found = attributes_of("github.com/owner");
    store.remove(&organization).unwrap();
    let kept = store.get(&repository).unwrap();
    store.remove(&repository).unwrap();

    assert!(found.contains("0x00000007 <blob>=\"Marshal: owner (organisation)\""));
    assert!(!found.contains("organization-token"));
    assert_eq!(kept.unwrap().expose(), "repository-token");
    assert!(store.get(&organization).unwrap().is_none());
}

/// the attributes of the test item whose account is `account`; `security` without -g
/// or -w prints them and never the token, and the label is attribute 7, which it
/// prints by number.
fn attributes_of(account: &str) -> String {
    let found = std::process::Command::new("/usr/bin/security")
        .args(["find-generic-password", "-s", SERVICE, "-a", account])
        .output()
        .unwrap();
    assert!(found.status.success(), "no item for {account}");
    String::from_utf8_lossy(&found.stdout).into_owned()
}
