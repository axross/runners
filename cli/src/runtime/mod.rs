//! What runs the pools: the container backend, registration, and the slot loop.
//!
//! Everything here is independent of the operating system the CLI runs on, apart
//! from how the container backend contains its job clients. `start` runs it through
//! the instance layer; tests also drive it through a backend seam, a fake `docker`
//! and a loopback GitHub stub.

pub mod backend;
pub mod board;
pub mod clock;
pub mod diagnostic;
pub mod docker;
pub mod docker_args;
#[cfg(any(debug_assertions, test))]
#[doc(hidden)]
pub mod fake;
pub mod image;
pub mod log;
pub mod name;
pub mod pool;
pub mod registration;
pub mod retained;
pub mod scrub;
pub mod secret;
pub mod token;
pub mod worker;
