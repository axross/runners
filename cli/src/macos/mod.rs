//! the host checks the macOS runtime makes before a VM boots: the versions of Tart
//! and Softnet, and Softnet's file. The code compiles on every platform so the
//! parsers and judgments run in the Linux tests; only the file's owner and mode need
//! Unix. `docs/operations/cli.md` describes the checks.

pub mod prerequisites;
pub mod softnet;
pub mod version;
