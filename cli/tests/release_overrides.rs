//! the overrides that point the axmc host at fake programs move only in a debug
//! build: a release build ignores each request, so a release binary cannot be made
//! to run another `tart`, `route`, `arp` or `caffeinate`, to read its signals from a
//! directory, or to accept a Softnet file it would refuse.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::ffi::OsStr;
use std::path::Path;

use marshal_cli::macos::keep_awake::CAFFEINATE_PROGRAM;
use marshal_cli::macos::network::{ARP_PROGRAM, ROUTE_PROGRAM};
use marshal_cli::macos::probe;
use marshal_cli::macos::signals::{Signals, resolve_scripted_directory};
use marshal_cli::macos::softnet::{OwnerMode, resolve_owner_mode_override};
use marshal_cli::runtime::tart::client::{TART_PROGRAM, resolve_program};

#[test]
fn the_profile_is_the_one_ci_asked_for() {
    let debug_profile = cfg!(debug_assertions);
    match std::env::var("MARSHAL_EXPECT_RELEASE").as_deref() {
        Ok("1") => assert!(!debug_profile, "this test was built in the debug profile"),
        Ok("0") => assert!(debug_profile, "this test was built in the release profile"),
        _ => {}
    }
}

#[test]
fn the_tart_override_names_the_client_in_a_debug_build_only() {
    let fake = OsStr::new("/tmp/fake-tart");
    let resolved = resolve_program(Some(fake));
    if cfg!(debug_assertions) {
        assert_eq!(resolved, fake);
    } else {
        assert_eq!(resolved, TART_PROGRAM);
    }
    assert_eq!(resolve_program(None), TART_PROGRAM);
}

#[test]
fn the_route_arp_and_caffeinate_overrides_name_the_program_in_a_debug_build_only() {
    let fake = OsStr::new("/tmp/fake-macos");
    for release in [ROUTE_PROGRAM, ARP_PROGRAM, CAFFEINATE_PROGRAM] {
        let resolved = probe::resolve_program(Some(fake), release);
        if cfg!(debug_assertions) {
            assert_eq!(resolved, fake);
        } else {
            assert_eq!(resolved, release);
        }
        assert_eq!(probe::resolve_program(None, release), release);
    }
}

#[test]
fn the_signal_directory_override_is_read_in_a_debug_build_only() {
    let directory = OsStr::new("/tmp/signals");
    let resolved = resolve_scripted_directory(Some(directory));
    if cfg!(debug_assertions) {
        assert_eq!(resolved.as_deref(), Some(Path::new("/tmp/signals")));
    } else {
        assert_eq!(resolved, None);
    }
    assert_eq!(resolve_scripted_directory(None), None);
    assert_eq!(
        Signals::from_scripted_directory(Some(directory)).is_scripted(),
        cfg!(debug_assertions)
    );
    assert!(!Signals::from_scripted_directory(None).is_scripted());
}

#[test]
fn the_softnet_owner_and_mode_override_is_read_in_a_debug_build_only() {
    let resolved = resolve_owner_mode_override(Some(OsStr::new("0:4755")));
    if cfg!(debug_assertions) {
        assert_eq!(
            resolved,
            Ok(Some(OwnerMode {
                uid: 0,
                mode: 0o4755
            }))
        );
    } else {
        assert_eq!(resolved, Ok(None));
    }
    assert_eq!(resolve_owner_mode_override(None), Ok(None));
}

#[test]
fn a_release_build_ignores_a_softnet_override_that_a_debug_build_refuses() {
    let resolved = resolve_owner_mode_override(Some(OsStr::new("not-an-owner-and-mode")));
    if cfg!(debug_assertions) {
        assert!(resolved.is_err(), "{resolved:?}");
    } else {
        assert_eq!(resolved, Ok(None));
    }
}
