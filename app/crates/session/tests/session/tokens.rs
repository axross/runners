//! the token path: a token entered in the window reaches `token_set` once, trimmed and
//! within the library's limits, and the app keeps no copy of it.

use marshal_session::{Failure, Forms, Method};
use marshal_view::TokenState;

use crate::support::{Agent, Answer, Called, Running, World, fixture, secs, start, what};

/// a token-shaped value built at run time, so no file holds one.
fn fake_token(tail: &str) -> String {
    format!("{}{}_{}", "gh", "p", tail)
}

async fn connected(name: &str) -> (Agent, Running) {
    let (agent, link) = Agent::new();
    let world = World::default();
    world.then(Answer::Agent(link));
    let session = start(world);
    agent.push(fixture(name));
    session.settle().await;
    (agent, session)
}

/// everything the app shows or logs, as text.
fn everything(session: &Running) -> String {
    format!(
        "{:?}\n{}\n{}\n{:?}",
        session.screen.shown(),
        serde_json::to_string(&session.screen.shown()).unwrap(),
        session.log.lines().join("\n"),
        session.screen.announced()
    )
}

fn row<'a>(view: &'a marshal_view::View, repository: &str) -> &'a marshal_view::TokenRow {
    view.list()
        .unwrap()
        .tokens
        .rows
        .iter()
        .find(|row| row.repository == repository)
        .unwrap()
}

#[tokio::test(start_paused = true)]
async fn a_token_reaches_the_agent_in_token_set_trimmed_and_the_app_keeps_no_copy() {
    let (agent, session) = connected("running").await;
    let token = fake_token("abcdefghijklmnop1234");
    session
        .handle
        .set_token("Owner/Docs".to_string(), format!("  {token}\r\n"))
        .await
        .unwrap();
    session.settle().await;
    assert_eq!(
        agent.calls(),
        [Called::TokenSet {
            repository: "owner/docs".to_string(),
            token: token.clone(),
        }]
    );
    let text = everything(&session);
    assert!(
        !text.contains(&token),
        "the token is in what the app shows or logs"
    );
    assert!(!text.contains("abcdefghijklmnop1234"));
    // and in none of the session's own state
    session.wait(secs(60)).await;
    assert!(!everything(&session).contains("abcdefghijklmnop1234"));
}

#[tokio::test(start_paused = true)]
async fn a_saved_token_says_which_pool_it_started() {
    let (_agent, session) = connected("running").await;
    session
        .handle
        .set_token("owner/docs".to_string(), fake_token("abcdefghijklmnop1234"))
        .await
        .unwrap();
    session.settle().await;
    let status = row(&session.screen.view(), "owner/docs")
        .status
        .clone()
        .unwrap();
    assert_eq!(status, "Token for owner/docs saved. Starting pool docs.");
    assert!(
        session
            .screen
            .announced()
            .contains(&"Token for owner/docs saved. Starting pool docs.".to_string())
    );
}

#[tokio::test(start_paused = true)]
async fn an_empty_token_is_refused_before_anything_is_sent() {
    let (agent, session) = connected("running").await;
    session
        .handle
        .set_token("owner/docs".to_string(), "  \t ".to_string())
        .await
        .unwrap();
    session.settle().await;
    assert!(agent.calls().is_empty());
    let status = row(&session.screen.view(), "owner/docs")
        .status
        .clone()
        .unwrap();
    assert!(status.contains("the token is empty"), "{status}");
}

#[tokio::test(start_paused = true)]
async fn a_token_over_1280_utf16_code_units_is_refused_and_one_of_1280_is_sent() {
    let (agent, session) = connected("running").await;
    // a character outside the basic plane counts as two units
    let too_long = format!("{}{}", "a".repeat(1279), "\u{1F600}");
    assert_eq!(too_long.encode_utf16().count(), 1281);
    session
        .handle
        .set_token("owner/docs".to_string(), too_long.clone())
        .await
        .unwrap();
    session.settle().await;
    assert!(agent.calls().is_empty(), "an over-long token is not sent");
    let status = row(&session.screen.view(), "owner/docs")
        .status
        .clone()
        .unwrap();
    assert!(status.contains("at most 1,280"), "{status}");
    assert!(!everything(&session).contains(&"a".repeat(40)));

    let just_fits = format!("{}\u{1F600}", "a".repeat(1278));
    assert_eq!(just_fits.encode_utf16().count(), 1280);
    session
        .handle
        .set_token("owner/docs".to_string(), just_fits.clone())
        .await
        .unwrap();
    assert_eq!(
        agent.calls(),
        [Called::TokenSet {
            repository: "owner/docs".to_string(),
            token: just_fits,
        }]
    );
}

