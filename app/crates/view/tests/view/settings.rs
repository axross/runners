//! the Settings tab: each setting's state against its Windows default, the notes the
//! agent's state and a change's result put under a setting, the chip, and when no
//! setting can be changed.

use marshal_view::{
    Cause, Connection, Input, NEEDS_ATTENTION, NoteTone, SettingResult, SettingRow, SettingsTab,
    SettingsUi, build,
};
use serde_json::{Value, json};

use crate::support::{list, now, patched, view_in, view_of};

/// `running.json` with a `lifecycle` part: sign-in set on (its default is off),
/// keep-awake at its default, the network stop set off (its default is on), the resume
/// set off, which is its default, and the launch setting left at its default.
fn with_lifecycle(change: impl FnOnce(&mut Value)) -> marshal_cli::agent::snapshot::Snapshot {
    patched("running", |value| {
        value["configuration"]["status"] = json!("valid");
        value["lifecycle"] = json!({
            "settings": {
                "start_at_login": { "on": true, "defaulted": false },
                "start_runners_on_launch": { "on": true, "defaulted": true },
                "keep_awake": { "on": true, "defaulted": true },
                "stop_on_network_change": { "on": false, "defaulted": false },
                "resume_after_stop": { "on": false, "defaulted": false }
            },
            "sign_in": "registered",
            "keep_awake": "held"
        });
        change(value);
    })
}

fn settings(view: &marshal_view::View) -> &SettingsTab {
    &list(view).settings
}

fn row<'a>(tab: &'a SettingsTab, key: &str) -> &'a SettingRow {
    tab.groups
        .iter()
        .flat_map(|group| &group.rows)
        .find(|row| row.key == key)
        .unwrap_or_else(|| panic!("no row for {key}"))
}

fn view_with(
    snapshot: &marshal_cli::agent::snapshot::Snapshot,
    ui: &SettingsUi,
) -> marshal_view::View {
    build(&Input::new(Connection::Connected, Some(snapshot), now()).settings(ui))
}

#[test]
fn the_settings_are_in_two_groups_in_the_designs_order() {
    let view = view_of(&with_lifecycle(|_| {}));
    let tab = settings(&view);
    let groups: Vec<(&str, Vec<&str>)> = tab
        .groups
        .iter()
        .map(|group| {
            (
                group.title.as_str(),
                group.rows.iter().map(|row| row.key.as_str()).collect(),
            )
        })
        .collect();
    assert_eq!(
        groups,
        vec![
            (
                "Starting Marshal",
                vec!["start_at_login", "start_runners_on_launch"]
            ),
            (
                "Sleep and network",
                vec!["keep_awake", "stop_on_network_change", "resume_after_stop"]
            ),
        ]
    );
    assert_eq!(
        row(tab, "start_at_login").label,
        "Start Marshal when I sign in"
    );
    assert_eq!(tab.unavailable, None);
    assert_eq!(tab.chip, None);
    assert_eq!(list(&view).header.settings_chip, None);
}

#[test]
fn each_state_is_read_against_the_windows_default_not_against_whether_the_file_sets_it() {
    let view = view_of(&with_lifecycle(|_| {}));
    let tab = settings(&view);
    let states: Vec<(&str, Option<bool>, &str)> = tab
        .groups
        .iter()
        .flat_map(|group| &group.rows)
        .map(|row| (row.key.as_str(), row.checked, row.state.as_str()))
        .collect();
    assert_eq!(
        states,
        vec![
            ("start_at_login", Some(true), "On (default: off)."),
            ("start_runners_on_launch", Some(true), "On (default)."),
            ("keep_awake", Some(true), "On (default)."),
            ("stop_on_network_change", Some(false), "Off (default: on)."),
            // set in the file, and to its default
            ("resume_after_stop", Some(false), "Off (default)."),
        ]
    );
}

#[test]
fn startup_apps_turning_marshal_off_is_a_warning_under_sign_in_and_needs_attention() {
    let view = view_of(&with_lifecycle(|value| {
        value["lifecycle"]["sign_in"] = json!("turned_off");
    }));
    let tab = settings(&view);
    let note = row(tab, "start_at_login").note.as_ref().expect("a note");
    assert_eq!(note.tone, NoteTone::Warning);
    assert!(
        note.text
            .ends_with("Windows Settings\u{a0}>\u{a0}Apps\u{a0}>\u{a0}Startup."),
        "{}",
        note.text
    );
    assert_eq!(tab.chip.as_deref(), Some(NEEDS_ATTENTION));
    assert_eq!(
        list(&view).header.settings_chip.as_deref(),
        Some(NEEDS_ATTENTION)
    );
}

