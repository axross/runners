//! the app against a real agent, real processes and real channels in a scratch host: the
//! tests the Windows job runs, which also run on Linux and macOS. A debug build only,
//! as the overrides that move the agent to a scratch host are.
#![cfg(debug_assertions)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod agent;
mod channel;
mod exporter;
mod squatter;
mod support;
mod token;