#[tokio::test(start_paused = true)]
async fn a_repository_that_is_not_owner_and_name_is_an_error_and_sends_nothing() {
    let (agent, session) = connected("running").await;
    assert!(
        session
            .handle
            .set_token("docs".to_string(), fake_token("abcdefghijklmnop1234"))
            .await
            .is_err()
    );
    assert!(
        session
            .handle
            .remove_token("../x".to_string())
            .await
            .is_err()
    );
    assert!(agent.calls().is_empty());
}

#[tokio::test(start_paused = true)]
async fn a_token_the_store_refuses_keeps_the_form_open_with_the_kind_and_code() {
    let (agent, session) = connected("running").await;
    agent.answer(
        "token_set",
        Err(Failure::Store {
            detail: Some("kind other, code 1312".to_string()),
        }),
    );
    session
        .handle
        .read_view(Some(Forms {
            editing: Some("owner/docs".to_string()),
            removing: None,
        }))
        .await
        .unwrap();
    let token = fake_token("abcdefghijklmnop1234");
    session
        .handle
        .set_token("owner/docs".to_string(), token.clone())
        .await
        .unwrap();
    session.settle().await;
    let view = session.screen.view();
    let form = row(&view, "owner/docs")
        .form
        .clone()
        .expect("the form stays open");
    let error = form.error.unwrap();
    assert!(error.contains("Not saved"), "{error}");
    assert!(error.contains("kind other, code 1312"), "{error}");
    assert!(!everything(&session).contains(&token));
}

#[tokio::test(start_paused = true)]
async fn a_saved_token_closes_its_form() {
    let (_agent, session) = connected("running").await;
    session
        .handle
        .read_view(Some(Forms {
            editing: Some("owner/docs".to_string()),
            removing: None,
        }))
        .await
        .unwrap();
    assert!(row(&session.screen.view(), "owner/docs").form.is_some());
    session
        .handle
        .set_token("owner/docs".to_string(), fake_token("abcdefghijklmnop1234"))
        .await
        .unwrap();
    session.settle().await;
    assert!(row(&session.screen.view(), "owner/docs").form.is_none());
}

#[tokio::test(start_paused = true)]
async fn removing_a_token_asks_the_agent_and_closes_the_question() {
    let (agent, session) = connected("running").await;
    session
        .handle
        .read_view(Some(Forms {
            editing: None,
            removing: Some("owner/app".to_string()),
        }))
        .await
        .unwrap();
    assert!(row(&session.screen.view(), "owner/app").confirming_removal);
    session
        .handle
        .remove_token("Owner/App".to_string())
        .await
        .unwrap();
    session.settle().await;
    assert_eq!(
        agent.calls(),
        [Called::TokenRemove {
            repository: "owner/app".to_string()
        }]
    );
    let view = session.screen.view();
    let app = row(&view, "owner/app");
    assert!(!app.confirming_removal);
    assert_eq!(app.status.as_deref(), Some("Token for owner/app removed."));
    assert_ne!(
        app.state,
        TokenState::Missing,
        "the snapshot decides the state"
    );
}

#[tokio::test(start_paused = true)]
async fn a_removal_the_store_refuses_says_the_token_was_not_removed() {
    let (agent, session) = connected("running").await;
    agent.answer(
        "token_remove",
        Err(Failure::Store {
            detail: Some("kind other, code 1312".to_string()),
        }),
    );
    session
        .handle
        .remove_token("owner/app".to_string())
        .await
        .unwrap();
    session.settle().await;
    let view = session.screen.view();
    let status = row(&view, "owner/app").status.clone().unwrap_or_default();
    assert!(status.contains("not removed"), "{status}");
    assert!(status.contains("It is still stored"), "{status}");
    assert!(!status.contains("Not saved"), "{status}");
}

