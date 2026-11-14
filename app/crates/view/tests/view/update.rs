//! the drain for an update in the existing state display: the summary line, the menu's
//! first line and the tooltip say what the drain waits for, the live region hears it, and
//! Start All and Build stay unavailable.

use marshal_cli::agent::snapshot::Snapshot;
use marshal_view::{Action, Connection, Input, Pending, PendingKind, Tracker, build};
use serde_json::json;

use crate::support::*;

fn draining(waiting: u32) -> Snapshot {
    patched("running", |value| {
        value["agent"]["update"] = json!({
            "version": "1.4.0",
            "waiting": waiting,
            "deadline": at(9, 15, 0),
            "pid": 4242
        });
    })
}

#[test]
fn a_drain_says_how_many_runners_it_waits_for() {
    let view = view_of(&draining(2));
    let text = "Updating to 1.4.0: waiting for 2 runners to finish";
    assert_eq!(view.summary.line, text);
    assert_eq!(view.menu.summary, text);
    assert_eq!(
        view.tooltip,
        "Marshal: updating to 1.4.0: waiting for 2 runners to finish"
    );
    // the count of runners up is still the heading
    assert_eq!(view.summary.heading, "3 of 6 runners up");

    let one = view_of(&draining(1));
    assert_eq!(
        one.menu.summary,
        "Updating to 1.4.0: waiting for 1 runner to finish"
    );
    let none = view_of(&draining(0));
    assert_eq!(none.summary.line, "Updating to 1.4.0");
    assert_eq!(none.menu.summary, "Updating to 1.4.0");
}

#[test]
fn start_all_and_build_stay_unavailable_while_the_agent_drains() {
    let view = view_of(&draining(2));
    assert!(!menu_item(&view, &Action::StartAll).available);
    assert!(!menu_item(&view, &Action::BuildImage).available);
    assert!(menu_item(&view, &Action::StopAll).available);
    assert!(menu_item(&view, &Action::Quit).available);
}

#[test]
fn the_live_region_hears_the_drain_and_its_count() {
    let mut tracker = Tracker::new();
    let running = load("running");
    tracker.update(Input::new(Connection::Connected, Some(&running), now()));
    let started = draining(2);
    let heard = tracker.update(Input::new(
        Connection::Connected,
        Some(&started),
        clock_at(3, 53, 0),
    ));
    assert_eq!(
        heard.announced.messages,
        ["Updating to 1.4.0: waiting for 2 runners to finish"]
    );
    let fewer = draining(1);
    let heard = tracker.update(Input::new(
        Connection::Connected,
        Some(&fewer),
        clock_at(3, 54, 0),
    ));
    assert_eq!(
        heard.announced.messages,
        ["Updating to 1.4.0: waiting for 1 runner to finish"]
    );
}

#[test]
fn a_stop_asked_for_during_the_drain_shows_as_stopping() {
    let snapshot = draining(2);
    let input = Input::new(Connection::Connected, Some(&snapshot), now()).pending(Some(Pending {
        kind: PendingKind::StopAll,
        since_ms: at(3, 52, 0),
    }));
    let view = build(&input);
    assert_eq!(view.menu.summary, "Stopping, 3 of 6 runners up");
    assert_eq!(view.summary.line, "Stopping since today 03:52");
}
