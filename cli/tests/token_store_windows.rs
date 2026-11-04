//! the real Windows Credential Manager: a token set through the store is a generic
//! credential with the documented name, persisted on this machine only, and is read
//! back and removed. Windows only; it needs a logon session with a credential vault.
#![cfg(windows)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::process::Command;

use marshal_cli::runtime::secret::Secret;
use marshal_cli::token_store::windows::{credential_manager, target_name};
use marshal_cli::token_store::{RepositoryKey, TokenStore};

const FAKE: &str = "fake-token-windows-4444";

/// `cmdkey` lists the credential the way Windows stores it.
fn listed(target: &str) -> String {
    let output = Command::new("cmdkey")
        .arg(format!("/list:{target}"))
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout).to_string()
}

#[test]
fn a_token_is_a_local_machine_credential_that_round_trips_and_goes() {
    let store = credential_manager().unwrap();
    let key = RepositoryKey::of(&format!("Marshal-Test-{}", std::process::id()), "Repo");
    let target = target_name(&key);
    assert!(target.starts_with("app.axross.marshal:github.com/marshal-test-"));

    assert!(store.get(&key).unwrap().is_none());
    store.set(&key, &Secret::new(FAKE.to_string())).unwrap();
    assert_eq!(store.get(&key).unwrap().unwrap().expose(), FAKE);

    // local persistence: the enterprise persistence roams with the profile
    let text = listed(&target);
    assert!(text.contains(&target), "{text}");
    assert!(text.contains("Local machine persistence"), "{text}");
    assert!(!text.contains(FAKE));

    // setting again replaces the token
    store
        .set(&key, &Secret::new("fake-token-windows-5555".to_string()))
        .unwrap();
    assert_eq!(
        store.get(&key).unwrap().unwrap().expose(),
        "fake-token-windows-5555"
    );

    store.remove(&key).unwrap();
    assert!(store.get(&key).unwrap().is_none());
    store.remove(&key).unwrap();
    assert!(!listed(&target).contains(&target));
}
