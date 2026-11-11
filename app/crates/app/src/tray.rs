//! the notification-area icon: its image, its tooltip and its menu, and what a click on
//! it does.

use marshal_session::{Chosen, Place, choose};
use marshal_tray::{Look, bitmap, menu_id};
use marshal_view::Menu;
use tauri::image::Image;
use tauri::menu::{Menu as TauriMenu, MenuEvent, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

use crate::app::{main_window_shown, show_main_window};
use crate::confirmation;
use crate::state::AppState;

pub const TRAY_ID: &str = "marshal";

/// the id of the menu's first line, which is a label.
const SUMMARY_ID: &str = "summary";

fn image(look: Look) -> Image<'static> {
    let drawn = bitmap(look.state, look.colour, look.size);
    Image::new_owned(drawn.rgba, drawn.size, drawn.size)
}

/// the icon with its first image, and no menu until the session shows its first view.
pub fn create(app: &AppHandle, first: Look) -> tauri::Result<()> {
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(image(first))
        .show_menu_on_left_click(false)
        .on_menu_event(on_menu_event)
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

pub fn set_look(app: &AppHandle, look: Look) -> tauri::Result<()> {
    match app.tray_by_id(TRAY_ID) {
        Some(tray) => tray.set_icon(Some(image(look))),
        None => Ok(()),
    }
}

pub fn set_tooltip(app: &AppHandle, tooltip: &str) -> tauri::Result<()> {
    match app.tray_by_id(TRAY_ID) {
        Some(tray) => tray.set_tooltip(Some(tooltip)),
        None => Ok(()),
    }
}

/// a menu with each item created in its availability: Windows does not grey an item that
/// is disabled in place, so an unavailable one is made unavailable.
fn build_menu(app: &AppHandle, menu: &Menu) -> tauri::Result<TauriMenu<tauri::Wry>> {
    let built = TauriMenu::new(app)?;
    built.append(&MenuItem::with_id(
        app,
        SUMMARY_ID,
        &menu.summary,
        false,
        None::<&str>,
    )?)?;
    built.append(&PredefinedMenuItem::separator(app)?)?;
    for item in &menu.items {
        let Some(id) = menu_id(&item.action) else {
            continue;
        };
        built.append(&MenuItem::with_id(
            app,
            id,
            &item.label,
            item.available,
            None::<&str>,
        )?)?;
    }
    Ok(built)
}

pub fn set_menu(app: &AppHandle, menu: &Menu) -> tauri::Result<()> {
    match app.tray_by_id(TRAY_ID) {
        Some(tray) => tray.set_menu(Some(build_menu(app, menu)?)),
        None => Ok(()),
    }
}

fn on_menu_event(app: &AppHandle, event: MenuEvent) {
    let id = event.id().as_ref().to_string();
    let app = app.clone();
    // windows are made off the event loop's thread, which a window's creation would
    // otherwise block
    tauri::async_runtime::spawn(async move { run_menu_item(&app, &id).await });
}

async fn run_menu_item(app: &AppHandle, id: &str) {
    let state = app.state::<AppState>();
    let Some(item) = state.latest.menu().and_then(|menu| {
        menu.items
            .into_iter()
            .find(|item| menu_id(&item.action) == Some(id))
    }) else {
        return;
    };
    let outcome = match choose(&item, main_window_shown(app)) {
        Chosen::Open => {
            show_main_window(app);
            Ok(())
        }
        Chosen::Run(act) => state.handle.act(act).await,
        Chosen::TryAgain => state.handle.try_again().await,
        Chosen::Quit => {
            state.handle.quit();
            Ok(())
        }
        Chosen::Ask {
            question,
            then,
            place: Place::MainWindow,
        } => confirmation::ask_in_main_window(app, &question, then).map_err(|e| e.to_string()),
        Chosen::Ask {
            question,
            then,
            place: Place::OwnWindow,
        } => {
            confirmation::ask_in_own_window(app, &state, question, then).map_err(|e| e.to_string())
        }
        Chosen::Nothing => Ok(()),
    };
    if let Err(words) = outcome {
        state
            .log
            .warn(&format!("The menu item {id} did not run: {words}"));
    }
}
