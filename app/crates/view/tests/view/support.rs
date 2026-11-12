//! loading the recorded snapshots and reading a view.

use marshal_cli::agent::snapshot::Snapshot;
use marshal_view::{
    Action, Banner, Body, Clock, Connection, Control, Input, ListWindow, MenuItem, View, build,
};
use serde_json::Value;

/// 2026-11-04 00:00 UTC, the day the fixtures' times are on.
const MIDNIGHT: u64 = 1_793_750_400_000;

/// a time on the fixtures' day, in milliseconds since the Unix epoch.
pub fn at(hours: u64, minutes: u64, seconds: u64) -> u64 {
    MIDNIGHT + ((hours * 60 + minutes) * 60 + seconds) * 1_000
}

/// the clock at 03:52 that day, local time being UTC.
pub fn now() -> Clock {
    Clock::new(at(3, 52, 0), 0)
}

pub fn clock_at(hours: u64, minutes: u64, seconds: u64) -> Clock {
    Clock::new(at(hours, minutes, seconds), 0)
}

pub fn load_json(name: &str) -> Value {
    let path = format!("{}/tests/fixtures/{name}.json", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&path).expect("a fixture");
    serde_json::from_str(&text).expect("a fixture is JSON")
}

pub fn snapshot_of(value: Value) -> Snapshot {
    serde_json::from_value(value).expect("a snapshot")
}

pub fn load(name: &str) -> Snapshot {
    snapshot_of(load_json(name))
}

/// a fixture with a change made to its JSON first.
pub fn patched(name: &str, change: impl FnOnce(&mut Value)) -> Snapshot {
    let mut value = load_json(name);
    change(&mut value);
    snapshot_of(value)
}

/// the view of a snapshot the app is connected to, at 03:52.
pub fn view_of(snapshot: &Snapshot) -> View {
    build(&Input::new(Connection::Connected, Some(snapshot), now()))
}

pub fn view_in(connection: Connection, snapshot: Option<&Snapshot>) -> View {
    build(&Input::new(connection, snapshot, now()))
}

pub fn list(view: &View) -> &ListWindow {
    view.list().expect("the window shows a list")
}

/// a header control, found by its action.
pub fn header<'a>(view: &'a View, action: &Action) -> &'a Control {
    list(view)
        .header
        .controls
        .iter()
        .find(|control| control.action == *action)
        .unwrap_or_else(|| panic!("the header has no {action:?}"))
}

pub fn menu_item<'a>(view: &'a View, action: &Action) -> &'a MenuItem {
    view.menu
        .items
        .iter()
        .find(|item| item.action == *action)
        .unwrap_or_else(|| panic!("the menu has no {action:?}"))
}

pub fn first_banner(view: &View) -> &Banner {
    view.banners().first().expect("a banner leads the list")
}

pub fn full_heading(view: &View) -> &str {
    match &view.window.body {
        Body::Full(full) => &full.heading,
        Body::List(_) => panic!("the window shows a list"),
    }
}

/// the reason a control is unavailable, or a failure if it is available.
pub fn reason(control: &Control) -> &str {
    assert!(!control.available, "{control:?} is available");
    control.reason.as_deref().expect("a reason")
}
