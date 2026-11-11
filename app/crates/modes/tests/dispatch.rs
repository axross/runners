use std::ffi::OsString;

use marshal_cli::agent::Launch;
use marshal_modes::{Mode, agent_arguments, run};

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

#[cfg(windows)]
#[test]
fn the_shim_is_not_a_mode_on_windows() {
    assert_eq!(Mode::of(args(&["marshal-app", "shim"])), Mode::App);
}

#[test]
fn a_refused_mode_exits_with_the_usage_code() {
    assert_eq!(
        format!("{:?}", run(Mode::Refused)),
        format!("{:?}", std::process::ExitCode::from(2))
    );
}
