//! the Settings tab: each setting's state against its default, the notes the
//! agent's state and a change's result put under a setting, the chip, and when no
//! setting can be changed.

use marshal_view::{
    Cause, Connection, Input, LoginItemState, MACOS, NEEDS_ATTENTION, NoteTone, OPEN_LOGIN_ITEMS,
    Platform, SettingResult, SettingRow, SettingsTab, SettingsUi, build, setting_label,
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
fn each_state_is_read_against_its_default_not_against_whether_the_file_sets_it() {
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
    assert!(!tab.reason_in_intro);
}

#[test]
fn with_config_toml_invalid_at_launch_the_values_are_unknown_and_the_tab_needs_attention() {
    let snapshot = patched("stopped_invalid_configuration", |_| {});
    let view = view_of(&snapshot);
    let tab = settings(&view);
    let invalid = "Settings cannot be changed while config.toml has problems. Fix the file, then choose Start All.";
    assert_eq!(tab.unavailable.as_deref(), Some(invalid));
    assert_eq!(tab.intro, invalid);
    assert!(tab.reason_in_intro);
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
    assert!(tab.reason_in_intro);
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
fn a_problem_on_a_key_that_only_starts_like_a_setting_leaves_that_setting_readable() {
    let snapshot = with_lifecycle(|value| {
        value["configuration"]["status"] = json!("invalid");
        value["configuration"]["problems"] =
            json!(["line 14: lifecycle.keep_awake_x: unknown key"]);
    });
    let view = view_of(&snapshot);
    let tab = settings(&view);
    assert_eq!(row(tab, "keep_awake").checked, Some(true));
    assert_eq!(row(tab, "keep_awake").state, "On (default).");
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
    assert!(!tab.reason_in_intro);
}

#[test]
fn the_intro_gives_the_reason_only_when_it_is_that_the_file_has_problems() {
    let missing = with_lifecycle(|value| value["configuration"]["status"] = json!("missing"));
    let view = view_of(&missing);
    let tab = settings(&view);
    assert_eq!(
        tab.unavailable.as_deref(),
        Some("Settings cannot be changed until config.toml exists.")
    );
    assert!(tab.intro.starts_with("Saved in config.toml"));
    assert!(!tab.reason_in_intro);

    // the intro still names the problems, but the reason shown is the lost contact
    let invalid = with_lifecycle(|value| value["configuration"]["status"] = json!("invalid"));
    let view = view_in(
        Connection::LostContact {
            last_heard_ms: now().now_ms,
            cause: Cause::Dropped,
        },
        Some(&invalid),
    );
    let tab = settings(&view);
    assert!(tab.intro.contains("has problems"));
    assert_eq!(
        tab.unavailable.as_deref(),
        Some("Settings cannot be changed while Marshal's agent does not answer.")
    );
    assert!(!tab.reason_in_intro);
}

#[test]
fn a_mac_snapshot_without_settings_leaves_every_setting_unavailable_without_the_chip() {
    let snapshot = patched("running", |value| {
        value["configuration"]["status"] = json!("valid");
    });
    assert!(snapshot.lifecycle.is_none());
    let view = build(&Input::new(Connection::Connected, Some(&snapshot), now()).words(MACOS));
    let tab = settings(&view);
    assert_eq!(
        tab.unavailable.as_deref(),
        Some("Settings cannot be changed: the agent did not report them.")
    );
    let rows: Vec<_> = tab.groups.iter().flat_map(|group| &group.rows).collect();
    let keys: Vec<&str> = rows.iter().map(|row| row.key.as_str()).collect();
    assert_eq!(
        keys,
        [
            "start_at_login",
            "start_runners_on_launch",
            "keep_awake",
            "stop_on_network_change",
            "resume_after_stop"
        ]
    );
    for row in rows {
        assert!(!row.label.is_empty(), "{row:?}");
        assert_eq!(row.checked, None, "{row:?}");
        assert_eq!(row.state, "Not known while config.toml cannot be read.");
        assert_eq!(row.note, None, "{row:?}");
    }
    assert_eq!(
        row(tab, "start_at_login").label,
        "Open Marshal when I log in"
    );
    assert!(!tab.reason_in_intro);
    assert_eq!(tab.chip, None);
    assert_eq!(list(&view).header.settings_chip, None);
}

fn mac_view(
    snapshot: &marshal_cli::agent::snapshot::Snapshot,
    login: Option<LoginItemState>,
) -> marshal_view::View {
    build(
        &Input::new(Connection::Connected, Some(snapshot), now())
            .words(MACOS)
            .login_item(login),
    )
}

#[test]
fn the_mac_words_each_setting_for_the_mac_and_fixes_the_network_row() {
    let view = mac_view(&with_lifecycle(|_| {}), Some(LoginItemState::On));
    let tab = settings(&view);
    let words: Vec<(&str, &str)> = tab
        .groups
        .iter()
        .flat_map(|group| &group.rows)
        .map(|row| (row.label.as_str(), row.description.as_str()))
        .collect();
    assert_eq!(
        words,
        vec![
            (
                "Open Marshal when I log in",
                "Marshal opens with only its icon in the menu bar."
            ),
            (
                "Start the runners when Marshal opens",
                "Applies when you open Marshal or log in, not after an update."
            ),
            (
                "Keep the Mac awake while runners run",
                "Prevents idle sleep only. Closing the lid or choosing Sleep still sleeps."
            ),
            (
                "Stop the runners when the network changes",
                "A sleep, wake or network change always stops them on the Mac."
            ),
            (
                "Start the runners again after a sleep or network stop",
                "Only on the network they stopped on."
            ),
        ]
    );
    let fixed: Vec<(&str, Option<&str>)> = tab
        .groups
        .iter()
        .flat_map(|group| &group.rows)
        .map(|row| (row.key.as_str(), row.fixed.as_deref()))
        .collect();
    assert_eq!(
        fixed,
        vec![
            ("start_at_login", None),
            ("start_runners_on_launch", None),
            ("keep_awake", None),
            (
                "stop_on_network_change",
                Some("macOS always stops the runners when the primary network changes.")
            ),
            ("resume_after_stop", None),
        ]
    );
    assert_eq!(
        setting_label("keep_awake", Platform::Macos),
        Some("Keep the Mac awake while runners run")
    );
}

#[test]
fn the_fixed_network_row_is_on_on_the_mac_whatever_the_file_says() {
    for in_file in [false, true] {
        let snapshot = with_lifecycle(|value| {
            value["lifecycle"]["settings"]["stop_on_network_change"] =
                json!({ "on": in_file, "defaulted": false });
        });
        let view = mac_view(&snapshot, Some(LoginItemState::On));
        let network = row(settings(&view), "stop_on_network_change");
        assert_eq!(network.checked, Some(true), "{in_file}");
        assert_eq!(network.state, "On (default).", "{in_file}");
        assert_eq!(
            network.fixed.as_deref(),
            Some("macOS always stops the runners when the primary network changes."),
            "{in_file}"
        );
    }
    // the file's value shows as it is on Windows, where the setting can be changed
    let off = with_lifecycle(|_| {});
    let view = view_of(&off);
    let network = row(settings(&view), "stop_on_network_change");
    assert_eq!(network.checked, Some(false));
    assert_eq!(network.state, "Off (default: on).");
}

#[test]
fn windows_shows_no_login_item_and_no_fixed_row() {
    let view = view_of(&with_lifecycle(|_| {}));
    let tab = settings(&view);
    for row in tab.groups.iter().flat_map(|group| &group.rows) {
        assert_eq!(
            (&row.fixed, &row.login_item, &row.open_login_items),
            (&None, &None, &None),
            "{}",
            row.key
        );
    }
    assert_eq!(
        setting_label("keep_awake", Platform::Windows),
        Some("Keep the computer awake while runners run")
    );
}

#[test]
fn each_login_item_state_has_its_line_note_button_and_chip() {
    let on = with_lifecycle(|_| {});
    let off = with_lifecycle(|value| {
        value["lifecycle"]["settings"]["start_at_login"] =
            json!({ "on": false, "defaulted": true });
    });
    let approval = "macOS needs your approval before Marshal opens at login. Allow Marshal in System\u{a0}Settings >\u{a0}General >\u{a0}Login\u{a0}Items\u{a0}&\u{a0}Extensions.";
    let removed = "Marshal was removed from Open at Login, so it does not open at login. To have it open at login again, add Marshal under Open at Login in System\u{a0}Settings >\u{a0}General >\u{a0}Login\u{a0}Items\u{a0}&\u{a0}Extensions.";
    let failed = "Marshal could not add itself to Open at Login. It tries again at the next launch or Start All, or when you change a setting.";
    let cases = [
        (&on, LoginItemState::On, "On", None, false),
        (
            &on,
            LoginItemState::WaitingForApproval,
            "Waiting for approval",
            Some((approval, NoteTone::Warning)),
            true,
        ),
        (
            &on,
            LoginItemState::Removed,
            "Removed from Open at Login",
            Some((removed, NoteTone::Warning)),
            true,
        ),
        (
            &on,
            LoginItemState::NotAdded { code: 4 },
            "Could not be added (code 4)",
            Some((failed, NoteTone::Error)),
            false,
        ),
        (&off, LoginItemState::Off, "Off", None, false),
    ];
    for (snapshot, state, line, note, chip) in cases {
        let view = mac_view(snapshot, Some(state));
        let tab = settings(&view);
        let first = row(tab, "start_at_login");
        assert_eq!(first.login_item.as_deref(), Some(line), "{state:?}");
        assert_eq!(
            first
                .note
                .as_ref()
                .map(|note| (note.text.as_str(), note.tone)),
            note,
            "{state:?}"
        );
        assert_eq!(
            first.open_login_items.as_deref(),
            note.map(|_| OPEN_LOGIN_ITEMS),
            "{state:?}"
        );
        let expected = chip.then_some(NEEDS_ATTENTION);
        assert_eq!(tab.chip.as_deref(), expected, "{state:?}");
        assert_eq!(
            list(&view).header.settings_chip.as_deref(),
            expected,
            "{state:?}"
        );
        assert_eq!(state.announcement(), format!("Login Items: {line}."));
        // the line shows under the first setting only
        assert!(
            tab.groups
                .iter()
                .flat_map(|group| &group.rows)
                .filter(|row| row.login_item.is_some())
                .count()
                == 1
        );
    }
    assert_eq!(
        row(
            settings(&mac_view(&on, Some(LoginItemState::On))),
            "start_at_login"
        )
        .state,
        "On (default: off)."
    );
}

#[test]
fn with_the_setting_off_a_login_item_still_there_has_its_note_and_button_and_no_chip() {
    let off = with_lifecycle(|value| {
        value["lifecycle"]["settings"]["start_at_login"] =
            json!({ "on": false, "defaulted": true });
    });
    let still = "Marshal is still in Open at Login, so it opens at login. To stop that, turn Marshal off under Open at Login in System\u{a0}Settings >\u{a0}General >\u{a0}Login\u{a0}Items\u{a0}&\u{a0}Extensions.";
    let not_removed = "Marshal could not remove itself from Open at Login, so it still opens at login. It tries again at the next launch or Start All, or when you change a setting. To stop that now, turn Marshal off under Open at Login in System\u{a0}Settings >\u{a0}General >\u{a0}Login\u{a0}Items\u{a0}&\u{a0}Extensions.";
    let cases = [
        (LoginItemState::On, "On", Some((still, NoteTone::Warning))),
        (
            LoginItemState::WaitingForApproval,
            "Waiting for approval",
            Some((still, NoteTone::Warning)),
        ),
        (
            LoginItemState::NotRemoved { code: 4 },
            "Could not be removed (code 4)",
            Some((not_removed, NoteTone::Error)),
        ),
        (LoginItemState::Removed, "Removed from Open at Login", None),
        (LoginItemState::Off, "Off", None),
    ];
    for (state, line, note) in cases {
        let view = mac_view(&off, Some(state));
        let tab = settings(&view);
        let first = row(tab, "start_at_login");
        assert_eq!(first.login_item.as_deref(), Some(line), "{state:?}");
        assert_eq!(
            first
                .note
                .as_ref()
                .map(|note| (note.text.as_str(), note.tone)),
            note,
            "{state:?}"
        );
        assert_eq!(
            first.open_login_items.as_deref(),
            note.map(|_| OPEN_LOGIN_ITEMS),
            "{state:?}"
        );
        assert_eq!(
            tab.chip, None,
            "{state:?}: the chip is for approval and removal"
        );
        assert_eq!(state.announcement(), format!("Login Items: {line}."));
    }
}

#[test]
fn with_the_setting_on_a_login_item_that_could_not_be_removed_has_no_note() {
    let view = mac_view(
        &with_lifecycle(|_| {}),
        Some(LoginItemState::NotRemoved { code: 4 }),
    );
    let first = row(settings(&view), "start_at_login");
    assert_eq!(first.note, None);
    assert_eq!(first.open_login_items, None);
}

#[test]
fn on_the_mac_the_windows_startup_apps_note_is_not_shown() {
    let view = mac_view(
        &with_lifecycle(|value| {
            value["lifecycle"]["sign_in"] = json!("turned_off");
        }),
        Some(LoginItemState::On),
    );
    let tab = settings(&view);
    assert_eq!(row(tab, "start_at_login").note, None);
    assert_eq!(tab.chip, None);
}
