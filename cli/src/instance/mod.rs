//! What makes `start` one instance per machine: the state it is in, the lock that
//! keeps a second one out, the channel other terminals use to talk to it, the
//! runtime core it supervises, and its log file. Tests also drive the layer through
//! its seams.

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
