//! The management CLI for the runner hosts in this repository.
//!
//! `validate` is a pure function from a configuration file's bytes and a host
//! profile to either a plan or an ordered list of errors; the command layer reads
//! the file, finds the checkout, prints, and sets the exit code. `start` runs the
//! host's pools through the instance layer and the runtime, and `stop`, `restart`
//! and `status` talk to it over the control channel.

pub mod affinity;
pub mod checkout;
pub mod command;
pub mod control;
pub mod de;
pub mod fold;
pub mod instance;
pub mod json;
#[cfg(unix)]
pub mod private;
pub mod profile;
pub mod runtime;
#[cfg(unix)]
pub mod shim;
pub mod start;
pub mod summary;
pub mod validate;
