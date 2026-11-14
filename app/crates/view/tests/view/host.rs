//! the Mac's words: the same snapshots drawn with `MACOS` name Tart, the golden VM,
//! VMs and the login keychain, and nothing of Windows or Docker the view adds itself.

use crate::support::{list, load, load_json, now, patched};
use marshal_cli::agent::snapshot::Snapshot;
use marshal_view::{Connection, Input, MACOS, View, build};
use serde_json::{Value, json};

fn mac_view(name: &str) -> View {
    mac_view_of(&load(name))
}

fn mac_view_of(snapshot: &Snapshot) -> View {
    build(&Input::new(Connection::Connected, Some(snapshot), now()).words(MACOS))
}

/// a running Mac with M3's items, in the codes M3 agreed with M2, as a fake until M3's
/// agent sends them: a slot waiting for the macOS VM limit and one for a VM's deletion,
/// the golden macOS VM after the Linux one with its warnings, and leftover VMs of a
/// configured pool and of one no longer configured.
fn with_m3_items() -> Snapshot {
    patched("running", |value| {
        let runners = &mut value["pools"][1]["runners"];
        runners[0]["state"] = json!("waiting_for_vm_limit");
        runners[0]["workload"] = Value::Null;
        runners[0]["workload_kind"] = json!("vm");
        runners[0]["reason"] = json!("two macOS VMs exist, the most macOS allows");
        runners[1]["state"] = json!("waiting_for_vm_deletion");
        runners[1]["workload"] = json!("marshal-mac-2-20261104030000000");
        runners[1]["workload_kind"] = json!("vm");
        value["images"].as_array_mut().unwrap().push(json!({
            "name": "golden macOS VM",
            "lines": [
                {"name": "name", "value": "marshal-golden-macos-g1-r2.338.2-20261101000000"},
                {"name": "golden-vm-version", "value": "1"},
                {"name": "runner-version", "value": "2.338.2"},
                {"name": "base-published", "value": "2026-09-20"}
            ],
            "warnings": [
                {"code": "runner_old", "days": 21, "version": "2.338.2"},
                {"code": "runner_age_unknown", "version": "2.338.2"},
                {"code": "base_old", "days": 45}
            ]
        }));
        value["leftovers"] = json!([
            {"kind": "vm", "name": "app-1-20261104020000000"},
            {"kind": "vm_unconfigured_pool", "name": "old-1-20261103020000000"}
        ]);
    })
}

fn strings(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::String(text) => out.push(text.clone()),
        Value::Array(items) => items.iter().for_each(|item| strings(item, out)),
        Value::Object(map) => map.values().for_each(|item| strings(item, out)),
        _ => {}
    }
}

/// every text of the view, as the window receives it.
fn texts(view: &View) -> Vec<String> {
    let mut out = Vec::new();
    strings(
        &serde_json::to_value(view).expect("a view serializes"),
        &mut out,
    );
    out
}

fn joined(view: &View) -> String {
    texts(view).join("\n")
}

#[test]
fn the_macs_view_adds_no_windows_or_docker_words() {
    for name in [
        "waiting_for_docker",
        "building",
        "build_failed",
        "build_ready",
        "stopped_image_missing",
        "image_out_of_date",
        "leftover",
        "diagnostics",
        "token_rejected",
        "running",
    ] {
        let fixture = {
            let mut out = Vec::new();
            strings(&load_json(name), &mut out);
            out
        };
        for text in texts(&mac_view(name)) {
            // the agent's own texts in the fixture are the Windows agent's; on the Mac
            // the agent words them itself.
            if fixture
                .iter()
                .any(|from| text.contains(from.as_str()) && !from.is_empty())
            {
                continue;
            }
            let lower = text.to_lowercase();
            for windows in [
                "windows",
                "docker",
                "container",
                "credential manager",
                "task manager",
                ".exe",
                "runner image",
            ] {
                assert!(
                    !lower.contains(windows),
                    "{name}: {text:?} names {windows:?}"
                );
            }
        }
    }
}

