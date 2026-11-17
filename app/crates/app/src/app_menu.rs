//! Marshal's own menus on macOS, which replace Tauri's default app menu so no item quits
//! without W1's question or leaves the agent behind: Marshal, Edit, Runners and Window.
//! macOS shows them while the main window is open, when Marshal is a regular app.

use marshal_tray::menu_id;
use marshal_view::{Action, Menu};
use tauri::menu::{
    AboutMetadata, Menu as TauriMenu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu,
};
use tauri::{AppHandle, Manager};

use crate::app::MAIN_WINDOW;

/// the prefix of the ids of the items this module adds, so the icon's menu, whose
/// events Tauri also delivers, and these menus never run each other's items.
const PREFIX: &str = "app_menu:";
const CLOSE_ID: &str = "app_menu:close_window";

/// the actions of the Runners menu, in order; they follow the icon menu's rules.
const RUNNERS: [Action; 3] = [Action::StartAll, Action::StopAll, Action::BuildImage];

/// sets the menus for the latest view's menu, or with every Runners item unavailable
/// before the first view.
pub fn set(app: &AppHandle, view: Option<&Menu>) -> tauri::Result<()> {
    app.set_menu(build(app, view)?)?;
    Ok(())
}

fn build(app: &AppHandle, view: Option<&Menu>) -> tauri::Result<TauriMenu<tauri::Wry>> {
    let about = AboutMetadata {
        name: Some("Marshal".to_string()),
        version: Some(env!("CARGO_PKG_VERSION").to_string()),
        ..AboutMetadata::default()
    };
    let quit = item_for(
        app,
        view,
        &Action::Quit,
        "Quit Marshal",
        Some("CmdOrCtrl+Q"),
    )?;
    let marshal = Submenu::with_items(
        app,
        "Marshal",
        true,
        &[
            &PredefinedMenuItem::about(app, Some("About Marshal"), Some(about))?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::hide(app, Some("Hide Marshal"))?,
            &PredefinedMenuItem::hide_others(app, None)?,
            &PredefinedMenuItem::show_all(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )?;
    let edit = Submenu::with_items(
        app,
        "Edit",
        true,
        &[
            &PredefinedMenuItem::undo(app, None)?,
            &PredefinedMenuItem::redo(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::cut(app, None)?,
            &PredefinedMenuItem::copy(app, None)?,
            &PredefinedMenuItem::paste(app, None)?,
            &PredefinedMenuItem::select_all(app, None)?,
        ],
    )?;
    let runners = Submenu::new(app, "Runners", true)?;
    // the host's words, Start All Runners and the rest, label the items before the
    // first view
    for (action, label) in RUNNERS.iter().zip(marshal_view::HOST.menu.iter().skip(1)) {
        runners.append(&item_for(app, view, action, label, None)?)?;
    }
    let window = Submenu::with_items(
        app,
        "Window",
        true,
        &[
            &PredefinedMenuItem::minimize(app, None)?,
            &MenuItem::with_id(app, CLOSE_ID, "Close", true, Some("CmdOrCtrl+W"))?,
        ],
    )?;
    TauriMenu::with_items(app, &[&marshal, &edit, &runners, &window])
}

/// the item of `action` with the view's label and availability; with no view yet it is
/// unavailable, under `fallback`.
fn item_for(
    app: &AppHandle,
    view: Option<&Menu>,
    action: &Action,
    fallback: &str,
    accelerator: Option<&str>,
) -> tauri::Result<MenuItem<tauri::Wry>> {
    let id = menu_id(action).unwrap_or_default();
    let found = view.and_then(|menu| menu.items.iter().find(|item| &item.action == action));
    let (label, available) = match found {
        Some(item) => (item.label.as_str(), item.available),
        None => (fallback, false),
    };
    MenuItem::with_id(app, format!("{PREFIX}{id}"), label, available, accelerator)
}

/// runs an item of these menus: Close hides the main window, and the rest run as the
/// icon menu's item of the same id does, Quit with W1's question.
pub fn on_menu_event(app: &AppHandle, event: &MenuEvent) {
    let id = event.id().as_ref();
    if id == CLOSE_ID {
        if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
            let _ = crate::app::hide_main_window(&window.as_ref().window());
        }
        return;
    }
    let Some(id) = id.strip_prefix(PREFIX) else {
        return;
    };
    let (app, id) = (app.clone(), id.to_string());
    tauri::async_runtime::spawn(async move { crate::tray::run_menu_item(&app, &id).await });
}
