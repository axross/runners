//! the token store behind the agent: setting and removing repository and
//! organization tokens, registration reads, and failures when the store cannot serve.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod support;

use marshal_cli::agent::machine::Launch;
use marshal_cli::agent::{CallError, Push};
use marshal_cli::runtime::log::Log;
use marshal_cli::token_store::{StoreError, StoreErrorKind, TokenKey, TokenStore};
use serde_json::json;
use support::agent::{ONE_POOL, OTHER_TOKEN, Raw, Rig, TOKEN, TWO_POOLS};

/// both pretty and compact debug formatting are potential secret sinks, before
/// and after request parsing.
#[test]
fn organization_token_requests_hide_the_secret_in_wire_and_parsed_debug_forms() {
    let params = json!({"organization": "Example-Org", "token": TOKEN});
    let wire: marshal_cli::agent::wire::ClientMessage = serde_json::from_value(json!({
        "type": "request", "id": 1, "method": "organization_token_set", "params": params,
    }))
    .unwrap();
    let parsed = marshal_cli::agent::api::parse("organization_token_set", params).unwrap();
    for text in [
        format!("{wire:?}"),
        format!("{wire:#?}"),
        format!("{parsed:?}"),
        format!("{parsed:#?}"),
    ] {
        assert!(!text.contains(TOKEN));
        assert!(text.contains("<redacted>"));
    }
}

/// case variants must address one organisation entry while the same owner's
/// repository token remains independent throughout replacement and removal.
#[tokio::test(flavor = "current_thread")]
async fn organization_tokens_are_normalized_replaced_and_removed_without_touching_repository_keys()
{
    let rig = Rig::new(ONE_POOL);
    let rig = &rig;
    rig.drive(Launch::Stopped, |conn| async move {
        let repository = TokenKey::parse("example-org/repo").unwrap();
        let organization = TokenKey::parse_organization("example-org").unwrap();
        assert_eq!(
            conn.call(
                "token_set",
                json!({"repository": "Example-Org/Repo", "token": TOKEN})
            )
            .await
            .unwrap(),
            json!({"repository": "example-org/repo"})
        );
        assert_eq!(
            conn.call(
                "organization_token_set",
                json!({"organization": "Example-Org", "token": format!("  {OTHER_TOKEN}  ")}),
            )
            .await
            .unwrap(),
            json!({"organization": "example-org"})
        );
        assert_eq!(rig.store.keys(), [organization.clone(), repository.clone()]);
        assert_eq!(
            rig.store.get(&organization).unwrap().unwrap().expose(),
            OTHER_TOKEN
        );
        assert_eq!(rig.store.get(&repository).unwrap().unwrap().expose(), TOKEN);
        conn.client
            .organization_token_set("EXAMPLE-ORG", TOKEN)
            .await
            .unwrap();
        assert_eq!(
            rig.store.get(&organization).unwrap().unwrap().expose(),
            TOKEN
        );
        assert_eq!(
            conn.call(
                "organization_token_remove",
                json!({"organization": "Example-Org"})
            )
            .await
            .unwrap(),
            json!({"organization": "example-org"})
        );
        conn.client
            .organization_token_remove("EXAMPLE-ORG")
            .await
            .unwrap();
        assert!(rig.store.get(&organization).unwrap().is_none());
        assert_eq!(rig.store.keys(), std::slice::from_ref(&repository));
        assert_eq!(rig.store.get(&repository).unwrap().unwrap().expose(), TOKEN);
        assert_eq!(
            conn.call("token_remove", json!({"repository": "Example-Org/Repo"}))
                .await
                .unwrap(),
            json!({"repository": "example-org/repo"})
        );
        assert!(rig.store.keys().is_empty());
        conn.ok("quit").await;
    })
    .await;
}

