//! the GitHub base address and the `docker` client cannot be moved in a release
//! build: a debug build accepts `http://127.0.0.1:<port>` for the loopback tests
//! and any client the tests name, but a release build ignores either request, so a
//! release binary cannot send a token anywhere but GitHub or run another program
//! as its `docker` client.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::ffi::OsStr;

use marshal_cli::runtime::docker::{DOCKER_PROGRAM, resolve_program};
use marshal_cli::runtime::registration::{
    GITHUB_API, GitHubRegistrar, HttpLimits, resolve_base_url,
};

const LOOPBACK: &str = "http://127.0.0.1:34567";

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
fn the_override_moves_the_base_address_in_a_debug_build_only() {
    let resolved = resolve_base_url(Some(LOOPBACK));
    if cfg!(debug_assertions) {
        assert_eq!(resolved.unwrap(), LOOPBACK);
    } else {
        assert_eq!(resolved.unwrap(), GITHUB_API);
    }
}

#[test]
fn a_release_build_ignores_every_override_even_one_it_would_otherwise_refuse() {
    for requested in [
        "http://127.0.0.1:34567",
        "http://evil.example:80",
        "https://evil.example",
        "http://127.0.0.1",
        "",
    ] {
        let resolved = resolve_base_url(Some(requested));
        if cfg!(debug_assertions) {
            assert_eq!(resolved.is_ok(), requested == LOOPBACK, "{requested}");
        } else {
            assert_eq!(resolved.unwrap(), GITHUB_API, "{requested}");
        }
    }
    assert_eq!(resolve_base_url(None).unwrap(), GITHUB_API);
}

#[test]
fn the_registrar_built_with_an_override_targets_github_in_a_release_build() {
    let registrar = GitHubRegistrar::new(Some(LOOPBACK), HttpLimits::default()).unwrap();
    if cfg!(debug_assertions) {
        assert_eq!(registrar.base_url(), LOOPBACK);
    } else {
        assert_eq!(registrar.base_url(), GITHUB_API);
    }
}

#[test]
fn the_docker_override_names_the_client_in_a_debug_build_only() {
    let fake = OsStr::new("/tmp/fake-docker");
    let resolved = resolve_program(Some(fake));
    if cfg!(debug_assertions) {
        assert_eq!(resolved, fake);
    } else {
        assert_eq!(resolved, DOCKER_PROGRAM);
    }
    assert_eq!(resolve_program(None), DOCKER_PROGRAM);
}
