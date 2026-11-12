//! the Tokens tab, a card's token note and pill, the chip, the forms and their results.

use std::collections::BTreeMap;

use marshal_view::{
    Action, Cause, Connection, Input, Look, NotSaved, Tab, TokenResult, TokenState, TokenUi, build,
    discarded_announcement, pools_started_by,
};

use crate::support::*;

fn ui(editing: Option<&str>, removing: Option<&str>, results: &[(&str, TokenResult)]) -> TokenUi {
    TokenUi {
        editing: editing.map(str::to_string),
        removing: removing.map(str::to_string),
        results: results
            .iter()
            .map(|(key, result)| ((*key).to_string(), result.clone()))
            .collect::<BTreeMap<_, _>>(),
    }
}

fn with_ui(name: &str, ui: &TokenUi) -> marshal_view::View {
    let snapshot = load(name);
    build(&Input::new(Connection::Connected, Some(&snapshot), now()).tokens(ui))
}

#[test]
fn first_launch_opens_on_tokens_and_asks_for_one_per_repository() {
    let view = view_of(&load("first_launch"));
    let list = list(&view);
    assert_eq!(list.header.opens_on, Tab::Tokens);
    assert_eq!(
        list.header.tokens_chip.as_deref(),
        Some("3 repositories need attention")
    );
    assert_eq!(list.tokens.chip, list.header.tokens_chip);
    assert_eq!(
        list.tokens.intro,
        "Each repository has one token, and every pool on that repository uses it. Marshal keeps tokens in Windows Credential Manager and never shows one."
    );
    let rows: Vec<_> = list
        .tokens
        .rows
        .iter()
        .map(|row| {
            (
                row.repository.as_str(),
                row.pools.as_str(),
                row.state_text.as_str(),
            )
        })
        .collect();
    assert_eq!(
        rows,
        [
            ("owner/app", "Used by pool app", "No token"),
            ("owner/site", "Used by pool site", "No token"),
            ("owner/docs", "Used by pool docs", "No token"),
        ]
    );
    for row in &list.tokens.rows {
        assert_eq!(row.state, TokenState::Missing);
        let labels: Vec<_> = row
            .controls
            .iter()
            .map(|control| control.label.as_str())
            .collect();
        assert_eq!(labels, ["Set Token"]);
        assert_eq!(row.controls[0].look, Look::Tonal);
        assert!(row.controls[0].available);
        assert!(row.form.is_none() && row.status.is_none() && !row.confirming_removal);
    }
    assert_eq!(
        list.pools[0].note.as_deref(),
        Some("No token for owner/app.")
    );
    let pill = list.pools[0].pill.as_ref().expect("Set Token");
    assert_eq!(pill.action, Action::SetToken("owner/app".to_string()));
    assert_eq!(pill.label, "Set Token");
}

#[test]
fn a_running_window_with_all_tokens_set_has_no_chip_and_opens_on_pools() {
    let view = view_of(&load("all_up"));
    let list = list(&view);
    assert_eq!(list.header.opens_on, Tab::Pools);
    assert!(list.header.tokens_chip.is_none());
    for row in &list.tokens.rows {
        assert_eq!(row.state, TokenState::Set);
        assert_eq!(row.state_text, "Token set");
        let labels: Vec<_> = row
            .controls
            .iter()
            .map(|control| control.label.as_str())
            .collect();
        assert_eq!(labels, ["Replace Token", "Remove Token"]);
        assert_eq!(row.controls[1].look, Look::Destructive);
    }
    assert!(
        list.pools
            .iter()
            .all(|pool| pool.pill.is_none() && pool.note.is_none())
    );
}

#[test]
fn a_skipped_pool_says_why_and_offers_set_token_where_the_chip_counts_one() {
    let view = view_of(&load("running"));
    let list = list(&view);
    assert_eq!(
        list.header.tokens_chip.as_deref(),
        Some("1 repository needs attention")
    );
    assert_eq!(
        list.pools[2].note.as_deref(),
        Some("No token for owner/docs. Start All skipped this pool.")
    );
    assert_eq!(
        list.pools[2].pill.as_ref().map(|pill| pill.action.clone()),
        Some(Action::SetToken("owner/docs".to_string()))
    );
    assert_eq!(view.attention.len(), 1);
    assert_eq!(
        view.attention[0].text,
        "Pool docs: No token for owner/docs. Start All skipped this pool."
    );
}

