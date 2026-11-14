//! the settings path: a change in the window reaches `lifecycle_set` once, the other
//! settings wait until it is answered, and what came of it is shown under the setting
//! and announced.

use marshal_cli::agent::CallError;
use marshal_cli::agent::config::Problem;
use marshal_cli::agent::wire::{ErrorBody, ErrorCode};
use marshal_session::{Failure, failure};
use serde_json::json;

use crate::support::{Agent, Answer, Called, Running, World, patched, start};

async fn connected() -> (Agent, Running) {
    let (agent, link) = Agent::new();
    let world = World::default();
    world.then(Answer::Agent(link));
    let session = start(world);
    agent.push(patched("running", |value| {
        value["configuration"]["status"] = json!("valid");
        value["lifecycle"] = json!({
            "settings": {
                "keep_awake": { "on": true, "defaulted": true }
            },
            "keep_awake": "held"
        });
    }));
    session.settle().await;
    (agent, session)
}

fn settings(session: &Running) -> marshal_view::SettingsTab {
    session.screen.view().list().unwrap().settings.clone()
}

fn note(session: &Running, key: &str) -> Option<String> {
    settings(session)
        .groups
        .iter()
        .flat_map(|group| &group.rows)
        .find(|row| row.key == key)
        .and_then(|row| row.note.as_ref().map(|note| note.text.clone()))
}

#[tokio::test(start_paused = true)]
async fn a_change_reaches_lifecycle_set_and_holds_the_others_until_it_is_answered() {
    let (agent, session) = connected().await;
    let gate = agent.hold("lifecycle_set");
    let handle = session.handle.clone();
    let call =
        tokio::spawn(async move { handle.set_setting("keep_awake".to_string(), false).await });
    session.settle().await;
    assert_eq!(
        agent.calls(),
        [Called::LifecycleSet {
            key: "keep_awake".to_string(),
            on: false,
        }]
    );
    assert!(settings(&session).unavailable.is_some());
    // a second change while the first is out is refused, and nothing more is sent
    assert!(
        session
            .handle
            .set_setting("stop_on_network_change".to_string(), false)
            .await
            .is_err()
    );
    gate.notify_one();
    call.await.unwrap().unwrap();
    session.settle().await;
    assert_eq!(agent.calls().len(), 1);
    assert_eq!(settings(&session).unavailable, None);
    assert!(
        session
            .screen
            .announced()
            .contains(&"Saved: \"Keep the computer awake while runners run\" is off.".to_string())
    );
}

#[tokio::test(start_paused = true)]
async fn a_change_refused_because_the_file_changed_is_shown_under_the_setting_and_announced() {
    let (agent, session) = connected().await;
    agent.answer(
        "lifecycle_set",
        Err(Failure::Config {
            code: "config_changed".to_string(),
            message: "config.toml changed after the agent read it; Start all reads it again"
                .to_string(),
        }),
    );
    session
        .handle
        .set_setting("keep_awake".to_string(), false)
        .await
        .unwrap();
    session.settle().await;
    let shown = note(&session, "keep_awake").expect("a note");
    assert!(
        shown.starts_with("Not saved: config.toml changed"),
        "{shown}"
    );
    assert!(session.screen.announced().contains(&shown));
}

#[tokio::test(start_paused = true)]
async fn a_key_that_is_not_a_setting_is_refused_before_anything_is_sent() {
    let (agent, session) = connected().await;
    assert!(
        session
            .handle
            .set_setting("token_path".to_string(), true)
            .await
            .is_err()
    );
    assert!(agent.calls().is_empty());
}

#[tokio::test(start_paused = true)]
async fn a_refusal_stands_until_the_lifecycle_part_changes_or_start_all_reads_the_file_again() {
    let (agent, session) = connected().await;
    let refuse = || {
        Err(Failure::Config {
            code: "config_changed".to_string(),
            message: "config.toml changed after the agent read it".to_string(),
        })
    };
    agent.answer("lifecycle_set", refuse());
    session
        .handle
        .set_setting("keep_awake".to_string(), false)
        .await
        .unwrap();
    session.settle().await;
    // a snapshot that changes something else leaves it
    agent.push(patched("running", |value| {
        value["configuration"]["status"] = json!("valid");
        value["runners_up"] = json!(0);
        value["lifecycle"] = json!({
            "settings": { "keep_awake": { "on": true, "defaulted": true } },
            "keep_awake": "held"
        });
    }));
    session.settle().await;
    assert!(note(&session, "keep_awake").is_some());
    // the lifecycle part changing clears it
    agent.push(patched("running", |value| {
        value["configuration"]["status"] = json!("valid");
        value["lifecycle"] = json!({
            "settings": { "keep_awake": { "on": false, "defaulted": false } },
            "keep_awake": "off"
        });
    }));
    session.settle().await;
    assert_eq!(note(&session, "keep_awake"), None);
    // and so does Start all
    agent.answer("lifecycle_set", refuse());
    session
        .handle
        .set_setting("keep_awake".to_string(), true)
        .await
        .unwrap();
    session.settle().await;
    assert!(note(&session, "keep_awake").is_some());
    session
        .handle
        .act(marshal_session::Act::StartAll)
        .await
        .ok();
    session.settle().await;
    assert_eq!(note(&session, "keep_awake"), None);
}

#[test]
fn a_refusal_for_problems_keeps_them_as_the_snapshot_words_them() {
    let mut body = ErrorBody::new(ErrorCode::ConfigInvalid, "the change breaks the rules.");
    body.problems = vec![
        Problem {
            path: Some("lifecycle.keep_awake".to_string()),
            line: Some(12),
            message: "must be true or false".to_string(),
        },
        Problem {
            path: None,
            line: Some(3),
            message: "expected a table".to_string(),
        },
    ]
    .into();
    assert_eq!(
        failure(CallError::Agent(body)),
        Failure::Config {
            code: "config_invalid".to_string(),
            message: "the change breaks the rules (line 12: lifecycle.keep_awake: must be true \
                      or false; line 3: expected a table)"
                .to_string(),
        }
    );
}
