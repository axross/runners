//! the settings path: a change in the window reaches `lifecycle_set` once, the other
//! settings wait until it is answered, and what came of it is shown under the setting
//! and announced.

use marshal_cli::agent::config::{ConfigChange, Problem};
use marshal_session::{ConfigAnswer, ConfigRequest, Failure};
use serde_json::json;

use crate::support::{Agent, Answer, Called, Running, World, patched, secs, start};

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
            problems: Vec::new(),
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
            problems: Vec::new(),
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

#[tokio::test(start_paused = true)]
async fn a_refusal_for_problems_keeps_them_as_the_snapshot_words_them() {
    let (agent, session) = connected().await;
    agent.answer(
        "lifecycle_set",
        Err(Failure::Config {
            code: "config_invalid".to_string(),
            message: "the change breaks the rules.".to_string(),
            problems: vec![
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
            ],
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
        shown.starts_with(
            "Not saved: the change breaks the rules (line 12: lifecycle.keep_awake: must be true \
             or false; line 3: expected a table)"
        ),
        "{shown}"
    );
}

// the notes under the settings after an edit or a reload ---------------------------

/// a running agent whose lifecycle part holds three settings.
async fn connected_with_three() -> (Agent, Running) {
    let (agent, link) = Agent::new();
    let world = World::default();
    world.then(Answer::Agent(link));
    let session = start(world);
    agent.push(patched("running", |value| {
        value["configuration"]["status"] = json!("valid");
        value["lifecycle"] = json!({
            "settings": {
                "keep_awake": { "on": true, "defaulted": true },
                "stop_on_network_change": { "on": true, "defaulted": true },
                "resume_after_stop": { "on": false, "defaulted": true }
            },
            "keep_awake": "held"
        });
    }));
    session.settle().await;
    (agent, session)
}

fn checked(session: &Running, key: &str) -> Option<bool> {
    settings(session)
        .groups
        .iter()
        .flat_map(|group| &group.rows)
        .find(|row| row.key == key)
        .and_then(|row| row.checked)
}

/// puts a saved value under `resume_after_stop`, a refusal under `keep_awake` and an
/// unanswered change under `stop_on_network_change`.
async fn with_a_saved_a_refused_and_an_unanswered_setting(agent: &Agent, session: &Running) {
    session
        .handle
        .set_setting("resume_after_stop".to_string(), true)
        .await
        .unwrap();
    agent.answer(
        "lifecycle_set",
        Err(Failure::Config {
            code: "config_changed".to_string(),
            message: "config.toml changed after the agent read it".to_string(),
            problems: Vec::new(),
        }),
    );
    session
        .handle
        .set_setting("keep_awake".to_string(), false)
        .await
        .unwrap();
    let gate = agent.hold("lifecycle_set");
    let handle = session.handle.clone();
    let call = tokio::spawn(async move {
        handle
            .set_setting("stop_on_network_change".to_string(), false)
            .await
    });
    session.wait(secs(11)).await;
    call.await.unwrap().unwrap();
    drop(gate);
    session.settle().await;
    assert!(note(session, "keep_awake").is_some());
    assert!(note(session, "stop_on_network_change").is_some());
    assert_eq!(checked(session, "resume_after_stop"), Some(true));
}

fn change() -> ConfigChange {
    serde_json::from_value(json!({ "revision": "r2", "effects": [], "applies": false })).unwrap()
}

fn reload(check: bool) -> ConfigRequest {
    ConfigRequest::Reload {
        revision: "r1".to_string(),
        check,
    }
}

fn edit(check: bool) -> ConfigRequest {
    ConfigRequest::Edit {
        revision: "r1".to_string(),
        edits: json!([{"op": "set", "path": "pool[0].runners", "value": 2}]),
        check,
    }
}

#[tokio::test(start_paused = true)]
async fn an_applied_reload_or_edit_removes_the_refusal_and_the_unanswered_notes_and_keeps_a_saved_value()
 {
    for request in [reload(false), edit(false)] {
        let (agent, session) = connected_with_three().await;
        with_a_saved_a_refused_and_an_unanswered_setting(&agent, &session).await;
        agent.answer_config(Ok(ConfigAnswer::Change(change())));
        session.handle.config(request.clone()).await.unwrap();
        session.settle().await;
        assert_eq!(note(&session, "keep_awake"), None, "{request:?}");
        assert_eq!(
            note(&session, "stop_on_network_change"),
            None,
            "{request:?}"
        );
        // a saved value still holds its checkbox, until the snapshot shows it
        assert_eq!(checked(&session, "resume_after_stop"), Some(true));
    }
}

#[tokio::test(start_paused = true)]
async fn a_check_a_refusal_a_failure_and_a_timeout_leave_the_notes() {
    let (agent, session) = connected_with_three().await;
    with_a_saved_a_refused_and_an_unanswered_setting(&agent, &session).await;
    let notes = |session: &Running| {
        (
            note(session, "keep_awake"),
            note(session, "stop_on_network_change"),
        )
    };
    let before = notes(&session);

    // a check writes nothing
    for request in [reload(true), edit(true)] {
        agent.answer_config(Ok(ConfigAnswer::Change(change())));
        session.handle.config(request).await.unwrap();
        session.settle().await;
        assert_eq!(notes(&session), before);
    }
    // a refusal and a failure
    for failure in [
        Failure::Config {
            code: "config_changed".into(),
            message: "config.toml changed after it was read".into(),
            problems: Vec::new(),
        },
        Failure::Refused("the agent is stopping".into()),
        Failure::Busy("the agent is busy".into()),
    ] {
        agent.answer_config(Err(failure.clone()));
        assert_eq!(session.handle.config(reload(false)).await, Err(failure));
        session.settle().await;
        assert_eq!(notes(&session), before);
    }
    // an answer that does not come in time
    let gate = agent.hold("config");
    let handle = session.handle.clone();
    let call = tokio::spawn(async move { handle.config(reload(false)).await });
    session.settle().await;
    session.wait(secs(41)).await;
    assert_eq!(call.await.unwrap(), Err(Failure::TimedOut));
    session.settle().await;
    assert_eq!(notes(&session), before);
    drop(gate);
}

#[tokio::test(start_paused = true)]
async fn a_read_and_a_token_removal_remove_no_note() {
    let (agent, session) = connected_with_three().await;
    with_a_saved_a_refused_and_an_unanswered_setting(&agent, &session).await;
    let before = note(&session, "keep_awake");
    agent.answer_config(Ok(ConfigAnswer::TokenRemoved));
    session
        .handle
        .config(ConfigRequest::RemoveReleasedToken {
            repository: "owner/released".to_string(),
        })
        .await
        .unwrap();
    session.settle().await;
    assert_eq!(note(&session, "keep_awake"), before);
}

// the next step a refused setting names ---------------------------------------------

async fn refused_with_the_agent(
    snapshot: marshal_cli::agent::snapshot::Snapshot,
    edits: bool,
) -> (Vec<String>, Vec<String>) {
    let (agent, link) = Agent::reporting(marshal_session::AgentInfo {
        version: "1.4.0".into(),
        drains: true,
        edits,
    });
    let world = World::default();
    world.then(Answer::Agent(link));
    let session = start(world);
    agent.push(snapshot);
    session.settle().await;
    agent.answer(
        "lifecycle_set",
        Err(Failure::Config {
            code: "config_changed".to_string(),
            message: "config.toml changed after the agent read it".to_string(),
            problems: Vec::new(),
        }),
    );
    session
        .handle
        .set_setting("keep_awake".to_string(), false)
        .await
        .unwrap();
    session.settle().await;
    (session.screen.announced(), session.log.lines())
}

fn with_settings(name: &str) -> marshal_cli::agent::snapshot::Snapshot {
    patched(name, |value| {
        value["configuration"]["status"] = json!("valid");
        value["lifecycle"] = json!({
            "settings": { "keep_awake": { "on": true, "defaulted": true } },
            "keep_awake": "held"
        });
    })
}

#[tokio::test(start_paused = true)]
async fn a_refused_settings_announcement_and_log_line_name_the_step_for_the_agents_state() {
    for (name, edits, step) in [
        ("running", true, "Choose Reload config.toml to read them"),
        (
            "running",
            false,
            "Choose Stop All and Start All to read them",
        ),
        ("stopped_by_you", true, "Choose Start All to read them"),
    ] {
        let (announced, log) = refused_with_the_agent(with_settings(name), edits).await;
        let said = announced
            .iter()
            .find(|text| text.starts_with("Not saved: config.toml changed"))
            .unwrap_or_else(|| panic!("{name}: nothing announced: {announced:?}"));
        assert!(said.contains(step), "{name} {edits}: {said}");
        assert!(
            log.iter()
                .any(|line| line.contains("keep_awake was not set off") && line.contains(step)),
            "{name} {edits}: {log:?}"
        );
    }
}