#[test]
fn a_failed_run_key_write_keeps_the_windows_error_and_says_when_it_is_tried_again() {
    let view = view_of(&with_lifecycle(|value| {
        value["lifecycle"]["sign_in"] = json!("failed");
        value["lifecycle"]["sign_in_error"] =
            json!("the Run key value could not be written: Access is denied. (os error 5)");
    }));
    let note = row(settings(&view), "start_at_login")
        .note
        .clone()
        .expect("a note");
    assert_eq!(note.tone, NoteTone::Error);
    assert_eq!(
        note.text,
        "Marshal could not register itself to start at sign-in (the Run key value could not be written: Access is denied. (os error 5)). It tries again at the next launch or Start All."
    );
}

#[test]
fn a_pending_resume_is_said_under_its_setting_without_the_chip() {
    let view = view_of(&with_lifecycle(|value| {
        value["agent"]["state"] = json!("stopped");
        value["agent"]["reason"] = json!({
            "code": "slept",
            "message": "Windows slept, so every runner was stopped"
        });
        value["lifecycle"]["settings"]["resume_after_stop"] =
            json!({ "on": true, "defaulted": false });
        value["lifecycle"]["resume"] = json!("waiting");
        value["lifecycle"]["resume_cause"] = json!("slept");
    }));
    let tab = settings(&view);
    let note = row(tab, "resume_after_stop").note.as_ref().expect("a note");
    assert_eq!(
        (note.tone, note.text.as_str()),
        (
            NoteTone::Info,
            "Waiting for the network the runners stopped on."
        )
    );
    assert_eq!(tab.chip, None);
}

#[test]
fn resuming_too_often_is_said_under_the_resume_setting() {
    let view = view_of(&with_lifecycle(|value| {
        value["agent"]["state"] = json!("stopped");
        value["agent"]["reason"] = json!({
            "code": "resumed_too_often",
            "message": "Windows slept, so every runner was stopped; the runners resumed 5 times within an hour, so they will not start again on their own"
        });
    }));
    let note = row(settings(&view), "resume_after_stop")
        .note
        .clone()
        .expect("a note");
    assert_eq!(note.tone, NoteTone::Warning);
    assert!(note.text.starts_with("Not resumed:"), "{}", note.text);
}

#[test]
fn a_keep_awake_warning_is_the_agents_words_as_a_sentence() {
    let view = view_of(&with_lifecycle(|value| {
        value["lifecycle"]["keep_awake"] = json!("warning");
        value["lifecycle"]["keep_awake_warning"] = json!(
            "keep-awake is not held (the request failed), so Windows may sleep when idle; trying again"
        );
    }));
    let note = row(settings(&view), "keep_awake")
        .note
        .clone()
        .expect("a note");
    assert_eq!(
        note.text,
        "Keep-awake is not held (the request failed), so Windows may sleep when idle; trying again."
    );
}

#[test]
fn a_change_refused_because_the_file_changed_stays_under_its_setting_and_needs_attention() {
    let snapshot = with_lifecycle(|_| {});
    let refused = SettingResult::Refused {
        code: "config_changed".to_string(),
        words: "config.toml changed after the agent read it; Start all reads it again".to_string(),
    };
    let ui = SettingsUi {
        writing: None,
        results: [("keep_awake".to_string(), refused.clone())].into(),
    };
    let view = view_with(&snapshot, &ui);
    let tab = settings(&view);
    let note = row(tab, "keep_awake").note.clone().expect("a note");
    assert_eq!(note.tone, NoteTone::Error);
    assert!(
        note.text.starts_with("Not saved: config.toml changed"),
        "{}",
        note.text
    );
    // what the live region says is what the setting shows
    assert_eq!(
        refused.announcement("Keep the computer awake while runners run"),
        note.text
    );
    // the value shown is still the one in effect
    assert_eq!(row(tab, "keep_awake").checked, Some(true));
    assert_eq!(tab.chip.as_deref(), Some(NEEDS_ATTENTION));
}

