//! the Keychain store against this user's login keychain, under a service of its own
//! so it never touches Marshal's items.
#![cfg(target_os = "macos")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use marshal_cli::runtime::secret::Secret;
use marshal_cli::token_store::macos::KeychainTokenStore;
use marshal_cli::token_store::{RepositoryKey, TokenStore};

const SERVICE: &str = "app.axross.marshal.test-keychain-store";

#[test]
fn a_token_is_set_read_replaced_and_removed() {
    let store = KeychainTokenStore::with_service(SERVICE).unwrap();
    let key = RepositoryKey::of("Owner", "Repository");
    store.remove(&key).unwrap();

    assert!(store.get(&key).unwrap().is_none());
    store.set(&key, &Secret::new("first-token".into())).unwrap();
    assert_eq!(store.get(&key).unwrap().unwrap().expose(), "first-token");

    store
        .set(&key, &Secret::new("second-token".into()))
        .unwrap();
    assert_eq!(store.get(&key).unwrap().unwrap().expose(), "second-token");

    // `security` without -g or -w prints the item's attributes and never its token
    let found = std::process::Command::new("/usr/bin/security")
        .args(["find-generic-password", "-s", SERVICE])
        .args(["-a", "github.com/owner/repository"])
        .output()
        .unwrap();
    store.remove(&key).unwrap();
    assert!(store.get(&key).unwrap().is_none());
    // removing a repository without a token is not an error
    store.remove(&key).unwrap();

    // the label is attribute 7, which `security` prints by number
    let attributes = String::from_utf8_lossy(&found.stdout);
    assert!(found.status.success());
    assert!(attributes.contains("0x00000007 <blob>=\"Marshal: owner/repository\""));
    assert!(!attributes.contains("second-token"));
}
