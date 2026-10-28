//! the management CLI for the runner hosts in this repository; see
//! `docs/operations/cli.md`.

pub mod affinity;
pub mod build;
pub mod checkout;
pub mod command;
pub mod control;
pub mod de;
pub mod diagnostics;
pub mod fold;
pub mod image;
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
