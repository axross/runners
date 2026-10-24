//! The management CLI for the runner hosts in this repository.
//!
//! `validate` is a pure function from a configuration file's bytes and a host
//! profile to either a plan or an ordered list of errors; the command layer reads
//! the file, finds the checkout, prints, and sets the exit code.

pub mod affinity;
pub mod checkout;
pub mod command;
pub mod de;
pub mod fold;
pub mod json;
pub mod profile;
pub mod runtime;
pub mod summary;
pub mod validate;
