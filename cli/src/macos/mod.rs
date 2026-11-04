//! what the axmc host reads from macOS before its VMs run: the versions of Tart and
//! Softnet and Softnet's file, checked before a VM boots. The code compiles on every
//! platform so the parsers and judgments run in the Linux tests; only the file's
//! owner and mode need Unix. `docs/operations/cli.md` describes it.

pub mod prerequisites;
pub mod softnet;
pub mod version;
