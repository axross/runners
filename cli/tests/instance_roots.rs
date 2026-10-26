//! The lock, the control channel and the logs cannot be moved in a release build.
//!
//! A debug build moves them to a test root when asked; a release build ignores the
//! request and keeps the host's fixed locations, so a release binary cannot be
//! pointed at a lock, a channel or a log directory an attacker prepared. Linux has
//! no host roots, so there an ignored request reads as an unsupported host, and a
//! release build's `start` refuses to run. CI runs this file twice:
//! in the debug profile with the rest of the tests, and in the release profile with
//! `MARSHAL_EXPECT_RELEASE=1`, which makes the file fail if it was built in the
//! wrong profile.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::PathBuf;

use marshal_cli::instance::roots::Roots;
#[cfg(target_os = "linux")]
use marshal_cli::instance::roots::RootsError;

fn test_root() -> PathBuf {
    std::env::temp_dir().join("marshal-instance-roots-test")
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

#[cfg(target_os = "linux")]
#[test]
fn linux_has_no_host_roots() {
    assert_eq!(Roots::of_this_host(), Err(RootsError::UnsupportedHost));
    assert_eq!(Roots::resolve(None), Err(RootsError::UnsupportedHost));
}

#[cfg(target_os = "macos")]
#[test]
fn the_macos_roots_are_the_shared_lock_file_and_a_directory_in_the_library() {
    let roots = Roots::resolve(None).unwrap();
    let channel_directory = std::env::home_dir()
        .unwrap()
        .join("Library/Application Support/marshal");
    assert_eq!(
        roots.lock_file(),
        std::path::Path::new("/Users/Shared/.marshal-instance.lock")
    );
    assert_eq!(roots.channel_directory(), channel_directory);
    assert_eq!(roots.socket_path(), channel_directory.join("control.sock"));
    assert_eq!(
        roots.log_directory(),
        std::env::home_dir().unwrap().join("Library/Logs/marshal")
    );
    assert_eq!(Roots::of_this_host(), Ok(roots));
}

#[cfg(windows)]
#[test]
fn the_windows_roots_are_the_pipe_names_without_a_suffix() {
    let roots = Roots::resolve(None).unwrap();
    assert_eq!(roots.lock_pipe_name(), r"\\.\pipe\marshal-instance");
    assert_eq!(
        roots.control_pipe_name("S-1-5-21-1"),
        r"\\.\pipe\marshal-control-S-1-5-21-1"
    );
    assert_eq!(
        roots.log_directory(),
        std::path::Path::new(&std::env::var_os("LOCALAPPDATA").unwrap())
            .join("marshal")
            .join("logs")
    );
    assert_eq!(Roots::of_this_host(), Ok(roots));
}

#[cfg(debug_assertions)]
#[test]
fn a_debug_build_moves_the_roots_to_a_test_root() {
    let root = test_root();
    assert_eq!(Roots::resolve(Some(&root)), Ok(Roots::for_tests(&root)));
}

#[cfg(not(debug_assertions))]
#[test]
fn a_release_build_ignores_a_test_root() {
    let resolved = Roots::resolve(Some(&test_root()));
    #[cfg(target_os = "linux")]
    assert_eq!(resolved, Err(RootsError::UnsupportedHost));
    #[cfg(not(target_os = "linux"))]
    assert_eq!(resolved, Roots::of_this_host());
}

#[cfg(all(target_os = "linux", not(debug_assertions)))]
#[test]
fn a_release_build_on_linux_does_not_start_even_with_a_test_root() {
    let root = test_root().join("release-start");
    std::fs::create_dir_all(&root).unwrap();
    let config = root.join("host.json");
    std::fs::write(
        &config,
        br#"{"imageName":"actions-runner:local","repositories":[{"owner":"o","repository":"r",
        "name":"r","slots":1,"tokenPath":"C:\\r.token","volumes":[]}]}"#,
    )
    .unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_marshal"))
        .args(["start", "--profile", "axpc", "--config"])
        .arg(&config)
        .env("MARSHAL_TEST_ROOT", &root)
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains("not a supported host"), "{stderr}");
    assert!(!root.join("logs").exists(), "nothing was written");
}
