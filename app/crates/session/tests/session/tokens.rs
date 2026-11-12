//! the token path: a token entered in the window reaches `token_set` once, trimmed and
//! within the library's limits, and the app keeps no copy of it.

use marshal_session::{Failure, Forms};
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