/// a token save, or a removal, that the agent holds past the call's 10 s: the row says
/// the agent has not answered yet and leaves the token's state to the snapshot.
async fn overrun(removal: bool) {
    let (method, verb) = if removal {
        ("token_remove", "removing")
    } else {
        ("token_set", "saving")
    };
    let (agent, session) = connected("running").await;
    let gate = agent.hold(method);
    session
        .handle
        .read_view(Some(Forms {
            editing: (!removal).then(|| "owner/app".to_string()),
            removing: removal.then(|| "owner/app".to_string()),
        }))
        .await
        .unwrap();
    let handle = session.handle.clone();
    let call = tokio::spawn(async move {
        if removal {
            handle.remove_token("owner/app".to_string()).await
        } else {
            handle
                .set_token("owner/app".to_string(), fake_token("abcdefghijklmnop1234"))
                .await
        }
    });
    session.settle().await;
    session.wait(secs(11)).await;
    call.await.unwrap().unwrap();
    let expected = format!(
        "The agent has not answered yet about {verb} the token for owner/app. It may still do it; this row shows where the token stands."
    );
    let view = session.screen.view();
    let app = row(&view, "owner/app");
    assert_eq!(app.status.as_deref(), Some(expected.as_str()));
    assert_eq!(
        app.state,
        TokenState::Set,
        "the snapshot decides, not the call"
    );
    assert!(
        app.form.is_none() && !app.confirming_removal,
        "nothing asks again"
    );
    assert!(
        session.screen.announced().contains(&expected),
        "{:?}",
        session.screen.announced()
    );
    assert_eq!(what(&session.screen.view()), "list", "contact is not lost");
    drop(gate);
}

#[tokio::test(start_paused = true)]
async fn a_save_that_takes_too_long_says_the_agent_has_not_answered_yet() {
    overrun(false).await;
}

#[tokio::test(start_paused = true)]
async fn a_removal_that_takes_too_long_says_the_agent_has_not_answered_yet() {
    overrun(true).await;
}

#[tokio::test(start_paused = true)]
async fn a_removal_without_a_connection_says_the_token_was_not_removed() {
    let (agent, session) = connected("running").await;
    agent.close();
    session.settle().await;
    session
        .handle
        .remove_token("owner/app".to_string())
        .await
        .unwrap();
    session.settle().await;
    let announced = session.screen.announced();
    assert!(
        announced
            .iter()
            .any(|said| said.starts_with("Token for owner/app not removed")
                && said.ends_with("It is still stored.")),
        "{announced:?}"
    );
}

#[tokio::test(start_paused = true)]
async fn a_token_entered_without_a_connection_is_not_saved_and_not_kept() {
    let session = start(World::default());
    session.wait(secs(30)).await;
    let token = fake_token("abcdefghijklmnop1234");
    session
        .handle
        .set_token("owner/docs".to_string(), token.clone())
        .await
        .unwrap();
    session.settle().await;
    assert!(!everything(&session).contains(&token));
}

#[tokio::test(start_paused = true)]
async fn a_form_that_opens_while_another_is_open_discards_the_first_and_says_so() {
    let (_agent, session) = connected("running").await;
    let before = session.screen.announced().len();
    for editing in ["owner/docs", "owner/app"] {
        session
            .handle
            .read_view(Some(Forms {
                editing: Some(editing.to_string()),
                removing: None,
            }))
            .await
            .unwrap();
    }
    assert_eq!(
        session.screen.announced().split_off(before),
        ["The token for owner/docs was not saved."]
    );
}

#[tokio::test(start_paused = true)]
async fn contact_lost_with_a_form_open_says_the_token_was_not_saved() {
    let (agent, session) = connected("running").await;
    session
        .handle
        .read_view(Some(Forms {
            editing: Some("owner/docs".to_string()),
            removing: None,
        }))
        .await
        .unwrap();
    agent.close();
    session.settle().await;
    assert!(
        session.screen.announced().iter().any(|said| said
            == "The token for owner/docs was not saved. Marshal has lost contact with the agent."),
        "{:?}",
        session.screen.announced()
    );
}

#[tokio::test(start_paused = true)]
async fn the_view_read_by_the_page_is_the_one_shown() {
    let (_agent, session) = connected("running").await;
    let read = session.handle.read_view(None).await.unwrap();
    assert_eq!(read.view, session.screen.view());
    assert!(read.announce.is_empty());
}

