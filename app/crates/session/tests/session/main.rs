//! the session against fakes and a clock that jumps: the connection lifecycle, the
//! actions, the token path, the launcher, the log and the policy files.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod actions;
mod choose;
mod launcher;
mod lifecycle;
mod log;
mod policy;
mod support;
mod tokens;
mod wall;
