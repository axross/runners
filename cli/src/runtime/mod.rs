//! the container backend, registration and the slot loop that run a host's pools;
//! `docs/operations/cli.md` describes the layer.

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