/// request errors must not echo input; the storage bound counts UTF-16 units, not
/// characters.
#[tokio::test(flavor = "current_thread")]
async fn organization_token_methods_refuse_malformed_params_without_repeating_them() {
    let rig = Rig::new(ONE_POOL);
    let rig = &rig;
    rig.drive(Launch::Stopped, |conn| async move {
        for method in ["organization_token_set", "organization_token_remove"] {
            for params in [
                json!(null),
                json!([]),
                json!({"token": TOKEN}),
                json!({"repository": "example-org/repo", "token": TOKEN}),
                json!({"organization": null, "token": TOKEN}),
                json!({"organization": 42, "token": TOKEN}),
                json!({"organization": "", "token": TOKEN}),
                json!({"organization": "example-org/repo", "token": TOKEN}),
                json!({"organization": " example-org ", "token": TOKEN}),
                json!({"organization": "-example-org", "token": TOKEN}),
                json!({"organization": "example-org\n", "token": TOKEN}),
            ] {
                let CallError::Agent(body) = conn.call(method, params).await.unwrap_err() else {
                    panic!("not an agent error");
                };
                assert_eq!(body.code, "invalid_request");
                assert!(!serde_json::to_string(&body).unwrap().contains(TOKEN));
            }
        }
        for params in [
            json!({"organization": "example-org"}),
            json!({"organization": "example-org", "token": null}),
            json!({"organization": "example-org", "token": 123_456_789}),
            json!({"organization": "example-org", "token": [TOKEN]}),
        ] {
            let CallError::Agent(body) = conn
                .call("organization_token_set", params)
                .await
                .unwrap_err()
            else {
                panic!("not an agent error");
            };
            assert_eq!(body.code, "invalid_request");
            let text = serde_json::to_string(&body).unwrap();
            assert!(!text.contains(TOKEN) && !text.contains("123456789"));
        }
        for token in [
            String::new(),
            "  ".to_string(),
            "x".repeat(1281),
            "😀".repeat(641),
        ] {
            let CallError::Agent(body) = conn
                .client
                .organization_token_set("example-org", &token)
                .await
                .unwrap_err()
            else {
                panic!("not an agent error");
            };
            assert_eq!(body.code, "token_invalid");
            assert!(!body.message.contains("xxxx") && !body.message.contains('😀'));
        }
        assert!(rig.store.keys().is_empty());
        let longest = "😀".repeat(640);
        conn.client
            .organization_token_set("example-org", &longest)
            .await
            .unwrap();
        let key = TokenKey::parse_organization("example-org").unwrap();
        assert_eq!(rig.store.get(&key).unwrap().unwrap().expose(), longest);
        conn.ok("quit").await;
    })
    .await;
}

/// callers need a stable store category and optional OS code, not the store's
/// private error text or the token involved in the failed operation.
#[tokio::test(flavor = "current_thread")]
async fn organization_token_store_failures_keep_the_kind_and_code_without_exposing_the_token() {
    let rig = Rig::new(ONE_POOL);
    let rig = &rig;
    rig.drive(Launch::Stopped, |conn| async move {
        for (kind, word, code) in [
            (StoreErrorKind::NeedsUser, "needs_user", Some(1312)),
            (StoreErrorKind::AccessRefused, "access_refused", Some(5)),
            (StoreErrorKind::Other, "other", None),
        ] {
            rig.store
                .fail_with_error(Some(StoreError::with(kind, code, "the vault said no")));
            for method in ["organization_token_set", "organization_token_remove"] {
                let CallError::Agent(body) = conn
                    .call(
                        method,
                        json!({"organization": "example-org", "token": TOKEN}),
                    )
                    .await
                    .unwrap_err()
                else {
                    panic!("not an agent error");
                };
                assert_eq!(body.code, "store_failed");
                let fault = body.store.as_ref().unwrap();
                assert_eq!((fault.kind.as_str(), fault.code), (word, code));
                assert!(body.message.contains(word));
                assert!(!serde_json::to_string(&body).unwrap().contains(TOKEN));
            }
            assert!(rig.store.keys().is_empty());
        }
        rig.store.fail_with_error(None);
        conn.client
            .organization_token_set("example-org", TOKEN)
            .await
            .unwrap();
        conn.client
            .organization_token_remove("example-org")
            .await
            .unwrap();
        conn.ok("quit").await;
    })
    .await;
}

