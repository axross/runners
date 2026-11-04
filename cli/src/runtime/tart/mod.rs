//! the `tart` client library the VM runtime calls: exact argument lists, a bounded
//! process wrapper, VM names and the reader of Tart's listing; no `start` or
//! `build` command calls it yet.

pub mod args;
pub mod client;
pub mod list;
pub mod names;

pub use client::{
    Captured, MAX_OUTPUT_BYTES, RunningVm, TartClient, TartConfig, TartError, TartLimits,
};
pub use list::{VmEntry, VmSource, VmState};
pub use names::GOLDEN_VM_VERSION;
