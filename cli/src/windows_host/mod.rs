//! what the axpc agent reads from and writes to Windows for the lifecycle settings:
//! whether Windows slept, the primary network, the keep-awake request, and the Run key
//! value that starts Marshal at sign-in. The judgments compile on every platform so the
//! Linux tests run them; only the calls into Windows need it. `docs/operations/cli.md`
//! describes them.

pub mod keep_awake;
pub mod monitor;
pub mod network;
pub mod run_key;
pub mod signals;
pub mod sleep;
