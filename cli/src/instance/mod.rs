//! What makes `start` one instance per machine: the state it is in, the lock that
//! keeps a second one out, and the channel other terminals use to talk to it.
//!
//! Nothing here is reachable from a command yet; the commands arrive in a later
//! pull request of the same phase, and tests drive the layer through its seams.

#[doc(hidden)]
pub mod fake;
pub mod lifecycle;
pub mod lock;
pub mod protocol;
pub mod restart;
pub mod roots;
pub mod source;
#[cfg(windows)]
pub mod windows;
