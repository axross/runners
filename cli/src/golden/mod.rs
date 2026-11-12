//! the assets that provision a golden VM: its pins, the launcher and the provisioning
//! scripts it embeds, the check of Docker's signing key, and the guest calls that
//! run them. Nothing here starts a process; `build` sequences the calls.
//!
//! The calls run in this order, each through the guest agent of the booted build VM:
//!
//! 1. [`steps::prepare_call`]
//! 2. [`steps::fetch_key_call`]
//! 3. [`steps::list_key_call`], whose standard output goes to
//!    [`key::check_key_listing`]; a refusal fails the build
//! 4. [`steps::install_docker_call`], which takes the proof the check returns
//! 5. [`steps::install_launcher_call`]
//! 6. [`steps::disable_ssh_call`]

pub mod key;
pub mod pins;
pub mod steps;
