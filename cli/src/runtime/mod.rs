//! What runs the pools: the container backend, registration, and the slot loop.
//!
//! Everything here is independent of the operating system the CLI runs on, and
//! nothing here is reachable from a command yet; tests drive it through a backend
//! seam, a fake `docker` and a loopback GitHub stub.

pub mod backend;
pub mod clock;
pub mod docker;
pub mod docker_args;
pub mod fake;
pub mod log;
pub mod name;
pub mod pool;
pub mod registration;
pub mod scrub;
pub mod secret;
pub mod token;
pub mod worker;
