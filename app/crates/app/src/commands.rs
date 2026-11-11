use marshal_view::{IconState, icon_state, tooltip};
use serde::Serialize;
use tauri::Window;

#[derive(Serialize)]
pub struct View {
    icon: &'static str,
    tooltip: String,
}

fn icon_name(state: IconState) -> &'static str {
    match state {
        IconState::AllUp => "all_up",
        IconState::SomeNotUp => "some_not_up",
        IconState::Stopped => "stopped",
        IconState::StoppedByError => "stopped_by_error",
        IconState::NotInControl => "not_in_control",
    }
}

/// what the window shows; the app does not talk to its agent yet, so it is not in
/// control of the runners.
#[tauri::command]
pub fn read_view() -> View {
    View {
        icon: icon_name(icon_state(None)),
        tooltip: tooltip(None),
    }
}

#[tauri::command]
pub fn hide_window(window: Window) -> Result<(), String> {
    window.hide().map_err(|error| error.to_string())
}
