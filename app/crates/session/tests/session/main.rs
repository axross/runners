//! the session against fakes and a clock that jumps: the connection lifecycle, the
//! actions, the token path, the launcher, the log and the login item.
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
mod login_item;
mod revision;
mod support;
mod tokens;
mod update;
mod wall;
