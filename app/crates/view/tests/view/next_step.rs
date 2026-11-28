//! the step after config.toml is fixed, which the footer, the Settings tab and the
//! stopped banners name by one rule: quit and launch again while a migration waits or
//! was rejected; otherwise Reload config.toml while the runners run and the agent
//! offers the editor, Stop All and Start All while they run without it, and Start All
//! in every other state.

use marshal_cli::agent::snapshot::Snapshot;
use marshal_view::{Connection, Input, SettingResult, SettingsUi, View, build};
use serde_json::{Value, json};

use crate::support::{at, load_json, now, snapshot_of};

const QUIT: &str = "quit Marshal and launch it again";
const RELOAD: &str = "Reload config.toml";
const STOP_AND_START: &str = "Stop All and Start All";
const START: &str = "Start All";

/// the steps `text` names, in the order the rule lists them.
fn steps(text: &str) -> Vec<&'static str> {
    let mut named = Vec::new();
    if text.contains(QUIT) {
        named.push(QUIT);
    }
    if text.contains(RELOAD) {
        named.push(RELOAD);
    }
    if text.contains(STOP_AND_START) {
        named.push(STOP_AND_START);
    }
    if text.replace(STOP_AND_START, "").contains(START) {
        named.push(START);
    }
    named
}

/// a snapshot with a `lifecycle` part, from the fixture `name`, patched by `change`.
fn state(name: &str, change: impl FnOnce(&mut Value)) -> Snapshot {
    let mut value = load_json(name);
    value["lifecycle"] = json!({
        "settings": {
            "keep_awake": { "on": true, "defaulted": true },
            "resume_after_stop": { "on": false, "defaulted": true }
        },
        "keep_awake": "held"
    });
    change(&mut value);
    snapshot_of(value)
}

fn view_with(snapshot: &Snapshot, editor: bool, ui: &SettingsUi) -> View {
    build(
        &Input::new(Connection::Connected, Some(snapshot), now())
            .editor(editor)
            .settings(ui),
    )
}

/// every text of the view that can name the step: the footer's warnings, the Settings
/// tab's introduction, reason and notes, and the banners, with where each is.
fn texts(view: &View) -> Vec<(String, String)> {
    let list = view.list().expect("the window shows a list");
    let mut found: Vec<(String, String)> = Vec::new();
    for warning in &view.window.footer.warnings {
        found.push(("footer".into(), warning.clone()));
    }
    found.push(("introduction".into(), list.settings.intro.clone()));
    if let Some(reason) = &list.settings.unavailable {
        found.push(("unavailable reason".into(), reason.clone()));
    }
    for row in list.settings.groups.iter().flat_map(|group| &group.rows) {
        if let Some(note) = &row.note {
            found.push((format!("note of {}", row.key), note.text.clone()));
        }
    }
    for banner in view.banners() {
        if banner.heading.starts_with("Stopped") {
            for line in &banner.lines {
                found.push(("banner".into(), line.clone()));
            }
        }
    }
    found
}

/// asserts that every text that names a step names `expected`, and that the places
/// in `required` name one.
fn assert_one_step(view: &View, expected: &str, required: &[&str], what: &str) {
    let texts = texts(view);
    for (place, text) in &texts {
        let named = steps(text);
        assert!(
            named.is_empty() || named == [expected],
            "{what}: the {place} names {named:?}, not {expected}: {text}"
        );
    }
    for place in required {
        assert!(
            texts
                .iter()
                .any(|(found, text)| found == place && steps(text) == [expected]),
            "{what}: the {place} does not name {expected}: {texts:#?}"
        );
    }
}

fn last_valid(snapshot: &mut Value, status: &str) {
    snapshot["configuration"]["status"] = json!(status);
    snapshot["configuration"]["problems"] = json!(["line 9: pool[0].runners: must be an integer"]);
    snapshot["configuration"]["in_use"] = json!({ "source": "last_valid", "read_at": at(1, 5, 0) });
}

fn refused(code: &str) -> SettingsUi {
    SettingsUi {
        writing: None,
        results: [(
            "keep_awake".to_string(),
            SettingResult::Refused {
                code: code.to_string(),
                words: "config.toml cannot be used".to_string(),
            },
        )]
        .into(),
    }
}

#[test]
fn running_with_the_editor_the_step_is_reload_everywhere() {
    let snapshot = state("running", |value| last_valid(value, "invalid"));
    for code in [
        "config_changed",
        "config_invalid",
        "config_unusable",
        "config_file_form",
    ] {
        let view = view_with(&snapshot, true, &refused(code));
        assert_one_step(
            &view,
            RELOAD,
            &[
                "footer",
                "introduction",
                "unavailable reason",
                "note of keep_awake",
            ],
            code,
        );
    }
}