/// an organization-owned repository still needs a separate repository token.
#[tokio::test(start_paused = true)]
async fn organization_and_repository_tokens_keep_separate_keys_and_methods() {
    let (agent, session) = connected("running").await;
    let token = fake_token("abcdefghijklmnop1234");
    let method = Method::OrganizationTokenSet {
        organization: "owner".to_string(),
        token: marshal_cli::runtime::secret::Secret::new(token.clone()),
    };
    for debug in [format!("{method:?}"), format!("{method:#?}")] {
        assert!(!debug.contains(&token));
        assert!(debug.contains("<redacted>"));
    }
    session
        .handle
        .set_organization_token("OwNeR".into(), format!(" {token}\r\n"))
        .await
        .unwrap();
    session
        .handle
        .set_token("Owner/Docs".into(), token.clone())
        .await
        .unwrap();
    session
        .handle
        .remove_organization_token("OWNER".into())
        .await
        .unwrap();
    session
        .handle
        .remove_token("Owner/Docs".into())
        .await
        .unwrap();
    session.settle().await;
    assert_eq!(
        agent.calls(),
        [
            Called::OrganizationTokenSet {
                organization: "owner".into(),
                token: token.clone()
            },
            Called::TokenSet {
                repository: "owner/docs".into(),
                token: token.clone()
            },
            Called::OrganizationTokenRemove {
                organization: "owner".into()
            },
            Called::TokenRemove {
                repository: "owner/docs".into()
            },
        ]
    );
    assert!(
        session
            .screen
            .announced()
            .contains(&"Token for owner saved.".into())
    );
    assert!(
        session
            .screen
            .announced()
            .contains(&"Token for owner removed.".into())
    );
    session.wait(secs(60)).await;
    assert!(!everything(&session).contains(&token));
}

/// input checks do not depend on the agent or on a valid configuration.
#[tokio::test(start_paused = true)]
async fn organization_keys_and_token_limits_are_checked_before_sending() {
    let (agent, session) = connected("running").await;
    for key in ["", "owner/docs", "../x", "-org", "o rg", "org_x"] {
        assert!(
            session
                .handle
                .set_organization_token(key.into(), fake_token("abcdefghijklmnop1234"))
                .await
                .is_err()
        );
        assert!(
            session
                .handle
                .remove_organization_token(key.into())
                .await
                .is_err()
        );
    }
    let too_long = format!("{}😀", "a".repeat(1279));
    for token in [" \t ".to_string(), too_long.clone()] {
        session
            .handle
            .set_organization_token("Owner".into(), token)
            .await
            .unwrap();
    }
    session.settle().await;
    assert!(agent.calls().is_empty());
    let announced = session.screen.announced();
    assert!(
        announced
            .iter()
            .any(|line| line.contains("the token is empty"))
    );
    assert!(announced.iter().any(|line| line.contains("at most 1,280")));
    assert!(!everything(&session).contains(&too_long));
    let just_fits = format!("{}😀", "a".repeat(1278));
    session
        .handle
        .set_organization_token("Owner".into(), just_fits.clone())
        .await
        .unwrap();
    assert_eq!(
        agent.calls(),
        [Called::OrganizationTokenSet {
            organization: "owner".into(),
            token: just_fits,
        }]
    );
    assert!(!everything(&session).contains(&"a".repeat(40)));
}