/// a raw subscribed connection observes replies and events together, including
/// output emitted after removal when the log still needs its masking history.
#[tokio::test(flavor = "current_thread")]
async fn organization_tokens_are_absent_from_wire_output_logs_and_configuration_files() {
    let rig = Rig::new(ONE_POOL);
    let rig = &rig;
    rig.drive(Launch::Stopped, |conn| async move {
        let mut raw = Raw::hello(&conn.roots).await;
        raw.request(1, "subscribe", json!({"topic": "state"})).await;
        raw.answer().await;
        let event = raw.next().await.unwrap();
        assert_eq!(event["type"], "event");
        raw.request(
            2,
            "organization_token_set",
            json!({"organization": "Example-Org", "token": TOKEN}),
        )
        .await;
        assert_eq!(
            raw.answer().await["result"],
            json!({"organization": "example-org"})
        );
        rig.log.info(&format!("test output containing {TOKEN}"));
        assert!(rig.log.lines().iter().any(|line| line.contains("<masked>")));
        raw.request(3, "status", json!(null)).await;
        assert!(raw.answer().await["result"].is_object());
        raw.request(
            4,
            "organization_token_remove",
            json!({"organization": "EXAMPLE-ORG"}),
        )
        .await;
        assert_eq!(
            raw.answer().await["result"],
            json!({"organization": "example-org"})
        );
        raw.request(5, "quit", json!(null)).await;
        while raw.next().await.is_some() {}
        assert!(!raw.heard.contains(TOKEN));
        assert!(rig.log.lines().iter().all(|line| !line.contains(TOKEN)));
        assert!(rig.root.path().join("config.toml").is_file());
        for entry in std::fs::read_dir(rig.root.path()).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_file() {
                let bytes = std::fs::read(entry.path()).unwrap();
                assert!(!String::from_utf8_lossy(&bytes).contains(TOKEN));
            }
        }
    })
    .await;
}

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
            let reason = snapshot.pools[0].runners[0].reason.clone().unwrap();
            assert!(reason.contains("no token"), "{reason}");
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

