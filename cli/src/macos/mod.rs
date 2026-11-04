//! what the axmc host reads from macOS before and while its VMs run: the versions
//! of Tart and Softnet and Softnet's file, checked before a VM boots; and the signals
//! that stop every runner, and the keep-awake hold. The code compiles on every
//! platform so the parsers and judgments run in the Linux tests; only the file's
//! owner and mode need Unix. `docs/operations/cli.md` describes both.

pub mod keep_awake;
pub mod mac;
pub mod network;
pub mod prerequisites;
pub mod probe;
pub mod reason;
pub mod signals;
pub mod sleep;
pub mod softnet;
pub mod version;
pub mod watch;

#[cfg(any(debug_assertions, test))]
#[doc(hidden)]
pub mod fake;
#[cfg(debug_assertions)]
#[doc(hidden)]
pub mod scripted;
