use std::ffi::OsString;
use std::path::PathBuf;

use marshal_cli::agent::Launch;
use marshal_modes::{Mode, Overrides, agent_arguments, run};

fn args(words: &[&str]) -> Vec<OsString> {
    words.iter().map(OsString::from).collect()
}

#[test]
fn no_argument_starts_the_app() {
    assert_eq!(Mode::of(args(&["marshal-app"])), Mode::App);
}

#[test]
fn an_argument_that_names_no_mode_starts_the_app() {
    assert_eq!(Mode::of(args(&["marshal-app", "--show"])), Mode::App);
    assert_eq!(Mode::of(args(&["marshal-app", "start"])), Mode::App);
}

#[test]
fn the_agent_arguments_start_the_agent_in_each_launch() {
    for launch in [Launch::Running, Launch::Stopped] {
        let mut words = vec!["marshal-app"];
        words.extend(agent_arguments(launch));
        assert_eq!(Mode::of(args(&words)), Mode::Agent(launch));
    }
}

#[test]
fn an_agent_without_a_valid_launch_is_refused_not_started_as_the_app() {
    for words in [
        &["marshal-app", "agent"][..],
        &["marshal-app", "agent", "paused"],
        &["marshal-app", "agent", "running", "extra"],
    ] {
        assert_eq!(Mode::of(args(words)), Mode::Refused, "{words:?}");
    }
}

#[test]
fn the_exporter_runs_through_the_library_with_the_program_name_first() {
    let words = ["marshal-app", "export", "--pool", "p"];
    assert_eq!(Mode::of(args(&words)), Mode::Command(args(&words)));
}

#[test]
fn the_exporter_reaches_the_library_and_is_refused_for_missing_arguments() {
    let mode = Mode::of(args(&["marshal-app", "export"]));
    assert!(matches!(mode, Mode::Command(_)));
    assert_eq!(
        format!("{:?}", run(mode)),
        format!("{:?}", std::process::ExitCode::from(2))
    );
}

#[cfg(unix)]
#[test]
fn the_shim_runs_through_the_library_on_unix() {
    let words = ["marshal-app", "shim", "--name", "n", "--", "docker"];
    assert_eq!(Mode::of(args(&words)), Mode::Command(args(&words)));
}

#[cfg(unix)]
#[test]
fn the_shim_reaches_the_library_and_is_refused_for_missing_arguments() {
    let mode = Mode::of(args(&["marshal-app", "shim"]));
    assert!(matches!(mode, Mode::Command(_)));
    assert_eq!(
        format!("{:?}", run(mode)),
        format!("{:?}", std::process::ExitCode::from(2))
    );
}

#[cfg(unix)]
#[test]
fn the_vm_shim_runs_through_the_library_on_unix() {
    let words = [
        "marshal-app",
        "vm-shim",
        "--name",
        "n",
        "--diagnostic-pool",
        "p",
        "--",
        "tart",
    ];
    assert_eq!(Mode::of(args(&words)), Mode::Command(args(&words)));
}

#[cfg(unix)]
#[test]
fn the_vm_shim_reaches_the_library_and_is_refused_for_missing_arguments() {
    let mode = Mode::of(args(&["marshal-app", "vm-shim"]));
    assert!(matches!(mode, Mode::Command(_)));
    assert_eq!(
        format!("{:?}", run(mode)),
        format!("{:?}", std::process::ExitCode::from(2))
    );
}

#[cfg(windows)]
#[test]
fn the_shims_are_not_modes_on_windows() {
    for word in ["shim", "vm-shim"] {
        assert_eq!(Mode::of(args(&["marshal-app", word])), Mode::App);
    }
}

#[test]
fn a_refused_mode_exits_with_the_usage_code() {
    assert_eq!(
        format!("{:?}", run(Mode::Refused)),
        format!("{:?}", std::process::ExitCode::from(2))
    );
}

fn lookup(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<OsString> {
    move |name| {
        pairs
            .iter()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| OsString::from(value))
    }
}

#[test]
fn the_agent_overrides_are_read_from_the_library_test_variables() {
    let overrides = Overrides::from_lookup(lookup(&[
        ("MARSHAL_TEST_ROOT", "scratch"),
        ("MARSHAL_TEST_DOCKER", "fake-docker"),
        ("MARSHAL_TEST_GITHUB_API", "http://127.0.0.1:1"),
    ]));
    assert_eq!(overrides.root, Some(PathBuf::from("scratch")));
    assert_eq!(overrides.docker, Some(OsString::from("fake-docker")));
    assert_eq!(overrides.github_api.as_deref(), Some("http://127.0.0.1:1"));
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

#[test]
fn no_variable_means_no_override() {
    assert_eq!(Overrides::from_lookup(lookup(&[])), Overrides::default());
}

#[cfg(not(debug_assertions))]
#[test]
fn a_release_build_ignores_the_variables() {
    // CI's release step sets all three, so the test fails if a release build reads them
    if std::env::var("MARSHAL_EXPECT_RELEASE").as_deref() == Ok("1") {
        for name in [
            marshal_modes::TEST_ROOT_VARIABLE,
            marshal_modes::TEST_DOCKER_VARIABLE,
            marshal_modes::TEST_GITHUB_API_VARIABLE,
        ] {
            assert!(
                std::env::var_os(name).is_some(),
                "the release step sets {name}"
            );
        }
    }
    assert_eq!(Overrides::from_environment(), Overrides::default());
}
