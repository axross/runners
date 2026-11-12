//! the app against a real agent, real processes and real channels in a scratch host: the
//! tests only the Windows job runs, as no other gate builds `marshal-app`. A debug build only,
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
