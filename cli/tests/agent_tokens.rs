//! the token store behind the agent: what `token_set` and `token_remove` do, when the
//! agent reads a token, and what it says when the store cannot serve.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod support;

use marshal_cli::agent::machine::Launch;
use marshal_cli::agent::{CallError, Push};
use marshal_cli::token_store::{StoreError, StoreErrorKind};
use serde_json::json;
use support::agent::{ONE_POOL, OTHER_TOKEN, Rig, TOKEN, TWO_POOLS};

#[tokio::test(flavor = "current_thread")]
async fn the_next_registration_uses_the_token_that_was_set() {
    let rig = Rig::new(ONE_POOL);
    rig.store_token("o", "r", TOKEN);
    rig.hold_jobs();
    let rig = &rig;
    let (_, ()) = rig
        .drive(Launch::Running, |conn| async move {
            conn.until_runner_runs().await;
            assert_eq!(rig.registered_with(), [TOKEN]);
            // the repository is named in any case, and the key is its lower case
            conn.set_token("O/R", OTHER_TOKEN).await.unwrap();
            rig.end_jobs().await;
            conn.eventually("a registration with the new token", || {
                rig.registered_with().len() >= 2
            })
            .await;
            assert_eq!(rig.registered_with()[1], OTHER_TOKEN);
            rig.mark_running();
            conn.ok("quit").await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn after_token_remove_the_next_registration_fails_and_the_runner_backs_off() {
    let rig = Rig::new(ONE_POOL);
    rig.store_token("o", "r", TOKEN);
    rig.hold_jobs();
    let rig = &rig;
    let (_, ()) = rig
        .drive(Launch::Running, |conn| async move {
            conn.until_runner_runs().await;
            conn.client.token_remove("o/r").await.unwrap();
            // removing a token the store no longer has is not an error
            conn.client.token_remove("o/r").await.unwrap();
            rig.end_jobs().await;
            let snapshot = conn
                .until("the runner backing off", |s| {
                    s.pools
                        .first()
                        .and_then(|pool| pool.runners.first())
                        .is_some_and(|runner| runner.state == "backing_off")
                })
                .await;
            assert!(
                rig.log.lines().iter().any(|line| line.contains("no token")),
                "the log says why"
            );
            assert_eq!(
                rig.registered_with(),
                [TOKEN],
                "no registration without a token"
            );
            assert!(!snapshot.repositories[0].has_token);
            // a token set again lets the runner register again
            conn.set_token("o/r", OTHER_TOKEN).await.unwrap();
            conn.until_runner_runs().await;
            assert_eq!(rig.registered_with().last().unwrap(), OTHER_TOKEN);
            rig.mark_running();
            conn.ok("quit").await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_pool_skipped_for_want_of_a_token_starts_when_one_is_set() {
    let rig = Rig::new(TWO_POOLS);
    rig.store_token("o", "r", TOKEN);
    rig.hold_jobs();
    let rig = &rig;
    let (_, ()) = rig
        .drive(Launch::Running, |conn| async move {
            conn.until_runner_runs().await;
            let snapshot = conn.status().await;
            let pool = snapshot.pools.iter().find(|pool| pool.name == "d").unwrap();
            assert_eq!(pool.state, "skipped");
            assert!(pool.reason.as_deref().unwrap().contains("no token"));
            conn.set_token("o/d", OTHER_TOKEN).await.unwrap();
            conn.until("both pools up", |s| s.runners_up == 2).await;
            assert!(rig.registered_with().contains(&OTHER_TOKEN.to_string()));
            rig.mark_running();
            conn.ok("quit").await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn token_set_changes_the_snapshot_and_pushes_an_event() {
    let rig = Rig::new(ONE_POOL);
    let rig = &rig;
    let (_, ()) = rig
        .drive(Launch::Stopped, |conn| async move {
            let mut watcher = conn.another().await;
            watcher.client.subscribe().await.unwrap();
            let Push::Event { snapshot, .. } = watcher.push().await else {
                panic!("the snapshot comes first");
            };
            assert!(!snapshot.repositories[0].has_token);
            conn.set_token("o/r", TOKEN).await.unwrap();
            loop {
                if let Push::Event { snapshot, .. } = watcher.push().await
                    && snapshot.repositories[0].has_token
                {
                    break;
                }
            }
            conn.ok("quit").await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_token_that_is_empty_or_too_long_or_for_no_repository_is_refused() {
    let rig = Rig::new(ONE_POOL);
    let rig = &rig;
    let (_, ()) = rig
        .drive(Launch::Stopped, |conn| async move {
            for (repository, token) in [
                ("o/r", ""),
                ("o/r", "   "),
                ("o/r", &"x".repeat(2000)),
                ("or", TOKEN),
                ("o/r/s", TOKEN),
                ("", TOKEN),
            ] {
                let error = conn.set_token(repository, token).await.unwrap_err();
                let CallError::Agent(body) = error else {
                    panic!("not an agent error");
                };
                assert!(
                    body.code == "token_invalid" || body.code == "invalid_request",
                    "{repository:?}: {}",
                    body.code
                );
                assert!(
                    !body.message.contains("xxxx"),
                    "the refusal quotes the token"
                );
            }
            assert_eq!(
                conn.code("token_set", json!({"repository": "o/r"})).await,
                "invalid_request"
            );
            assert!(rig.store.keys().is_empty());
            conn.ok("quit").await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_store_failure_names_its_kind_and_code_and_never_the_token() {
    let rig = Rig::new(ONE_POOL);
    let rig = &rig;
    let (_, ()) = rig
        .drive(Launch::Stopped, |conn| async move {
            for (kind, word, code) in [
                (StoreErrorKind::NeedsUser, "needs_user", Some(1312)),
                (StoreErrorKind::AccessRefused, "access_refused", Some(5)),
                (StoreErrorKind::Other, "other", None),
            ] {
                rig.store
                    .fail_with_error(Some(StoreError::with(kind, code, "the vault said no")));
                let CallError::Agent(body) = conn.set_token("o/r", TOKEN).await.unwrap_err() else {
                    panic!("not an agent error");
                };
                assert_eq!(body.code, "store_failed");
                let fault = body.store.unwrap();
                assert_eq!((fault.kind.as_str(), fault.code), (word, code));
                assert!(body.message.contains(word), "{}", body.message);
                if let Some(code) = code {
                    assert!(body.message.contains(&code.to_string()));
                }
                assert!(!body.message.contains(TOKEN));
                let CallError::Agent(body) = conn.client.token_remove("o/r").await.unwrap_err()
                else {
                    panic!("not an agent error");
                };
                assert_eq!(body.code, "store_failed");
            }
            rig.store.fail_with_error(None);
            conn.set_token("o/r", TOKEN).await.unwrap();
            conn.ok("quit").await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_skipped_pool_and_a_failed_registration_name_the_stores_kind_and_code() {
    let rig = Rig::new(ONE_POOL);
    rig.store.fail_with_error(Some(StoreError::with(
        StoreErrorKind::NeedsUser,
        Some(1312),
        "the vault is locked",
    )));
    let rig = &rig;
    let (_, ()) = rig
        .drive(Launch::Running, |conn| async move {
            let snapshot = conn.until_state("stopped").await;
            let reason = snapshot.pools[0].reason.clone().unwrap();
            assert!(reason.contains("needs_user"), "{reason}");
            assert!(reason.contains("1312"), "{reason}");
            conn.ok("quit").await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_registration_whose_store_fails_names_the_kind_and_code_in_the_log() {
    let rig = Rig::new(ONE_POOL);
    rig.store_token("o", "r", TOKEN);
    rig.hold_jobs();
    let rig = &rig;
    let (_, ()) = rig
        .drive(Launch::Running, |conn| async move {
            conn.until_runner_runs().await;
            rig.store.fail_with_error(Some(StoreError::with(
                StoreErrorKind::AccessRefused,
                Some(5),
                "access is denied",
            )));
            rig.end_jobs().await;
            let snapshot = conn
                .until("the runner backing off", |s| {
                    s.pools
                        .first()
                        .and_then(|pool| pool.runners.first())
                        .is_some_and(|runner| runner.state == "backing_off")
                })
                .await;
            drop(snapshot);
            let line = rig
                .log
                .lines()
                .into_iter()
                .find(|line| line.contains("access_refused"))
                .expect("the log names the kind");
            assert!(line.contains("code 5"), "{line}");
            rig.store.fail_with_error(None);
            rig.mark_running();
            conn.ok("quit").await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn without_a_token_store_every_pool_is_skipped_and_a_token_cannot_be_set() {
    let rig = Rig::new(ONE_POOL);
    rig.no_store.set(true);
    let rig = &rig;
    let (_, ()) = rig
        .drive(Launch::Running, |conn| async move {
            let snapshot = conn.until_state("stopped").await;
            let reason = snapshot.pools[0].reason.clone().unwrap();
            assert!(reason.contains("no token store"), "{reason}");
            assert_eq!(snapshot.agent.reason.unwrap().code, "no_pool_started");
            assert_eq!(
                conn.code("token_set", json!({"repository": "o/r", "token": TOKEN}))
                    .await,
                "store_failed"
            );
            conn.ok("quit").await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_token_path_that_names_no_file_does_not_stop_the_pool() {
    // the configuration's `tokenPath` is for today's commands; the agent never reads it
    let rig = Rig::new(ONE_POOL);
    rig.store_token("o", "r", TOKEN);
    rig.hold_jobs();
    let rig = &rig;
    let (_, ()) = rig
        .drive(Launch::Running, |conn| async move {
            let snapshot = conn.until_runner_runs().await;
            assert!(snapshot.configuration.problems.is_empty());
            rig.mark_running();
            conn.ok("quit").await;
        })
        .await;
}
