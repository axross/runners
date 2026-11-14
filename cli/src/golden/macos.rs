//! the guest calls that provision the golden macOS VM, each through the guest agent
//! of the booted macOS build VM, in this order:
//!
//! 1. [`remove_runner_call`]
//! 2. [`install_runner_call`], which refuses an archive whose SHA-256 is not the
//!    runner record's
//! 3. [`install_launcher_call`]
//! 4. [`disable_remote_access_call`]
//! 5. [`disable_updates_call`]
//! 6. [`flush_disk_call`], before the VM stops
//!
//! The disk keeps the base's layout, recovery partition included: the first Mac
//! check on #80 saw the guest agent grow it at boot.

use std::time::Duration;

use crate::image::runner::DeclaredRunner;

use super::pins::{GUEST_USER, LAUNCHER_PATH};
use super::steps::{GuestCall, script_call};

/// where the golden macOS VM keeps the runner, in the guest user's home.
pub const RUNNER_DIRECTORY: &str = "/Users/admin/actions-runner";

/// the launcher the golden macOS VM starts the runner through.
pub const LAUNCHER: &str = include_str!("scripts/macos/launcher.sh");

const REMOVE_RUNNER: &str = include_str!("scripts/macos/remove-runner.sh");
const INSTALL_RUNNER: &str = include_str!("scripts/macos/install-runner.sh");
const INSTALL_LAUNCHER: &str = include_str!("scripts/macos/install-launcher.sh");
const DISABLE_REMOTE_ACCESS: &str = include_str!("scripts/macos/disable-remote-access.sh");
const DISABLE_UPDATES: &str = include_str!("scripts/macos/disable-updates.sh");

const DOWNLOAD_BUDGET: Duration = Duration::from_secs(6 * 60);
const QUICK_STEP_BUDGET: Duration = Duration::from_secs(60);
const FLUSH_BUDGET: Duration = Duration::from_secs(2 * 60);

/// the release asset of the runner for macOS on arm64.
#[must_use]
pub fn runner_archive_url(version: &str) -> String {
    format!(
        "https://github.com/actions/runner/releases/download/v{version}/actions-runner-osx-arm64-{version}.tar.gz"
    )
}

/// step 1: removes the runner the base ships, of whatever version it was built with.
#[must_use]
pub fn remove_runner_call() -> GuestCall {
    script_call(
        true,
        "marshal-remove-runner",
        REMOVE_RUNNER,
        &[RUNNER_DIRECTORY],
        None,
        QUICK_STEP_BUDGET,
    )
}

/// step 2: downloads the declared runner's osx-arm64 archive, refuses it unless its
/// SHA-256 is the recorded one, and unpacks it for the guest user.
#[must_use]
pub fn install_runner_call(runner: &DeclaredRunner) -> GuestCall {
    let url = runner_archive_url(&runner.version);
    script_call(
        true,
        "marshal-install-runner",
        INSTALL_RUNNER,
        &[&url, &runner.osx_arm64_sha256, RUNNER_DIRECTORY, GUEST_USER],
        None,
        DOWNLOAD_BUDGET,
    )
}

/// step 3: installs the launcher at [`LAUNCHER_PATH`], owned by root, mode 0755.
#[must_use]
pub fn install_launcher_call() -> GuestCall {
    script_call(
        true,
        "marshal-install-launcher",
        INSTALL_LAUNCHER,
        &[LAUNCHER_PATH],
        Some(LAUNCHER.as_bytes().to_vec()),
        QUICK_STEP_BUDGET,
    )
}

/// step 4: turns off Remote Login and Screen Sharing; the guest agent does not use
/// either, so it keeps working.
#[must_use]
pub fn disable_remote_access_call() -> GuestCall {
    script_call(
        true,
        "marshal-disable-remote-access",
        DISABLE_REMOTE_ACCESS,
        &[],
        None,
        QUICK_STEP_BUDGET,
    )
}

/// step 5: turns off the automatic download and installation of updates, and leaves
/// the automatic check on, as plan revision 3 on #80 decided.
#[must_use]
pub fn disable_updates_call() -> GuestCall {
    script_call(
        true,
        "marshal-disable-updates",
        DISABLE_UPDATES,
        &[],
        None,
        QUICK_STEP_BUDGET,
    )
}

/// writes the files the guest has not yet written to its disk. `tart stop` ends the
/// VM without a shutdown inside it, and the first Mac check lost the settings of a
/// VM stopped without this.
#[must_use]
pub fn flush_disk_call() -> GuestCall {
    script_call(true, "marshal-flush-disk", "sync", &[], None, FLUSH_BUDGET)
}