#[test]
fn running_without_the_editor_the_step_is_stop_all_and_start_all_everywhere() {
    let snapshot = state("running", |value| last_valid(value, "invalid"));
    for code in [
        "config_changed",
        "config_invalid",
        "config_unusable",
        "config_file_form",
    ] {
        let view = view_with(&snapshot, false, &refused(code));
        assert_one_step(
            &view,
            STOP_AND_START,
            &[
                "footer",
                "introduction",
                "unavailable reason",
                "note of keep_awake",
            ],
            code,
        );
    }
}

#[test]
fn resuming_is_grouped_with_running() {
    let snapshot = state("resuming", |value| last_valid(value, "invalid"));
    let view = view_with(&snapshot, true, &refused("config_changed"));
    assert_one_step(&view, RELOAD, &["footer", "introduction"], "resuming");
}

#[test]
fn stopped_the_step_is_start_all_everywhere() {
    for editor in [true, false] {
        let snapshot = state("stopped_by_you", |value| last_valid(value, "invalid"));
        for code in [
            "config_changed",
            "config_invalid",
            "config_unusable",
            "config_file_form",
        ] {
            let view = view_with(&snapshot, editor, &refused(code));
            assert_one_step(
                &view,
                START,
                &[
                    "footer",
                    "introduction",
                    "unavailable reason",
                    "note of keep_awake",
                ],
                &format!("{code} with the editor {editor}"),
            );
        }
        let invalid = state("stopped_invalid_configuration", |_| {});
        let view = view_with(&invalid, editor, &SettingsUi::default());
        assert_one_step(&view, START, &["banner", "introduction"], "stopped invalid");
        let missing = state("stopped_no_configuration", |value| {
            value["configuration"]["status"] = json!("missing");
        });
        let view = view_with(&missing, editor, &SettingsUi::default());
        assert_one_step(&view, START, &["banner"], "stopped missing");
    }
}

#[test]
fn a_pending_migration_says_to_quit_and_launch_again_everywhere() {
    for name in ["running", "stopped_no_configuration"] {
        let snapshot = state(name, |value| {
            last_valid(value, "missing");
            value["configuration"]["migration"] = json!({ "state": "pending" });
        });
        for editor in [true, false] {
            let view = view_with(&snapshot, editor, &refused("config_unusable"));
            assert_one_step(
                &view,
                QUIT,
                &["introduction", "note of keep_awake"],
                &format!("{name} with the editor {editor}"),
            );
        }
    }
}

#[test]
fn a_config_json_rejected_at_its_migration_says_to_quit_and_launch_again_everywhere() {
    let snapshot = state("stopped_invalid_configuration", |value| {
        value["configuration"]["status"] = json!("missing");
        value["configuration"]["migration"] = json!({
            "state": "failed",
            "reason": "config.json is not valid, so it was not migrated"
        });
    });
    for editor in [true, false] {
        let view = view_with(&snapshot, editor, &refused("config_unusable"));
        assert_one_step(
            &view,
            QUIT,
            &["banner", "introduction", "note of keep_awake"],
            "rejected",
        );
    }
}

#[test]
fn a_migration_that_failed_to_write_keeps_stop_all_and_start_all_while_running() {
    let snapshot = state("running", |value| {
        value["configuration"]["status"] = json!("invalid");
        value["configuration"]["problems"] = json!(["line 3: image_name: must be a string"]);
        value["configuration"]["in_use"] = json!({ "source": "json", "read_at": at(1, 5, 0) });
        value["configuration"]["migration"] = json!({
            "state": "failed",
            "reason": "config.toml was not written: permission denied"
        });
    });
    let view = view_with(&snapshot, true, &refused("config_unusable"));
    assert_one_step(
        &view,
        STOP_AND_START,
        &["footer", "introduction", "note of keep_awake"],
        "failed write",
    );
}

#[test]
fn a_stopped_by_you_agent_with_the_fixed_file_shows_its_banner_build_and_a_usable_tab() {
    use marshal_view::{Action, BannerKind};
    let snapshot = state("stopped_by_you", |value| {
        value["configuration"]["status"] = json!("valid");
        value["configuration"]["in_use"] = json!({ "source": "file", "read_at": at(1, 5, 0) });
    });
    let view = view_with(&snapshot, true, &SettingsUi::default());
    let list = view.list().unwrap();
    assert_eq!(view.banners()[0].kind, BannerKind::StoppedByYou);
    assert!(crate::support::menu_item(&view, &Action::BuildImage).available);
    assert_eq!(list.settings.unavailable, None);
    assert!(!list.settings.reason_in_intro);
    assert!(
        view.window
            .footer
            .warnings
            .iter()
            .all(|warning| !warning.contains("config.toml"))
    );
}