/// saving repairs a missing or unreadable credential, not a failed store or an already
/// running pool.
#[tokio::test(start_paused = true)]
async fn an_organization_save_announces_only_its_pools_skipped_for_want_of_a_readable_token() {
    let (agent, session) = connected("running").await;
    let mut snapshot = fixture("running");
    let missing = snapshot
        .pools
        .iter()
        .find(|pool| pool.name == "docs")
        .unwrap()
        .clone();
    for (name, organization, state, reason) in [
        ("org-one", "owner", "skipped", "pool[3]: no token."),
        ("org-two", "owner", "skipped", "repositories[4]: no token."),
        ("other-org", "other", "skipped", "pool[5]: no token."),
        (
            "org-unreadable",
            "owner",
            "skipped",
            "pool[7]: the token is stored but could not be read.",
        ),
        (
            "store-unavailable",
            "owner",
            "skipped",
            "pool[6]: no token store.",
        ),
        ("already-running", "owner", "running", "no token"),
    ] {
        let mut pool = missing.clone();
        pool.name = name.into();
        pool.repository.clear();
        pool.organization = Some(organization.into());
        pool.state = state.into();
        pool.reason = Some(reason.into());
        snapshot.pools.push(pool);
    }
    assert_eq!(
        marshal_view::pools_started_by(&snapshot, "owner/docs"),
        ["docs"]
    );
    assert_eq!(
        marshal_view::pools_started_by(&snapshot, "owner"),
        ["org-one", "org-two", "org-unreadable"]
    );
    agent.push(snapshot.clone());
    session.settle().await;
    session
        .handle
        .set_organization_token("OWNER".into(), fake_token("abcdefghijklmnop1234"))
        .await
        .unwrap();
    session.settle().await;
    assert!(session.screen.announced().contains(
        &"Token for owner saved. Starting pools org-one, org-two and org-unreadable.".into()
    ));
    snapshot.agent.state = "stopped".into();
    assert!(marshal_view::pools_started_by(&snapshot, "owner").is_empty());
    agent.push(snapshot);
    session.settle().await;
    session
        .handle
        .set_organization_token("owner".into(), fake_token("abcdefghijklmnop1234"))
        .await
        .unwrap();
    session.settle().await;
    assert!(
        session
            .screen
            .announced()
            .contains(&"Token for owner saved.".into())
    );
}

/// an Ok reply acknowledges session handling, not successful storage.
#[tokio::test(start_paused = true)]
async fn organization_token_failures_use_session_feedback_without_echoing_secrets() {
    let token = fake_token("abcdefghijklmnop1234");
    for (failure, expected) in [
        (
            Failure::Store {
                detail: Some("kind other, code 1312".into()),
            },
            "kind other, code 1312",
        ),
        (
            Failure::Refused(format!("the agent refused {token}")),
            "the agent refused <token>",
        ),
        (Failure::Lost, "lost contact"),
        (
            Failure::NotServed("organization_token_set".into()),
            "does not offer organization_token_set",
        ),
    ] {
        let (agent, session) = connected("running").await;
        agent.answer("organization_token_set", Err(failure.clone()));
        agent.answer("organization_token_remove", Err(failure));
        session
            .handle
            .set_organization_token("Owner".into(), token.clone())
            .await
            .unwrap();
        session
            .handle
            .remove_organization_token("OWNER".into())
            .await
            .unwrap();
        session.settle().await;
        let announced = session.screen.announced();
        assert!(
            announced.iter().any(|line| line.contains(expected)),
            "{announced:?}"
        );
        assert!(
            announced
                .iter()
                .any(|line| line.starts_with("Token for owner not saved.")
                    || line.starts_with("The token for owner was not saved.")),
            "{announced:?}"
        );
        assert!(
            announced
                .iter()
                .any(|line| line.starts_with("Token for owner not removed.")
                    && line.ends_with("It is still stored.")),
            "{announced:?}"
        );
        assert!(
            !announced
                .iter()
                .any(|line| line == "Token for owner saved." || line == "Token for owner removed.")
        );
        assert!(!everything(&session).contains(&token));
    }
}

/// an overrun leaves the outcome to later snapshots and is not a failed connection.
#[tokio::test(start_paused = true)]
async fn organization_token_timeouts_are_not_lost_contact_or_success() {
    for removal in [false, true] {
        let (agent, session) = connected("running").await;
        let method = if removal {
            "organization_token_remove"
        } else {
            "organization_token_set"
        };
        let gate = agent.hold(method);
        let handle = session.handle.clone();
        let call = tokio::spawn(async move {
            if removal {
                handle.remove_organization_token("OWNER".into()).await
            } else {
                handle
                    .set_organization_token("Owner".into(), fake_token("abcdefghijklmnop1234"))
                    .await
            }
        });
        session.settle().await;
        session.wait(secs(11)).await;
        call.await.unwrap().unwrap();
        let verb = if removal { "removing" } else { "saving" };
        assert!(session.screen.announced().contains(&format!("The agent has not answered yet about {verb} the token for owner. It may still do it; this row shows where the token stands.")));
        assert_eq!(what(&session.screen.view()), "list");
        assert!(!everything(&session).contains("abcdefghijklmnop1234"));
        drop(gate);
    }
}