/// the command a job runs: the launcher, with the runner's directory, as the guest
/// agent's user; the JIT configuration goes on its standard input.
#[must_use]
pub fn launch_command() -> Vec<String> {
    vec![LAUNCHER_PATH.to_string(), RUNNER_DIRECTORY.to_string()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::docker_args::JIT_VARIABLE;

    const SCRIPTS: [(&str, &str); 6] = [
        ("launcher", LAUNCHER),
        ("remove-runner", REMOVE_RUNNER),
        ("install-runner", INSTALL_RUNNER),
        ("install-launcher", INSTALL_LAUNCHER),
        ("disable-remote-access", DISABLE_REMOTE_ACCESS),
        ("disable-updates", DISABLE_UPDATES),
    ];

    fn words(list: &[&str]) -> Vec<String> {
        list.iter().map(ToString::to_string).collect()
    }

    fn runner() -> DeclaredRunner {
        DeclaredRunner {
            version: "2.337.0".to_string(),
            osx_arm64_sha256: "5a2cd92908a93d7276a194e1de6008099f3e7946f3f8e14aa7a1a7b4a31fdec2"
                .to_string(),
        }
    }

    #[test]
    fn the_runner_is_downloaded_from_its_release_and_checked_against_the_record() {
        assert_eq!(
            install_runner_call(&runner()).command,
            [
                words(&["sudo", "-n", "bash", "-c"]),
                vec![INSTALL_RUNNER.to_string()],
                words(&[
                    "marshal-install-runner",
                    "https://github.com/actions/runner/releases/download/v2.337.0/actions-runner-osx-arm64-2.337.0.tar.gz",
                    "5a2cd92908a93d7276a194e1de6008099f3e7946f3f8e14aa7a1a7b4a31fdec2",
                    "/Users/admin/actions-runner",
                    "admin",
                ]),
            ]
            .concat()
        );
        assert!(INSTALL_RUNNER.contains("shasum -a 256"));
        let check = INSTALL_RUNNER.find("!= \"$expected\"").unwrap();
        assert!(check < INSTALL_RUNNER.find("tar -xzf").unwrap());
    }

    #[test]
    fn the_other_calls_are_exact() {
        let script = |script: &str, name: &str, arguments: &[&str]| {
            [
                words(&["sudo", "-n", "bash", "-c"]),
                vec![script.to_string()],
                words(&[name]),
                words(arguments),
            ]
            .concat()
        };
        assert_eq!(
            remove_runner_call().command,
            script(
                REMOVE_RUNNER,
                "marshal-remove-runner",
                &["/Users/admin/actions-runner"]
            )
        );
        assert_eq!(
            install_launcher_call().command,
            script(
                INSTALL_LAUNCHER,
                "marshal-install-launcher",
                &["/usr/local/bin/marshal-launch"]
            )
        );
        assert_eq!(
            disable_remote_access_call().command,
            script(DISABLE_REMOTE_ACCESS, "marshal-disable-remote-access", &[])
        );
        assert_eq!(
            disable_updates_call().command,
            script(DISABLE_UPDATES, "marshal-disable-updates", &[])
        );
        assert_eq!(
            flush_disk_call().command,
            script("sync", "marshal-flush-disk", &[])
        );
        assert_eq!(
            launch_command(),
            words(&[
                "/usr/local/bin/marshal-launch",
                "/Users/admin/actions-runner"
            ])
        );
    }

    #[test]
    fn only_the_launcher_install_takes_standard_input() {
        assert_eq!(remove_runner_call().input, None);
        assert_eq!(install_runner_call(&runner()).input, None);
        assert_eq!(disable_remote_access_call().input, None);
        assert_eq!(disable_updates_call().input, None);
        assert_eq!(
            install_launcher_call().input,
            Some(LAUNCHER.as_bytes().to_vec())
        );
    }

    #[test]
    fn remote_access_is_turned_off_by_launchd_for_both_services_and_checked() {
        for service in ["com.openssh.sshd", "com.apple.screensharing"] {
            assert!(DISABLE_REMOTE_ACCESS.contains(service), "{service}");
        }
        for line in [
            "launchctl disable \"system/$service\"",
            "launchctl bootout \"system/$service\"",
            "launchctl print-disabled system",
            "launchctl print \"system/$service\"",
        ] {
            assert!(DISABLE_REMOTE_ACCESS.contains(line), "{line}");
        }
        assert!(!DISABLE_REMOTE_ACCESS.contains("systemsetup -setremotelogin"));
    }

    #[test]
    fn download_and_installation_are_off_and_the_automatic_check_is_left_alone() {
        for key in [
            "AutomaticDownload",
            "AutomaticallyInstallMacOSUpdates",
            "CriticalUpdateInstall",
            "ConfigDataInstall",
            "com.apple.commerce AutoUpdate",
        ] {
            assert!(DISABLE_UPDATES.contains(key), "{key}");
        }
        assert!(!DISABLE_UPDATES.contains("AutomaticCheckEnabled"));
        assert!(!DISABLE_UPDATES.contains("softwareupdate"));
    }

    #[test]
    fn every_script_stops_at_its_first_failure() {
        for (name, script) in SCRIPTS {
            assert!(
                script.starts_with("#!/bin/sh\n") || script.starts_with("#!/usr/bin/env bash\n"),
                "{name}"
            );
            assert!(
                script.contains("\nset -eu\n") || script.contains("\nset -euo pipefail\n"),
                "{name}"
            );
        }
    }

    #[test]
    fn only_the_launcher_names_the_jit_variable_and_it_starts_run_sh() {
        for (name, script) in SCRIPTS {
            assert_eq!(script.contains(JIT_VARIABLE), name == "launcher", "{name}");
        }
        assert!(LAUNCHER.contains(&format!("export {JIT_VARIABLE}\n")));
        assert!(LAUNCHER.ends_with("cd \"$1\"\nexec ./run.sh\n"));
        for forbidden in ["set -x", "tee ", "printf", "logger", "> ", "env "] {
            assert!(
                LAUNCHER
                    .lines()
                    .filter(|line| !line.starts_with('#'))
                    .all(|line| !line.replace(">&2", "").contains(forbidden)),
                "{forbidden}"
            );
        }
    }
}
