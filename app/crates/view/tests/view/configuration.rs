//! the window's wording for K1's configuration: a migration of `config.json`, the
//! last valid configuration while `config.toml` cannot be used, a migration that
//! failed, and one that waits for the next start.

use serde_json::json;

use crate::support::*;

const PATH: &str = r"C:\Users\<user>\AppData\Local\marshal\config.toml";

/// `running` with its configuration part replaced.
fn running_with(configuration: serde_json::Value) -> marshal_cli::agent::snapshot::Snapshot {
    patched("running", |value| {
        let mut configuration = configuration;
        configuration["path"] = PATH.into();
        value["configuration"] = configuration;
    })
}

/// the footer's warnings that are not the image's.
fn configuration_warnings(view: &marshal_view::View) -> Vec<String> {
    view.window
        .footer
        .warnings
        .iter()
        .filter(|warning| !warning.contains("runner 2.338.2"))
        .cloned()
        .collect()
}

#[test]
fn the_footer_says_when_config_json_was_migrated_and_that_it_is_no_longer_read() {
    let view = view_of(&running_with(json!({
        "status": "valid",
        "in_use": { "source": "file", "read_at": at(3, 15, 0) },
        "migration": { "state": "migrated", "at": at(3, 15, 0) }
    })));
    assert_eq!(
        view.window.footer.configuration,
        format!(
            "Configuration: {PATH}, migrated from config.json today 03:15; \
             config.json is no longer read"
        )
    );
    assert!(configuration_warnings(&view).is_empty());
}

#[test]
fn a_file_with_problems_warns_that_the_last_valid_configuration_runs() {
    let view = view_of(&running_with(json!({
        "status": "invalid",
        "problems": ["line 9: pool[0].runners: must be an integer from 1 to 16"],
        "in_use": { "source": "last_valid", "read_at": at(1, 5, 0) }
    })));
    assert_eq!(
        configuration_warnings(&view),
        [
            "Warning: config.toml is not valid (line 9: pool[0].runners: must be an integer \
          from 1 to 16), so Marshal runs the configuration it read today 01:05. Fix the \
          file, then choose Stop All and Start All."
        ]
    );
    // the agent runs, so no banner says it is stopped
    assert!(
        view.banners()
            .iter()
            .all(|banner| !banner.heading.starts_with("Stopped"))
    );
}

#[test]
fn a_missing_file_beside_config_json_warns_and_says_how_the_migration_runs() {
    let view = view_of(&running_with(json!({
        "status": "missing",
        "in_use": { "source": "last_valid", "read_at": at(1, 5, 0) },
        "migration": { "state": "pending" }
    })));
    assert_eq!(
        configuration_warnings(&view),
        [
            "Warning: config.toml is missing, so Marshal runs the configuration it read \
             today 01:05. Fix the file, then choose Stop All and Start All.",
            "Marshal migrates config.json when it next starts: quit Marshal and launch it again."
        ]
    );
}

#[test]
fn a_migration_that_could_not_write_warns_that_config_json_runs() {
    let view = view_of(&running_with(json!({
        "status": "missing",
        "in_use": { "source": "json", "read_at": at(3, 15, 0) },
        "migration": {
            "state": "failed",
            "reason": "config.toml was not written: could not create config.toml: permission denied"
        }
    })));
    assert_eq!(
        configuration_warnings(&view),
        [
            "Warning: config.toml was not written: could not create config.toml: permission \
          denied. Marshal runs the configuration of config.json until it next starts, and \
          tries again then."
        ]
    );
}

#[test]
fn no_configuration_with_a_pending_migration_says_to_launch_marshal_again() {
    let snapshot = patched("stopped_no_configuration", |value| {
        value["configuration"]["status"] = "missing".into();
        value["configuration"]["migration"] = json!({ "state": "pending" });
    });
    let view = view_of(&snapshot);
    let banner = first_banner(&view);
    assert_eq!(banner.heading, "Stopped: there is no configuration");
    assert_eq!(
        banner.lines[1],
        "Marshal migrates config.json when it next starts: quit Marshal and launch it again."
    );
}

#[test]
fn a_config_json_refused_at_its_migration_is_named_with_its_problems() {
    let snapshot = patched("stopped_invalid_configuration", |value| {
        value["configuration"]["status"] = "missing".into();
        value["configuration"]["migration"] = json!({
            "state": "failed",
            "reason": "config.json is not valid, so it was not migrated"
        });
    });
    let view = view_of(&snapshot);
    let banner = first_banner(&view);
    assert_eq!(
        banner.heading,
        "Stopped: config.json is not valid, so it was not migrated"
    );
    assert_eq!(
        banner.lines,
        [
            "repositories[1].slots: must be an integer from 1 to 16",
            r"Fix C:\Users\<user>\AppData\Local\marshal\config.json, then quit Marshal and launch it again."
        ]
    );
}