/// an organisation token is saved, rotated between jobs and removed before the next
/// registration attempt, without affecting a repository pool's token.
#[tokio::test(flavor = "current_thread")]
async fn an_organization_token_starts_a_skipped_pool_and_rotates_without_crossing_repository_keys()
{
    let rig = Rig::new(ONE_POOL);
    rig.write_toml(
        r#"schema_version = 1
routing_label = "axpc"
image_name = "actions-runner:local"
[[pool]]
name = "r"
repository = "Example-Org/Repo"
labels = ["repo-only"]
runners = 1
[[pool]]
name = "org"
organization = "Example-Org"
runner_group = "selected-private"
labels = ["org-only"]
runners = 1
"#,
    );
    rig.store_token("example-org", "repo", TOKEN);
    rig.hold_jobs();
    let rig = &rig;
    rig.drive(Launch::Running, |conn| async move {
        conn.until_runner_runs().await;
        let before = conn.status().await;
        let org = before.pools.iter().find(|pool| pool.name == "org").unwrap();
        assert_eq!(org.state, "skipped");
        assert!(org.reason.as_deref().unwrap().contains("no token"));
        assert_eq!(before.organizations.len(), 1);
        assert_eq!(before.organizations[0].organization, "example-org");
        assert!(!before.organizations[0].has_token);
        conn.client
            .organization_token_set("EXAMPLE-ORG", OTHER_TOKEN)
            .await
            .unwrap();
        let started = conn
            .until(
                "both repository and organization pools running",
                |snapshot| snapshot.runners_up == 2,
            )
            .await;
        let org = started
            .pools
            .iter()
            .find(|pool| pool.name == "org")
            .unwrap();
        let wire = serde_json::to_value(org).unwrap();
        assert_eq!(wire["organization"], "example-org");
        assert_eq!(wire["runner_group"], "selected-private");
        assert!(wire.get("repository").is_none());
        assert_eq!(started.organizations[0].pools, ["org"]);
        assert!(started.organizations[0].has_token);
        conn.client
            .organization_token_set("example-org", "replacement-org-token")
            .await
            .unwrap();
        rig.end_jobs().await;
        conn.eventually("the new organization token reaches registration", || {
            rig.registered_with()
                .iter()
                .any(|token| token == "replacement-org-token")
        })
        .await;
        conn.until("both pools running after rotation", |snapshot| {
            snapshot.runners_up == 2
        })
        .await;
        conn.client
            .organization_token_remove("example-org")
            .await
            .unwrap();
        let removed_at = rig.registrar.seen.borrow().len();
        rig.end_jobs().await;
        let removed = conn
            .until(
                "the organization runner backs off without a token",
                |snapshot| {
                    snapshot
                        .pools
                        .iter()
                        .find(|pool| pool.name == "org")
                        .and_then(|pool| pool.runners.first())
                        .is_some_and(|runner| {
                            runner.state == "backing_off"
                                && runner
                                    .reason
                                    .as_deref()
                                    .is_some_and(|cause| cause.contains("no token"))
                        })
                },
            )
            .await;
        assert!(!removed.organizations[0].has_token);
        assert!(removed.repositories[0].has_token);
        assert!(
            rig.registrar
                .seen
                .borrow()
                .iter()
                .skip(removed_at)
                .all(|registration| {
                    matches!(
                        registration.target,
                        marshal_cli::validate::PoolTarget::Repository { .. }
                    )
                })
        );
        for snapshot in [&before, &started, &removed] {
            let encoded = serde_json::to_string(snapshot).unwrap();
            for token in [TOKEN, OTHER_TOKEN, "replacement-org-token"] {
                assert!(!encoded.contains(token));
                assert!(rig.log.lines().iter().all(|line| !line.contains(token)));
            }
        }
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

/// the reason and the warning of an admission skip, word for word, so that the agent's
/// admission and `start`'s say the same thing.
#[tokio::test(flavor = "current_thread")]
async fn a_pool_skipped_for_want_of_a_token_says_why_in_the_snapshot_and_the_log() {
    for (store_error, why) in [
        (None, "no token"),
        (
            Some(StoreError::with(
                StoreErrorKind::NeedsUser,
                Some(1312),
                "the vault is locked",
            )),
            "the token store failed: the vault is locked (kind needs_user, code 1312)",
        ),
    ] {
        let rig = Rig::new(ONE_POOL);
        rig.store.fail_with_error(store_error);
        let rig = &rig;
        let (_, ()) = rig
            .drive(Launch::Running, |conn| async move {
                let snapshot = conn.until_state("stopped").await;
                assert_eq!(
                    snapshot.pools[0].reason.as_deref(),
                    Some(format!("pool[0]: {why}.").as_str())
                );
                conn.ok("quit").await;
            })
            .await;
        let expected =
            format!("WARN: pool[0] (o/r) is not started and gets no slots: pool[0]: {why}.");
        assert!(
            rig.log.lines().contains(&expected),
            "{expected}\n{:?}",
            rig.log.lines()
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_pool_skipped_because_the_store_failed_names_the_stores_kind_and_code() {
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
async fn a_registration_whose_store_fails_names_the_kind_and_code_in_the_runner_and_the_log() {
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
            let reason = snapshot.pools[0].runners[0].reason.clone().unwrap();
            assert!(reason.contains("access_refused"), "{reason}");
            assert!(reason.contains("code 5"), "{reason}");
            assert!(!reason.contains(TOKEN));
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
            assert_eq!(
                conn.code(
                    "organization_token_set",
                    json!({"organization": "example-org", "token": TOKEN})
                )
                .await,
                "store_failed"
            );
            assert_eq!(
                conn.code(
                    "organization_token_remove",
                    json!({"organization": "example-org"})
                )
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

#[tokio::test(flavor = "current_thread")]
async fn a_github_rejection_is_the_reason_of_the_runner_that_backs_off() {
    let rig = Rig::new(ONE_POOL);
    rig.store_token("o", "r", TOKEN);
    rig.hold_jobs();
    let rig = &rig;
    let (_, ()) = rig
        .drive(Launch::Running, |conn| async move {
            conn.until_runner_runs().await;
            rig.registrar.results.borrow_mut().push_front(Err(
                marshal_cli::runtime::registration::RegistrationError::new(
                    "GitHub answered 403: Resource not accessible by personal access token",
                ),
            ));
            rig.end_jobs().await;
            let snapshot = conn
                .until("the runner backing off", |s| {
                    s.pools
                        .first()
                        .and_then(|pool| pool.runners.first())
                        .is_some_and(|runner| runner.state == "backing_off")
                })
                .await;
            let runner = &snapshot.pools[0].runners[0];
            let reason = runner.reason.clone().unwrap();
            assert!(reason.contains("403"), "{reason}");
            assert!(reason.contains("Resource not accessible"), "{reason}");
            assert!(!reason.contains("docker"), "{reason}");
            // it registers again after the backoff, and the reason goes with the state
            conn.until_runner_runs().await;
            assert!(conn.status().await.pools[0].runners[0].reason.is_none());
            rig.mark_running();
            conn.ok("quit").await;
        })
        .await;
}
