//! when the menu and the tooltip are set again.

use marshal_tray::{Surface, menu_id};
use marshal_view::{Action, Menu, MenuItem};

fn item(action: Action, label: &str, available: bool) -> MenuItem {
    MenuItem {
        action,
        label: label.to_string(),
        available,
        reason: (!available).then(|| "Unavailable: for a reason.".to_string()),
        confirmation: None,
    }
}

fn menu(summary: &str, stop_available: bool) -> Menu {
    Menu {
        summary: summary.to_string(),
        items: vec![
            item(Action::OpenMarshal, "Open Marshal", true),
            item(Action::StopAll, "Stop all runners", stop_available),
            item(Action::Quit, "Quit Marshal", true),
        ],
    }
}

#[test]
fn the_first_view_sets_the_menu_and_the_tooltip() {
    let changes = Surface::new().show(&menu("2 of 2 runners up", true), "Marshal: 2 up");
    assert_eq!(changes.menu, Some(menu("2 of 2 runners up", true)));
    assert_eq!(changes.tooltip.as_deref(), Some("Marshal: 2 up"));
}

#[test]
fn nothing_is_set_again_while_nothing_changes() {
    let mut surface = Surface::new();
    surface.show(&menu("2 of 2 runners up", true), "Marshal: 2 up");
    let changes = surface.show(&menu("2 of 2 runners up", true), "Marshal: 2 up");
    assert_eq!((changes.menu, changes.tooltip), (None, None));
}

#[test]
fn a_change_of_availability_rebuilds_the_menu_only() {
    let mut surface = Surface::new();
    surface.show(&menu("Stopped", true), "Marshal: stopped");
    let changes = surface.show(&menu("Stopped", false), "Marshal: stopped");
    assert_eq!(changes.menu, Some(menu("Stopped", false)));
    assert_eq!(changes.tooltip, None);
}

#[test]
fn a_change_of_summary_rebuilds_the_menu() {
    let mut surface = Surface::new();
    surface.show(&menu("1 of 2 runners up", true), "Marshal");
    let changes = surface.show(&menu("2 of 2 runners up", true), "Marshal");
    assert!(changes.menu.is_some());
}

#[test]
fn a_change_of_the_reason_alone_does_not_rebuild_the_menu() {
    let mut surface = Surface::new();
    surface.show(&menu("Stopped", false), "Marshal");
    let mut other = menu("Stopped", false);
    other.items[1].reason = Some("Unavailable: for another reason.".to_string());
    assert_eq!(surface.show(&other, "Marshal").menu, None);
}

#[test]
fn a_change_of_the_tooltip_alone_sets_the_tooltip_only() {
    let mut surface = Surface::new();
    surface.show(&menu("Stopped", true), "Marshal: stopped");
    let changes = surface.show(&menu("Stopped", true), "Marshal: starting");
    assert_eq!(changes.menu, None);
    assert_eq!(changes.tooltip.as_deref(), Some("Marshal: starting"));
}

#[test]
fn the_menus_actions_have_ids_and_the_token_actions_have_none() {
    assert_eq!(menu_id(&Action::Quit), Some("quit"));
    assert_eq!(menu_id(&Action::OpenMarshal), Some("open_marshal"));
    assert_eq!(menu_id(&Action::SetToken("o/r".to_string())), None);
}
