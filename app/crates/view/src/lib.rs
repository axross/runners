//! what the window, the menu and the notification-area icon show for an agent
//! snapshot; it depends on no operating system, so its tests run everywhere.

mod icon;

pub use icon::{IconState, icon_state, tooltip};
