//! the view-state tests: recorded snapshots in `../fixtures` turned into the window's
//! texts, each action's availability and reason, the menu, the diagnostics, the token
//! states, the tooltip and the icon state, for every state the boards draw and the
//! ones they do not.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod availability;
mod configuration;
mod diagnostics;
mod github;
mod newer_agent;
mod privacy;
mod settings;
mod states;
mod support;
mod time;
mod tokens;
mod update;