#[test]
fn waiting_names_tart_and_how_to_install_it() {
    let all = joined(&mac_view("waiting_for_docker"));
    assert!(all.contains("Waiting for Tart"), "{all}");
    assert!(all.contains("Install Tart and Softnet"), "{all}");
}

#[test]
fn build_names_the_golden_vm() {
    for name in ["building", "build_failed", "stopped_image_missing"] {
        let all = joined(&mac_view(name));
        assert!(all.contains("golden VM"), "{name}: {all}");
    }
}

#[test]
fn leftovers_are_vms() {
    let all = joined(&mac_view("leftover"));
    assert!(all.contains("VM"), "{all}");
}

#[test]
fn tokens_are_kept_in_the_login_keychain() {
    let all = joined(&mac_view("token_rejected"));
    assert!(all.contains("login keychain"), "{all}");
}

#[test]
fn m3s_slots_say_what_they_wait_for() {
    let view = mac_view_of(&with_m3_items());
    let rows = &list(&view).pools[1].runners;
    assert_eq!(rows[0].state, "Waiting for the macOS VM limit");
    assert_eq!(
        rows[0].detail.as_deref(),
        Some("two macOS VMs exist, the most macOS allows")
    );
    assert_eq!(
        rows[1].state,
        "Waiting for the deletion of VM marshal-mac-2-20261104030000000"
    );
    assert_eq!(list(&view).pools[0].runners[0].state, "VM running");
}

#[test]
fn the_golden_macos_vm_follows_the_linux_one_with_its_own_warnings() {
    let view = mac_view_of(&with_m3_items());
    assert_eq!(
        view.window.footer.image_lines[1],
        "Golden macOS VM: marshal-golden-macos-g1-r2.338.2-20261101000000 · version 1 · runner 2.338.2 · base published 2026-09-20"
    );
    let warnings = view.window.footer.warnings.join("\n");
    assert!(
        warnings.contains("Warning: runner 2.338.2 was released 21 days ago, so each job's runner likely updates itself at start; update Marshal to get a newer runner, then build the golden macOS VM again."),
        "{warnings}"
    );
    assert!(
        warnings.contains("Warning: the golden macOS VM's runner 2.338.2 has no known release date, so its age is unknown."),
        "{warnings}"
    );
    assert!(
        warnings.contains("Warning: the golden macOS VM's macOS base was published 45 days ago; update Marshal to get a newer base, then build the golden macOS VM again."),
        "{warnings}"
    );
    // the Linux golden VM's warning keeps naming the golden VM
    assert!(warnings.contains("a newer runner may exist"), "{warnings}");
}

#[test]
fn a_vm_of_a_pool_no_longer_configured_is_not_promised_to_the_next_start() {
    let view = mac_view_of(&with_m3_items());
    let all = joined(&view);
    assert!(
        all.contains("app-1-20261104020000000 remains; the next start removes it."),
        "{all}"
    );
    assert!(
        all.contains("old-1-20261103020000000 is from a pool no longer configured, so Marshal does not delete it; macOS Runner Host says how to delete it."),
        "{all}"
    );
    assert!(
        !all.contains("tart delete"),
        "no command in the window: {all}"
    );

    let only_unconfigured = patched("running", |value| {
        value["leftovers"] = json!([
            {"kind": "vm_unconfigured_pool", "name": "old-1-20261103020000000"}
        ]);
    });
    let all = joined(&mac_view_of(&only_unconfigured));
    assert!(
        all.contains("A VM of a pool no longer configured remains"),
        "{all}"
    );
    assert!(!all.contains("the next start removes"), "{all}");
}

#[test]
fn the_next_launch_says_the_last_agent_ended_because_marshal_moved() {
    let snapshot = load("running");
    let after = build(
        &Input::new(Connection::Connected, Some(&snapshot), now())
            .words(MACOS)
            .after_moved(true),
    );
    assert!(
        joined(&after).contains("The last agent ended because Marshal was moved"),
        "{}",
        joined(&after)
    );
    assert!(!joined(&mac_view("running")).contains("because Marshal was moved"));
}