#[test]
fn pools_on_one_repository_share_one_row_and_one_state() {
    let view = view_of(&load("shared_repository"));
    let list = list(&view);
    let docs = &list.tokens.rows[0];
    assert_eq!(docs.repository, "owner/docs");
    assert_eq!(docs.pools, "Used by pools docs and docs-arm");
    assert_eq!(docs.state, TokenState::Missing);
    assert_eq!(
        list.pools[0].note.as_deref(),
        Some("No token for owner/docs.")
    );
    assert_eq!(list.pools[1].note, list.pools[0].note);
    let question = removal_question_of(&load("shared_repository"), "owner/docs");
    assert!(
        question.is_none(),
        "a repository with no token has nothing to remove"
    );
}

fn removal_question_of(
    snapshot: &marshal_cli::agent::snapshot::Snapshot,
    repository: &str,
) -> Option<marshal_view::Confirmation> {
    let view = view_of(snapshot);
    list(&view)
        .tokens
        .rows
        .iter()
        .find(|row| row.repository == repository)?
        .controls
        .iter()
        .find(|control| control.action == Action::RemoveToken(repository.to_string()))?
        .confirmation
        .clone()
}

#[test]
fn removing_a_shared_token_names_every_pool_that_stops_getting_runners() {
    let snapshot = patched("shared_repository", |value| {
        for repository in value["repositories"].as_array_mut().expect("repositories") {
            repository["has_token"] = true.into();
        }
        for pool in value["pools"].as_array_mut().expect("pools") {
            pool["has_token"] = true.into();
        }
    });
    let question = removal_question_of(&snapshot, "owner/docs").expect("a question");
    assert_eq!(question.title, "Remove the token for owner/docs?");
    assert_eq!(
        question.body,
        "Pools docs and docs-arm stop getting new runners."
    );
    assert_eq!(
        (question.confirm.as_str(), question.cancel.as_str()),
        ("Remove", "Cancel")
    );
    let one = removal_question_of(&snapshot, "owner/app").expect("a question");
    assert_eq!(one.body, "Pool app stops getting new runners.");
}

#[test]
fn removing_a_token_with_runners_up_says_they_finish_their_job() {
    let snapshot = patched("running", |value| {
        for repository in value["repositories"].as_array_mut().expect("repositories") {
            repository["has_token"] = true.into();
        }
    });
    let question = removal_question_of(&snapshot, "owner/app").expect("a question");
    assert_eq!(question.title, "Remove the token for owner/app?");
    assert_eq!(
        question.body,
        "Pool app stops getting new runners; runners already up finish their job."
    );
}

#[test]
fn a_token_removed_while_runners_are_up_keeps_them_up_and_does_not_say_skipped() {
    let view = view_of(&load("token_removed_runners_up"));
    let list = list(&view);
    let app = &list.pools[0];
    assert_eq!(app.line, "Running · 2 of 2 up");
    assert_eq!(
        app.note.as_deref(),
        Some("No token for owner/app. Runners up finish their jobs; no new runner registers.")
    );
    assert!(!app.note.as_deref().unwrap_or_default().contains("skipped"));
    assert_eq!(
        app.pill.as_ref().map(|pill| pill.label.as_str()),
        Some("Set Token")
    );
    assert_eq!(list.tokens.rows[0].state_text, "No token");
    assert_eq!(
        list.header.tokens_chip.as_deref(),
        Some("2 repositories need attention")
    );
}

#[test]
fn a_token_removed_when_no_runner_is_up_says_no_new_runner_registers() {
    let snapshot = patched("token_removed_runners_up", |value| {
        let app = &mut value["pools"][0];
        app["runners_up"] = 0.into();
        for runner in app["runners"].as_array_mut().expect("runners") {
            runner["state"] = "registering".into();
        }
    });
    let view = view_of(&snapshot);
    assert_eq!(
        list(&view).pools[0].note.as_deref(),
        Some("No token for owner/app. No new runner registers.")
    );
}

