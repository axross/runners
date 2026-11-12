//! the management CLI for the runner hosts in this repository; see
//! `docs/operations/cli.md`.

pub mod affinity;
pub mod agent;
pub mod build;
pub mod checkout;
pub mod command;
pub mod control;
pub mod convert;
pub mod de;
pub mod diagnostics;
pub mod edit;
pub mod fold;
pub mod golden;
pub mod image;
pub mod instance;
pub mod json;
pub mod lifecycle_settings;
pub mod macos;
#[cfg(unix)]
pub mod private;
pub mod profile;
pub mod runtime;
#[cfg(unix)]
pub mod shim;
pub mod start;
pub mod summary;
pub mod token_store;
pub mod validate;
#[cfg(unix)]
pub mod vm_shim;
pub mod windows_host;
