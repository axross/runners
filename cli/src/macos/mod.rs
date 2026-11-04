//! what the macOS VM runtime builds on that needs no process call: the names of its
//! VMs, and the count of macOS VMs against the host's limit of two; see
//! `docs/operations/cli.md`.

pub mod count;
pub mod linux_forms;
pub mod name;
pub mod place;
