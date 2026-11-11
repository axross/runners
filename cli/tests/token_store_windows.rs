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

mod support;

use std::process::Command;
use std::sync::Arc;

use marshal_cli::agent::CallError;
use marshal_cli::agent::machine::Launch;
use marshal_cli::runtime::secret::Secret;
use marshal_cli::token_store::windows::{credential_manager, target_name};
use marshal_cli::token_store::{MAX_TOKEN_UTF16_UNITS, RepositoryKey, TokenStore, accepted_token};
use support::agent::Rig;

const FAKE: &str = "fake-token-windows-4444";

/// `cmdkey` lists the credential the way Windows stores it.
fn listed(target: &str) -> String {
    let output = Command::new("cmdkey")
        .arg(format!("/list:{target}"))
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout).to_string()
}

/// whether `cmdkey`'s listing holds an entry for `target`: its `Target:` line names it.
/// The heading `Currently stored credentials for <target>:` is printed whether or not
/// there is one, so the target's mere presence in the text says nothing.
fn has_entry(listing: &str, target: &str) -> bool {
    listing
        .lines()
        .any(|line| line.trim_start().starts_with("Target:") && line.contains(target))
}

#[test]
fn a_token_is_a_local_machine_credential_that_round_trips_and_goes() {
    let store = credential_manager().unwrap();
    let key = RepositoryKey::of(&format!("Marshal-Test-{}", std::process::id()), "Repo");
    let _cleanup = Cleanup(key.clone());
    let target = target_name(&key);
    assert!(target.starts_with("app.axross.marshal:github.com/marshal-test-"));

    assert!(store.get(&key).unwrap().is_none());
    store.set(&key, &Secret::new(FAKE.to_string())).unwrap();
    assert_eq!(store.get(&key).unwrap().unwrap().expose(), FAKE);

    // local persistence: the enterprise persistence roams with the profile; the store
    // reports it as an attribute, and cmdkey shows the same
    let attributes = store.attributes(&key).unwrap();
    assert_eq!(
        attributes.get("persistence").map(String::as_str),
        Some("Local")
    );
    let text = listed(&target);
    assert!(has_entry(&text, &target), "{text}");
    assert!(!text.contains("* NONE *"), "{text}");
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
    let text = listed(&target);
    assert!(!has_entry(&text, &target), "{text}");
    assert!(text.contains("* NONE *"), "{text}");
    assert!(!text.contains(FAKE));
}

/// removes a test credential when the test ends, whether it passed or not.
struct Cleanup(RepositoryKey);

impl Drop for Cleanup {
    fn drop(&mut self) {
        if let Ok(store) = credential_manager() {
            let _ = store.remove(&self.0);
        }
    }
}

fn test_key(label: &str) -> RepositoryKey {
    RepositoryKey::of(
        &format!("marshal-test-{}-{label}", std::process::id()),
        "repo",
    )
}

#[test]
fn a_token_of_the_most_utf16_units_round_trips_and_one_more_is_refused() {
    let store = credential_manager().unwrap();
    let key = test_key("limit");
    let _cleanup = Cleanup(key.clone());

    let longest = "a".repeat(MAX_TOKEN_UTF16_UNITS);
    let accepted = accepted_token(&longest).unwrap();
    store.set(&key, &accepted).unwrap();
    assert_eq!(store.get(&key).unwrap().unwrap().expose(), longest);

    // the same units from characters outside the basic plane, two units each
    let pairs = "\u{1F600}".repeat(MAX_TOKEN_UTF16_UNITS / 2);
    assert_eq!(pairs.encode_utf16().count(), MAX_TOKEN_UTF16_UNITS);
    store.set(&key, &accepted_token(&pairs).unwrap()).unwrap();
    assert_eq!(store.get(&key).unwrap().unwrap().expose(), pairs);

    assert!(accepted_token(&"a".repeat(MAX_TOKEN_UTF16_UNITS + 1)).is_err());
    assert!(accepted_token(&format!("{pairs}a")).is_err());
}

#[test]
fn a_token_with_non_ascii_and_non_bmp_text_round_trips() {
    let store = credential_manager().unwrap();
    let key = test_key("unicode");
    let _cleanup = Cleanup(key.clone());
    let text = "fake-\u{e9}\u{4e16}\u{1F600}-token";
    store.set(&key, &Secret::new(text.to_string())).unwrap();
    assert_eq!(store.get(&key).unwrap().unwrap().expose(), text);
}

#[tokio::test(flavor = "current_thread")]
async fn a_token_set_through_the_agent_reaches_the_credential_manager_and_a_registration() {
    let key = test_key("agent");
    let _cleanup = Cleanup(key.clone());
    let (owner, name) = key.as_str().split_once('/').unwrap();
    let config = format!(
        r#"{{"imageName":"actions-runner:local","repositories":[
            {{"owner":"{owner}","repository":"{name}","name":"r","slots":1,"tokenPath":"C:\\no-such.token","volumes":[]}}]}}"#
    );
    let rig = Rig::new(&config);
    let store: Arc<dyn TokenStore> = Arc::new(credential_manager().unwrap());
    *rig.other_store.borrow_mut() = Some(Arc::clone(&store));
    rig.hold_jobs();
    let rig = &rig;
    let key = &key;
    let (_, ()) = rig
        .drive(Launch::Running, |conn| async move {
            // the only pool has no token, so the agent stops at launch; a token set
            // later does not start a stopped agent, a `start_all` does
            let stopped = conn.until_state("stopped").await;
            assert_eq!(stopped.agent.reason.unwrap().code, "no_pool_started");

            // too long a token is refused and stores nothing
            let long = "a".repeat(MAX_TOKEN_UTF16_UNITS + 1);
            match conn.set_token(key.as_str(), &long).await {
                Err(CallError::Agent(body)) => assert_eq!(body.code, "token_invalid"),
                other => panic!("an oversize token was not refused: {other:?}"),
            }
            assert!(store.get(key).unwrap().is_none());

            conn.set_token(key.as_str(), FAKE).await.unwrap();
            assert_eq!(store.get(key).unwrap().unwrap().expose(), FAKE);
            assert_eq!(conn.status().await.agent.state, "stopped");
            // the pool starts on the stored token, read from the store
            conn.ok("start_all").await;
            conn.until_runner_runs().await;
            assert_eq!(rig.registered_with(), vec![FAKE.to_string()]);

            conn.client.token_remove(key.as_str()).await.unwrap();
            assert!(store.get(key).unwrap().is_none());
            rig.mark_running();
            conn.ok("quit").await;
        })
        .await;
}