/// an open form and a subsequent call each need their own lost-contact feedback.
#[tokio::test(start_paused = true)]
async fn disconnected_organization_token_calls_and_an_open_form_report_lost_contact() {
    let (agent, session) = connected("running").await;
    session
        .handle
        .read_view(Some(Forms {
            editing: Some("owner".into()),
            removing: None,
        }))
        .await
        .unwrap();
    agent.close();
    session.settle().await;
    session
        .handle
        .set_organization_token("OWNER".into(), fake_token("abcdefghijklmnop1234"))
        .await
        .unwrap();
    session
        .handle
        .remove_organization_token("Owner".into())
        .await
        .unwrap();
    session.settle().await;
    let announced = session.screen.announced();
    assert!(
        announced
            .iter()
            .filter(|line| *line
                == "The token for owner was not saved. Marshal has lost contact with the agent.")
            .count()
            >= 2
    );
    assert!(
        announced
            .iter()
            .any(|line| line.starts_with("Token for owner not removed.")
                && line.contains("lost contact"))
    );
    assert!(agent.calls().is_empty());
    assert!(!everything(&session).contains("abcdefghijklmnop1234"));
}

/// exercises the real guarded dispatch without a resident agent or token store.
#[cfg(all(unix, debug_assertions))]
#[tokio::test]
async fn the_library_link_dispatches_organization_methods_only_when_welcome_lists_each_one() {
    use std::os::unix::fs::DirBuilderExt;

    use marshal_cli::instance::roots::Roots;
    use marshal_cli::runtime::secret::Secret;
    use marshal_session::{Dial, Library, Link};
    use serde_json::json;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::UnixListener;

    for (set, remove) in [(false, false), (true, false), (false, true), (true, true)] {
        // macOS's $TMPDIR is long enough to push the socket past sun_path's 104 bytes.
        let directory = std::path::Path::new("/tmp").join(format!("ms-ow-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let roots = Roots::for_tests(&directory);
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(roots.channel_directory())
            .unwrap();
        let listener = UnixListener::bind(roots.socket_path()).unwrap();
        let token = fake_token("abcdefghijklmnop1234");
        let expected_token = token.clone();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let (read, mut write) = stream.into_split();
            let mut lines = BufReader::new(read).lines();
            let hello: serde_json::Value =
                serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
            assert_eq!(hello["type"], "hello");
            let methods: Vec<_> = [
                set.then_some("organization_token_set"),
                remove.then_some("organization_token_remove"),
            ]
            .into_iter()
            .flatten()
            .collect();
            write
                .write_all(
                    format!(
                        "{}\n",
                        json!({"type": "welcome", "version": 2, "methods": methods})
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
            let mut called = Vec::new();
            while let Some(line) = lines.next_line().await.unwrap() {
                let request: serde_json::Value = serde_json::from_str(&line).unwrap();
                let method = request["method"].as_str().unwrap().to_string();
                let expected = match method.as_str() {
                    "organization_token_set" => {
                        json!({"organization": "owner", "token": expected_token})
                    }
                    "organization_token_remove" => json!({"organization": "owner"}),
                    other => panic!("unexpected method: {other}"),
                };
                assert_eq!(request["params"], expected);
                called.push(method);
                write.write_all(format!("{}\n", json!({"type": "response", "id": request["id"], "result": {"organization": "owner"}})).as_bytes()).await.unwrap();
            }
            called
        });
        let connected = Library::new(roots, "0.0.0-test").dial().await.unwrap();
        let saved = tokio::time::timeout(
            secs(2),
            connected.link.call(Method::OrganizationTokenSet {
                organization: "owner".into(),
                token: Secret::new(token),
            }),
        )
        .await
        .unwrap();
        let removed = tokio::time::timeout(
            secs(2),
            connected.link.call(Method::OrganizationTokenRemove {
                organization: "owner".into(),
            }),
        )
        .await
        .unwrap();
        assert_eq!(
            saved,
            if set {
                Ok(())
            } else {
                Err(Failure::NotServed("organization_token_set".into()))
            }
        );
        assert_eq!(
            removed,
            if remove {
                Ok(())
            } else {
                Err(Failure::NotServed("organization_token_remove".into()))
            }
        );
        drop(connected);
        let called = tokio::time::timeout(secs(2), server)
            .await
            .unwrap()
            .unwrap();
        let expected: Vec<_> = [
            set.then_some("organization_token_set"),
            remove.then_some("organization_token_remove"),
        ]
        .into_iter()
        .flatten()
        .collect();
        assert_eq!(called, expected);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