#[test]
fn a_token_github_rejects_shows_on_the_pool_and_the_row_with_replace_token() {
    let view = view_of(&load("token_rejected"));
    let list = list(&view);
    let words = "GitHub rejected the token for owner/app: Bad credentials.";
    assert_eq!(list.pools[0].note.as_deref(), Some(words));
    let pill = list.pools[0].pill.as_ref().expect("Replace Token");
    assert_eq!(pill.action, Action::ReplaceToken("owner/app".to_string()));
    assert_eq!(
        (pill.label.as_str(), pill.look),
        ("Replace Token", Look::Tonal)
    );
    let row = &list.tokens.rows[0];
    assert_eq!(row.state, TokenState::Rejected);
    assert_eq!(row.state_text, words);
    let labels: Vec<_> = row
        .controls
        .iter()
        .map(|control| control.label.as_str())
        .collect();
    assert_eq!(labels, ["Replace Token", "Remove Token"]);
    assert_eq!(list.tokens.rows[1].state, TokenState::Set);
    assert!(
        view.attention
            .iter()
            .any(|item| item.text == format!("Pool app: {words}"))
    );
}

#[test]
fn a_backing_off_runner_with_another_reason_is_not_taken_for_a_rejected_token() {
    let view = view_of(&load("running"));
    let list = list(&view);
    assert_eq!(list.tokens.rows[1].state, TokenState::Set);
    assert!(list.pools[1].note.is_none());
}

#[test]
fn a_rejection_without_a_message_says_unauthorized() {
    let snapshot = patched("token_rejected", |value| {
        for runner in value["pools"][0]["runners"]
            .as_array_mut()
            .expect("runners")
        {
            runner["reason"] =
                "failed to request a JIT registration (Response status code does not indicate success: 401 (Unauthorized).)".into();
        }
    });
    let view = view_of(&snapshot);
    assert_eq!(
        list(&view).pools[0].note.as_deref(),
        Some("GitHub rejected the token for owner/app: Unauthorized.")
    );
}

#[test]
fn an_open_field_names_the_repository_and_says_what_kind_of_token_it_takes() {
    let view = with_ui("running", &ui(Some("owner/docs"), None, &[]));
    let row = list(&view)
        .tokens
        .rows
        .iter()
        .find(|row| row.repository == "owner/docs")
        .expect("row");
    let form = row.form.as_ref().expect("the field is open");
    assert_eq!(form.label, "Token for owner/docs");
    assert_eq!(
        form.hint,
        "A fine-grained personal access token with Administration: read and write for owner/docs only. Marshal keeps it in Windows Credential Manager and never shows it again."
    );
    assert_eq!(
        (form.save.as_str(), form.cancel.as_str()),
        ("Save", "Cancel")
    );
    assert_eq!(
        form.save_reason_when_empty,
        "Unavailable: the field is empty."
    );
    assert!(form.error.is_none());
    let others = list(&view)
        .tokens
        .rows
        .iter()
        .filter(|other| other.form.is_some())
        .count();
    assert_eq!(others, 1, "one field is open at a time");
}

#[test]
fn a_save_that_started_pools_says_which() {
    let running = load("running");
    assert_eq!(pools_started_by(&running, "owner/docs"), ["docs"]);
    assert!(pools_started_by(&running, "owner/app").is_empty());
    let stopped = load("stopped_by_you");
    assert!(
        pools_started_by(&stopped, "owner/docs").is_empty(),
        "stopped runners start nothing"
    );

    let saved = TokenResult::Saved {
        started: vec!["docs".into()],
    };
    assert_eq!(
        saved.status_line("owner/docs"),
        "Token for owner/docs saved. Starting pool docs."
    );
    assert_eq!(
        saved.announcement("owner/docs"),
        "Token for owner/docs saved. Starting pool docs."
    );
    let two = TokenResult::Saved {
        started: vec!["docs".into(), "docs-arm".into()],
    };
    assert_eq!(
        two.status_line("owner/docs"),
        "Token for owner/docs saved. Starting pools docs and docs-arm."
    );
    let none = TokenResult::Saved {
        started: Vec::new(),
    };
    assert_eq!(
        none.status_line("owner/docs"),
        "Token for owner/docs saved."
    );
    assert_eq!(
        TokenResult::Removed.status_line("owner/app"),
        "Token for owner/app removed."
    );
    assert_eq!(
        TokenResult::Removed.announcement("owner/app"),
        "Token for owner/app removed."
    );
}