#[test]
fn another_refusal_gives_the_agents_reason_and_a_next_step() {
    let snapshot = with_lifecycle(|_| {});
    let ui = SettingsUi {
        writing: None,
        results: [(
            "keep_awake".to_string(),
            SettingResult::Refused {
                code: "config_unwritable".to_string(),
                words: "config.toml was not written: Access is denied. (os error 5)".to_string(),
            },
        )]
        .into(),
    };
    let view = view_with(&snapshot, &ui);
    assert_eq!(
        row(settings(&view), "keep_awake")
            .note
            .clone()
            .expect("a note")
            .text,
        "Not saved: config.toml was not written: Access is denied. (os error 5). Close any program \
         that has config.toml open, then change the setting again."
    );
}

#[test]
fn a_saved_change_shows_no_note_holds_its_value_and_is_announced_with_its_label() {
    let snapshot = with_lifecycle(|_| {});
    let saved = SettingResult::Saved { on: false };
    let ui = SettingsUi {
        writing: None,
        results: [("keep_awake".to_string(), saved.clone())].into(),
    };
    let view = view_with(&snapshot, &ui);
    assert_eq!(row(settings(&view), "keep_awake").note, None);
    // the snapshot still says on: the saved value holds until it shows the change
    assert_eq!(row(settings(&view), "keep_awake").checked, Some(false));
    assert_eq!(
        row(settings(&view), "keep_awake").state,
        "Off (default: on)."
    );
    assert_eq!(
        saved.announcement("Keep the computer awake while runners run"),
        "Saved: \"Keep the computer awake while runners run\" is off."
    );
}

#[test]
fn while_a_setting_is_saved_no_other_can_be_changed_and_it_shows_the_value_chosen() {
    let snapshot = with_lifecycle(|_| {});
    let ui = SettingsUi {
        writing: Some(("keep_awake".to_string(), false)),
        results: Default::default(),
    };
    let view = view_with(&snapshot, &ui);
    let tab = settings(&view);
    assert_eq!(
        tab.unavailable.as_deref(),
        Some("Settings cannot be changed while a setting is being saved.")
    );
    let rows: Vec<_> = tab.groups.iter().flat_map(|group| &group.rows).collect();
    let saving = rows.iter().find(|row| row.key == "keep_awake").unwrap();
    assert_eq!(saving.checked, Some(false));
    assert_eq!(saving.state, "Being saved as off.");
    let other = rows.iter().find(|row| row.key == "start_at_login").unwrap();
    assert_ne!(other.state, "Being saved as off.");
}

#[test]
fn with_config_toml_invalid_at_launch_the_values_are_unknown_and_the_tab_needs_attention() {
    let snapshot = patched("stopped_invalid_configuration", |_| {});
    let view = view_of(&snapshot);
    let tab = settings(&view);
    let invalid = "Settings cannot be changed while config.toml has problems. Fix the file, then choose Start All.";
    assert_eq!(tab.unavailable.as_deref(), Some(invalid));
    assert_eq!(tab.intro, invalid);
    assert!(
        tab.groups
            .iter()
            .flat_map(|group| &group.rows)
            .all(|row| row.checked.is_none()),
        "{tab:?}"
    );
    assert_eq!(tab.chip.as_deref(), Some(NEEDS_ATTENTION));
}

#[test]
fn a_problem_naming_one_setting_leaves_only_that_one_unknown() {
    let snapshot = with_lifecycle(|value| {
        value["configuration"]["status"] = json!("invalid");
        value["configuration"]["problems"] =
            json!(["line 12: lifecycle.keep_awake: must be true or false"]);
    });
    let view = view_of(&snapshot);
    let tab = settings(&view);
    assert_eq!(row(tab, "keep_awake").checked, None);
    assert_eq!(row(tab, "keep_awake").state, "Not readable in config.toml.");
    assert_eq!(row(tab, "stop_on_network_change").checked, Some(false));
    // the runners still run on the file read before, so Start All alone is not offered
    assert_eq!(
        tab.unavailable.as_deref(),
        Some(
            "Settings cannot be changed while config.toml has problems. Fix the file, then \
             choose Stop All and Start All."
        )
    );
}

#[test]
fn under_lost_contact_the_last_values_stay_and_nothing_can_be_changed() {
    let snapshot = with_lifecycle(|_| {});
    let view = view_in(
        Connection::LostContact {
            last_heard_ms: now().now_ms,
            cause: Cause::Dropped,
        },
        Some(&snapshot),
    );
    let tab = settings(&view);
    assert_eq!(
        tab.unavailable.as_deref(),
        Some("Settings cannot be changed while Marshal's agent does not answer.")
    );
    assert_eq!(row(tab, "start_at_login").checked, Some(true));
}
