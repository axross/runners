//! the agent: the instance in a resident form, started stopped or running, that
//! stops and starts all runners, builds the runner image, quits, and stays up when
//! its configuration, image or pools fail; it serves protocol version 2 on the
//! instance's channel and reads tokens from the token store; `docs/operations/cli.md`
//! describes it.

pub mod api;
pub mod builder;
pub mod channel;
mod core;
pub mod executable;
pub mod log;
pub mod machine;
pub mod run;
pub mod server;
pub mod snapshot;
mod state;
pub mod tokens;
mod view;
pub mod wire;

pub use channel::{
    AgentChannel, CallError, Client, ConnectError, Push, RawStream, Welcome, connect, connect_raw,
};
pub use machine::{EXECUTABLE_CHANGED, Ended, Launch, QUIT};
pub use run::{AgentOptions, Resident, run, run_resident};
/// the reasons the agent is stopped for; a reader of the snapshot compares a `reason`'s
/// `code` with its constants.
pub use state::Reason as StopReason;
