//! the Mac's words: the same snapshots drawn with `MACOS` name Tart, the golden VM,
//! VMs and the login keychain, and nothing of Windows or Docker the view adds itself.

use crate::support::{load, load_json, now};
use marshal_view::{Connection, Input, MACOS, View, build};
use serde_json::Value;

fn mac_view(name: &str) -> View {
    build(&Input::new(Connection::Connected, Some(&load(name)), now()).words(MACOS))
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
            for windows in [
                "Windows",
                "Docker",
                "container",
                "Credential Manager",
                "Task Manager",
                ".exe",
                "runner image",
            ] {
                assert!(
                    !text.contains(windows),
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
