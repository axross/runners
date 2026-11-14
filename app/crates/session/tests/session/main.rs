//! the session against fakes and a clock that jumps: the connection lifecycle, the
//! actions, the token path, the settings path, the launcher and the log.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod actions;
mod choose;
mod github;
mod launcher;
mod lifecycle;
mod log;
mod revision;
mod settings;
mod support;
mod tokens;
mod update;
mod wall;
