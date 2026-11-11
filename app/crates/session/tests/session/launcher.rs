//! starting the agent from the app's own executable: never from a missing or changed
//! file, with the agent's own argument, and detached with no console window.

use std::fs;
use std::sync::Arc;

use marshal_cli::agent::Launch;
use marshal_session::{AGENT_CREATION_FLAGS, Launcher, OwnExecutable};

use crate::support::{MemoryLog, temp_dir};

fn launcher(path: &std::path::Path) -> (OwnExecutable, Arc<MemoryLog>) {
    let log = Arc::new(MemoryLog::default());
    (OwnExecutable::at(path.to_path_buf(), log.clone()), log)
}

#[test]
fn a_missing_executable_is_never_started() {
    let directory = temp_dir("launcher-missing");
    let (launcher, _) = launcher(&directory.join("marshal-app"));
    let error = launcher.start_agent(Launch::Running).unwrap_err();
    assert!(error.contains("is missing"), "{error}");
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn an_executable_deleted_after_the_app_started_is_never_started() {
    let directory = temp_dir("launcher-deleted");
    let path = directory.join("marshal-app");
    fs::write(&path, b"first").unwrap();
    let (launcher, _) = launcher(&path);
    fs::remove_file(&path).unwrap();
    let error = launcher.start_agent(Launch::Running).unwrap_err();
    assert!(error.contains("cannot be read"), "{error}");
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn an_executable_replaced_after_the_app_started_is_never_started() {
    let directory = temp_dir("launcher-replaced");
    let path = directory.join("marshal-app");
    fs::write(&path, b"first").unwrap();
    let (launcher, _) = launcher(&path);
    // another file at the same path, as an installer leaves it
    let other = directory.join("new");
    fs::write(&other, b"second, and longer").unwrap();
    fs::rename(&other, &path).unwrap();
    let error = launcher.start_agent(Launch::Stopped).unwrap_err();
    assert!(
        error.contains("has changed since the app started"),
        "{error}"
    );
    fs::remove_dir_all(directory).unwrap();
}

#[cfg(unix)]
#[test]
fn the_unchanged_executable_is_started_in_agent_mode_with_the_launch_as_its_argument() {
    use std::os::unix::fs::PermissionsExt;
    let directory = temp_dir("launcher-starts");
    let path = directory.join("marshal-app");
    let out = directory.join("arguments");
    fs::write(
        &path,
        format!("#!/bin/sh\necho \"$@\" >> '{}'\n", out.display()),
    )
    .unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    let (launcher, log) = launcher(&path);
    launcher.start_agent(Launch::Running).unwrap();
    launcher.start_agent(Launch::Stopped).unwrap();
    let mut waited = 0;
    let text = loop {
        let text = fs::read_to_string(&out).unwrap_or_default();
        if text.lines().count() >= 2 || waited > 500 {
            break text;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
        waited += 1;
    };
    let mut lines: Vec<&str> = text.lines().collect();
    lines.sort_unstable();
    assert_eq!(lines, ["agent running", "agent stopped"]);
    // the process is reaped and its end logged
    let mut waited = 0;
    while log.lines().len() < 2 && waited < 500 {
        std::thread::sleep(std::time::Duration::from_millis(10));
        waited += 1;
    }
    assert!(
        log.lines()
            .iter()
            .all(|line| line.contains("The agent process ended")),
        "{:?}",
        log.lines()
    );
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn on_windows_the_agent_has_no_console_window_and_a_process_group_of_its_own() {
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    assert_eq!(
        AGENT_CREATION_FLAGS,
        DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW
    );
}