#[test]
fn a_save_that_failed_says_so_in_the_field_and_in_the_announcement() {
    let cases = [
        (
            NotSaved::Store {
                detail: Some("Access is denied".into()),
            },
            "Not saved: Windows Credential Manager refused the token (Access is denied). The field is empty; enter the token again.",
            "Token for owner/docs not saved. Windows Credential Manager refused the token: Access is denied. The field is empty; enter the token again.",
        ),
        (
            NotSaved::Store { detail: None },
            "Not saved: Windows Credential Manager refused the token. The field is empty; enter the token again.",
            "Token for owner/docs not saved. Windows Credential Manager refused the token. The field is empty; enter the token again.",
        ),
        (
            NotSaved::TooLong,
            "Not saved: a token is at most 1,280 characters. The field is empty; enter the token again.",
            "Token for owner/docs not saved. A token is at most 1,280 characters. The field is empty; enter the token again.",
        ),
        (
            NotSaved::Refused("the agent could not read the store.".into()),
            "Not saved: the agent could not read the store. The field is empty; enter the token again.",
            "Token for owner/docs not saved. the agent could not read the store. The field is empty; enter the token again.",
        ),
    ];
    for (failure, field, announced) in cases {
        let result = TokenResult::NotSaved(failure);
        let view = with_ui(
            "running",
            &ui(Some("owner/docs"), None, &[("owner/docs", result.clone())]),
        );
        let row = list(&view)
            .tokens
            .rows
            .iter()
            .find(|row| row.repository == "owner/docs")
            .expect("row");
        let form = row.form.as_ref().expect("the field stays open");
        assert_eq!(form.error.as_deref(), Some(field));
        assert!(row.status.is_none(), "the error is in the form, not twice");
        assert_eq!(result.announcement("owner/docs"), announced);
    }
}

#[test]
fn a_failure_with_the_field_closed_stays_as_the_rows_status() {
    let result = TokenResult::NotSaved(NotSaved::TooLong);
    let view = with_ui("running", &ui(None, None, &[("owner/docs", result)]));
    let row = list(&view)
        .tokens
        .rows
        .iter()
        .find(|row| row.repository == "owner/docs")
        .expect("row");
    assert!(row.form.is_none());
    assert_eq!(
        row.status.as_deref(),
        Some(
            "Not saved: a token is at most 1,280 characters. The field is empty; enter the token again."
        )
    );
}

#[test]
fn a_removal_that_failed_says_the_token_is_still_stored() {
    // a token-shaped word in the reason, built from pieces, is never shown
    let token = format!("ghp_{}", "Q".repeat(36));
    let failed = TokenResult::NotRemoved(format!("Access is denied {token}"));
    let expected = "Token for owner/app not removed: Access is denied <token>. It is still stored.";
    assert_eq!(failed.status_line("owner/app"), expected);
    assert_eq!(failed.announcement("owner/app"), expected);
    let view = with_ui("running", &ui(None, None, &[("owner/app", failed)]));
    let row = list(&view)
        .tokens
        .rows
        .iter()
        .find(|row| row.repository == "owner/app")
        .expect("row");
    assert_eq!(row.status.as_deref(), Some(expected));
    assert_eq!(row.state, TokenState::Set, "the token is still stored");
}

