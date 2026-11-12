//! what an agent newer than the app may send within protocol version 2: reasons, runner
//! states, pool states, fields, images and warnings the app does not know. The app
//! ignores what it does not know, shows an unknown runner state by the runner's reason
//! and takes an unknown stop code for a stop that is not an error.

use marshal_view::{Action, BannerKind, Connection, IconState, Mark, Tone};

use crate::support::*;

#[test]
fn an_unknown_runner_state_is_shown_by_the_runners_reason() {
    let view = view_of(&load("newer_agent"));
    let app = &list(&view).pools[0];
    let unknown = &app.runners[1];
    assert_eq!(unknown.state, "waiting for a virtual machine to boot");
    assert_eq!(unknown.mark, Some(Mark::Unknown));
    assert!(unknown.detail.is_none(), "the reason is already the state");
    assert_eq!(
        unknown.label,
        "app, Runner 2, waiting for a virtual machine to boot"
    );
    assert_eq!(unknown.since.as_deref(), Some("since 03:15"));
    assert_eq!(
        app.runners[0].state, "Container running",
        "known states are as before"
    );
}

#[test]
fn an_unknown_runner_state_without_a_reason_is_shown_by_its_word() {
    let snapshot = patched("newer_agent", |value| {
        value["pools"][0]["runners"][1]["reason"] = serde_json::Value::Null;
    });
    let view = view_of(&snapshot);
    assert_eq!(list(&view).pools[0].runners[1].state, "Provisioning");
}

#[test]
fn an_unknown_pool_state_is_shown_by_its_word_and_its_runners_as_they_are() {
    let view = view_of(&load("newer_agent"));
    let site = &list(&view).pools[1];
    assert_eq!(site.state, "Hibernating");
    assert_eq!(site.line, "Hibernating · 1 of 3 up");
    assert!(site.mark.is_none());
    assert_eq!(site.runners[2].state, "Backing off");
}

#[test]
fn fields_topics_and_warnings_the_app_does_not_know_are_ignored() {
    let view = view_of(&load("newer_agent"));
    assert_eq!(view.summary.heading, "2 of 6 runners up");
    assert_eq!(
        view.window.footer.warnings,
        ["Warning: runner 2.338.2 was released 9 days ago, and a newer runner may exist."],
        "the warning code the app does not know says nothing"
    );
}

#[test]
fn an_image_the_app_does_not_know_is_listed_by_its_lines() {
    let view = view_of(&load("newer_agent"));
    let lines = &view.window.footer.image_lines;
    assert_eq!(lines.len(), 2);
    assert_eq!(
        lines[0],
        "Runner image: contract 3 · built 2 days ago · runner 2.338.2, released 9 days ago"
    );
    assert!(lines[1].contains("golden-vm"), "{}", lines[1]);
    assert!(lines[1].contains("ready"), "{}", lines[1]);
}

#[test]
fn an_unknown_stop_code_is_a_stop_that_is_not_an_error() {
    let view = view_of(&load("newer_agent_stopped"));
    assert_eq!(view.tooltip, "Marshal: runners stopped");
    assert_eq!(view.icon, IconState::Stopped);
    let banner = first_banner(&view);
    assert_eq!(banner.kind, BannerKind::StoppedForAnotherReason);
    assert_eq!(banner.heading, "Runners stopped");
    assert_eq!(
        banner.lines,
        [
            "the virtual machine host is not available",
            "Choose Start All to run them again."
        ]
    );
    assert_ne!(banner.tone, Tone::Error);
    assert_eq!(
        banner.announcement,
        "Runners stopped. the virtual machine host is not available"
    );
}

#[test]
fn an_unknown_stop_code_still_lets_the_agent_start_and_build() {
    let view = view_of(&load("newer_agent_stopped"));
    assert!(menu_item(&view, &Action::StartAll).available);
    assert!(menu_item(&view, &Action::BuildImage).available);
    assert!(!menu_item(&view, &Action::StopAll).available);
}

#[test]
fn a_stop_with_no_reason_says_the_agent_gave_none() {
    let snapshot = patched("newer_agent_stopped", |value| {
        value["agent"]["reason"] = serde_json::Value::Null;
    });
    let view = view_of(&snapshot);
    let banner = first_banner(&view);
    assert_eq!(banner.lines[0], "The agent gave no reason.");
    assert_eq!(banner.kind, BannerKind::StoppedForAnotherReason);
}

#[test]
fn an_unknown_agent_state_is_named_and_nothing_is_offered() {
    let view = view_of(&load("unknown_agent_state"));
    assert_eq!(
        view.tooltip,
        "Marshal: the agent is in a state this version does not know"
    );
    assert_eq!(view.icon, IconState::NotInControl);
    assert_eq!(
        view.summary.line,
        "The agent's state is not known to this version"
    );
    let banner = first_banner(&view);
    assert_eq!(banner.kind, BannerKind::UnknownState);
    assert_eq!(
        banner.heading,
        "Marshal does not know what the agent is doing"
    );
    assert_eq!(
        banner.lines,
        [
            "The agent reports the state \u{201c}hibernating\u{201d}, which this version of Marshal does not know."
        ]
    );
    for action in [Action::StartAll, Action::StopAll, Action::BuildImage] {
        assert_eq!(
            menu_item(&view, &action).reason.as_deref(),
            Some("Unavailable: this version of Marshal does not know what the agent is doing."),
            "{action:?}"
        );
    }
    assert!(menu_item(&view, &Action::Quit).available);
}

#[test]
fn quitting_in_an_unknown_agent_state_asks_about_the_agent_it_still_reaches() {
    let snapshot = load("unknown_agent_state");
    let question = |connection| {
        menu_item(&view_in(connection, Some(&snapshot)), &Action::Quit)
            .confirmation
            .clone()
            .expect("a question")
            .body
    };
    let connected = question(Connection::Connected);
    assert_eq!(
        connected,
        "Its agent ends, and so do any runners it runs and any job running on them."
    );
    assert!(
        question(Connection::NotAnswering).contains("any runners keep running"),
        "an agent the app cannot reach is left as it is"
    );
}

#[test]
fn the_reason_codes_the_view_reads_are_the_ones_the_agent_writes() {
    use marshal_cli::agent::StopReason;
    assert_eq!(
        [
            StopReason::BY_YOU,
            StopReason::NO_CONFIGURATION,
            StopReason::INVALID_CONFIGURATION,
            StopReason::IMAGE_UNUSABLE,
            StopReason::NO_POOL_STARTED,
            StopReason::RESTART_LIMIT,
            StopReason::IMAGE_BUILD_FAILED,
        ],
        [
            "by_you",
            "no_configuration",
            "invalid_configuration",
            "image_unusable",
            "no_pool_started",
            "restart_limit",
            "image_build_failed",
        ]
    );
}

#[test]
fn a_snapshot_of_an_older_shape_with_fields_missing_still_reads() {
    let snapshot = snapshot_of(serde_json::json!({
        "agent": { "state": "running" },
        "pools": [{ "name": "app", "runners": [{ "slot": 1, "state": "running" }] }]
    }));
    let view = view_of(&snapshot);
    assert_eq!(list(&view).pools[0].runners[0].state, "Container running");
    assert_eq!(
        view.window.footer.image_lines,
        ["Runner image: not checked"]
    );
}
