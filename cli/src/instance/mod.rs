//! what makes `start` one instance per machine: its state, the lock, the control
//! channel, the runtime core it supervises and the log file;
//! `docs/operations/cli.md` describes the layer.

pub mod channel;
#[cfg(any(debug_assertions, test))]
#[doc(hidden)]
pub mod fake;
pub mod lifecycle;
pub mod lock;
pub mod log_file;
pub mod protocol;
pub mod restart;
pub mod roots;
pub mod runtime_core;
pub mod source;
#[cfg(windows)]
pub mod windows;