#[test]
fn a_call_the_agent_has_not_answered_says_so_and_leaves_the_token_to_the_snapshot() {
    for (removal, verb) in [(false, "saving"), (true, "removing")] {
        let unanswered = TokenResult::NotAnswered { removal };
        let expected = format!(
            "The agent has not answered yet about {verb} the token for owner/app. It may still do it; this row shows where the token stands."
        );
        assert_eq!(unanswered.announcement("owner/app"), expected);
        let view = with_ui(
            "running",
            &ui(Some("owner/app"), None, &[("owner/app", unanswered)]),
        );
        let row = list(&view)
            .tokens
            .rows
            .iter()
            .find(|row| row.repository == "owner/app")
            .expect("row");
        assert_eq!(row.status.as_deref(), Some(expected.as_str()));
        assert_eq!(row.state, TokenState::Set, "the snapshot decides");
        assert_eq!(
            row.form.as_ref().and_then(|form| form.error.as_deref()),
            None,
            "nothing asks for the token again"
        );
    }
}

#[test]
fn a_result_is_the_rows_status_line() {
    let saved = TokenResult::Saved {
        started: vec!["docs".into()],
    };
    let view = with_ui("running", &ui(None, None, &[("owner/docs", saved)]));
    let row = list(&view)
        .tokens
        .rows
        .iter()
        .find(|row| row.repository == "owner/docs")
        .expect("row");
    assert_eq!(
        row.status.as_deref(),
        Some("Token for owner/docs saved. Starting pool docs.")
    );
}

#[test]
fn a_field_closed_without_a_save_is_announced() {
    assert_eq!(
        discarded_announcement("owner/docs", false),
        "The token for owner/docs was not saved."
    );
    assert_eq!(
        discarded_announcement("owner/docs", true),
        "The token for owner/docs was not saved. Marshal has lost contact with the agent."
    );
    let lost = TokenResult::NotSaved(NotSaved::LostContact);
    assert_eq!(
        lost.announcement("owner/docs"),
        "The token for owner/docs was not saved. Marshal has lost contact with the agent."
    );
    assert_eq!(
        lost.status_line("owner/docs"),
        "Not saved: Marshal has lost contact with the agent. The field is empty; enter the token again."
    );
}

#[test]
fn the_removal_question_opens_in_the_row_it_was_asked_in() {
    let view = with_ui("all_up", &ui(None, Some("owner/app"), &[]));
    let rows = &list(&view).tokens.rows;
    assert!(
        rows.iter()
            .find(|row| row.repository == "owner/app")
            .expect("row")
            .confirming_removal
    );
    assert_eq!(rows.iter().filter(|row| row.confirming_removal).count(), 1);
}

#[test]
fn token_actions_are_unavailable_with_the_reason_while_the_agent_is_unreachable() {
    let running = load("running");
    let cases = [
        (
            Connection::NotAnswering,
            "Unavailable: the agent is not answering.",
        ),
        (
            Connection::NoAgent,
            "Unavailable: Marshal cannot reach its agent.",
        ),
        (
            Connection::LockHeld,
            "Unavailable: another Marshal is running.",
        ),
        (
            Connection::StartingMarshal,
            "Unavailable: Marshal is starting.",
        ),
        (
            Connection::LostContact {
                last_heard_ms: at(3, 50, 0),
                cause: Cause::Dropped,
            },
            "Unavailable: Marshal has lost contact with the agent.",
        ),
    ];
    for (connection, expected) in cases {
        let view = view_in(connection, Some(&running));
        let Some(list) = view.list() else { continue };
        for row in &list.tokens.rows {
            for control in &row.controls {
                assert_eq!(
                    reason(control),
                    expected,
                    "{connection:?} {:?}",
                    control.action
                );
                assert_eq!(control.look, Look::Neutral);
            }
        }
        for pool in &list.pools {
            if let Some(pill) = &pool.pill {
                assert_eq!(reason(pill), expected, "{connection:?}");
            }
        }
    }
}

#[test]
fn token_actions_are_available_in_every_state_the_agent_answers_in() {
    for name in [
        "running",
        "stopped_by_you",
        "building",
        "stopping",
        "stopped_no_configuration",
        "waiting_for_docker",
    ] {
        let view = view_of(&load(name));
        for row in &list(&view).tokens.rows {
            assert!(
                row.controls.iter().all(|control| control.available),
                "{name}"
            );
        }
    }
}
