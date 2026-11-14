//! Marshal's notification-area app: the commands its windows call, the icon and the
//! menu, and the wiring of the session that talks to the agent.

mod app;
pub mod commands;
pub mod confirmation;
mod icon;
#[cfg(target_os = "macos")]
mod login_item;
mod screen;
pub mod state;
mod tray;
#[cfg(windows)]
mod updates;

pub use app::{HIDDEN_EVENT, MAIN_WINDOW, hide_main_window, on_window_event, opener, run};
