//! what the axpc agent reads from and writes to Windows for the lifecycle settings:
//! whether Windows slept, and the Run key value that starts Marshal at sign-in. The
//! judgments compile on every platform so the Linux tests run them; only the calls
//! into Windows need it. `docs/operations/cli.md` describes both.

pub mod network;
pub mod run_key;
pub mod sleep;
