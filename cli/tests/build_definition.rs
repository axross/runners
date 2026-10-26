//! `build` sends only the definition embedded in a release build.
//!
//! A debug build reads a test definition from a directory when asked, for the
//! real-Docker test; a release build ignores the request, so a release binary
//! cannot be pointed at a Dockerfile an attacker prepared. Linux has no host roots,
//! so a release build's `build` there refuses to run even with a test root. CI runs
//! this file twice: in the debug profile with the rest of the tests, and in the
//! release profile with `MARSHAL_EXPECT_RELEASE=1`, which makes the file fail if
//! it was built in the wrong profile.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::PathBuf;

use marshal_cli::image::definition::{Definition, resolve_definition};

fn test_definition() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/build-definition")
}

#[test]
fn the_profile_is_the_one_ci_asked_for() {
    let debug_profile = cfg!(debug_assertions);
    match std::env::var("MARSHAL_EXPECT_RELEASE").as_deref() {
        Ok("1") => assert!(!debug_profile, "this test was built in the debug profile"),
        Ok("0") => assert!(debug_profile, "this test was built in the release profile"),
        _ => {}
    }
}

#[cfg(debug_assertions)]
#[test]
fn a_debug_build_reads_the_test_definition() {
    let definition = resolve_definition(Some(&test_definition())).unwrap();
    let paths: Vec<&str> = definition
        .files
        .iter()
        .map(|file| file.path.as_str())
        .collect();
    assert_eq!(paths, ["Dockerfile"]);
    assert_eq!(
        definition.files[0].bytes,
        std::fs::read(test_definition().join("Dockerfile")).unwrap()
    );
    assert_ne!(definition, Definition::embedded());
}

#[cfg(not(debug_assertions))]
#[test]
fn a_release_build_ignores_the_test_definition() {
    assert_eq!(
        resolve_definition(Some(&test_definition())).unwrap(),
        Definition::embedded()
    );
}

#[cfg(all(target_os = "linux", not(debug_assertions)))]
#[test]
fn a_release_build_on_linux_does_not_build_even_with_a_test_root() {
    let root = std::env::temp_dir().join("marshal-build-definition-test");
    std::fs::create_dir_all(&root).unwrap();
    let config = root.join("host.json");
    std::fs::write(
        &config,
        br#"{"imageName":"actions-runner:local","repositories":[{"owner":"o","repository":"r",
        "name":"r","slots":1,"tokenPath":"C:\\r.token","volumes":[]}]}"#,
    )
    .unwrap();
    // a docker that cannot exist, so a release build that ignored its override and
    // reached Docker anyway would still not build anything on this host
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_marshal"))
        .args(["build", "--profile", "axpc", "--config"])
        .arg(&config)
        .env("MARSHAL_TEST_ROOT", &root)
        .env("MARSHAL_TEST_DOCKER", root.join("no-docker"))
        .env("MARSHAL_TEST_BUILD_DEFINITION", test_definition())
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains("not a supported host"), "{stderr}");
    let written: Vec<_> = std::fs::read_dir(&root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(written, ["host.json"], "nothing was written");
}
