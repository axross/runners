//! the Tart backend and the `tart` client library under it: exact argument lists, a
//! bounded process wrapper, VM names and the reader of Tart's listing, the golden VM
//! check, the guest agent, the process that holds a VM's `tart run`, and the teardown
//! of a VM within a budget. `start` runs the axmc Linux pools with it, and `build` makes the
//! golden VM with it.

pub mod args;
pub mod backend;
pub mod boot;
pub mod client;
pub mod golden_check;
pub mod guest;
pub mod hold;
pub mod list;
pub mod names;
pub mod teardown;

pub use backend::{BOOT_DEADLINE, RELEASE_RESERVE, TART_POLL_INTERVAL, TartBackend, TartTimings};
pub use client::{
    Captured, MAX_OUTPUT_BYTES, RunningVm, TartClient, TartConfig, TartError, TartLimits,
};
pub use hold::{HeldVm, VmLauncher};
pub use list::{VmEntry, VmSource, VmState};
pub use names::GOLDEN_VM_VERSION;
