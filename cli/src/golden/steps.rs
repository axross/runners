//! the guest calls that provision a golden VM, each as the `tart exec` command and
//! the standard input it takes. A script is passed as the argument of `bash -c`, not
//! on standard input, so a command inside it cannot read the rest of the script and
//! standard input stays free for the data a step installs.

use std::time::Duration;

use super::key::VerifiedKey;
use super::pins::{
    DOCKER_ARCHITECTURE, DOCKER_COMPONENT, DOCKER_GROUP_ID, DOCKER_KEY_PATH, DOCKER_KEY_URL,
    DOCKER_PACKAGES, DOCKER_REPOSITORY_URI, DOCKER_SOURCES_PATH, DOCKER_SUITE, GUEST_USER,
    LAUNCHER_PATH, PackagePin,
};

/// the launcher the golden VM runs `docker` through.
pub const LAUNCHER: &str = include_str!("scripts/launcher.sh");

const PREPARE: &str = include_str!("scripts/prepare.sh");
const FETCH_KEY: &str = include_str!("scripts/fetch-key.sh");
const LIST_KEY: &str = include_str!("scripts/list-key.sh");
const INSTALL_DOCKER: &str = include_str!("scripts/install-docker.sh");
const INSTALL_LAUNCHER: &str = include_str!("scripts/install-launcher.sh");
const DISABLE_SSH: &str = include_str!("scripts/disable-ssh.sh");

const PACKAGE_STEP_BUDGET: Duration = Duration::from_secs(20 * 60);
const DOWNLOAD_BUDGET: Duration = Duration::from_secs(2 * 60);
const QUICK_STEP_BUDGET: Duration = Duration::from_secs(60);
const FLUSH_BUDGET: Duration = Duration::from_secs(2 * 60);

/// one call through the guest agent: pass `command` to `tart exec`, with `-i` and
/// `input` on its standard input when there is some, and give it `budget`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuestCall {
    pub command: Vec<String>,
    pub input: Option<Vec<u8>>,
    pub budget: Duration,
}

pub(super) fn script_call(
    privileged: bool,
    name: &str,
    script: &str,
    arguments: &[&str],
    input: Option<Vec<u8>>,
    budget: Duration,
) -> GuestCall {
    let mut command: Vec<String> = Vec::new();
    if privileged {
        command.extend(["sudo", "-n"].map(str::to_string));
    }
    command.extend(["bash", "-c", script, name].map(str::to_string));
    command.extend(arguments.iter().map(ToString::to_string));
    GuestCall {
        command,
        input,
        budget,
    }
}

/// step 1: upgrades the Ubuntu packages and installs `ca-certificates`, `curl` and
/// `gnupg`, all from Ubuntu's archive.
#[must_use]
pub fn prepare_call() -> GuestCall {
    script_call(
        true,
        "marshal-prepare",
        PREPARE,
        &[],
        None,
        PACKAGE_STEP_BUDGET,
    )
}

/// step 2: downloads Docker's key to [`DOCKER_KEY_PATH`]. Nothing is installed from
/// Docker's repository yet.
#[must_use]
pub fn fetch_key_call() -> GuestCall {
    script_call(
        true,
        "marshal-fetch-key",
        FETCH_KEY,
        &[DOCKER_KEY_URL, DOCKER_KEY_PATH],
        None,
        DOWNLOAD_BUDGET,
    )
}

/// step 3: prints `gpg`'s listing of the downloaded key, which the caller gives to
/// [`check_key_listing`](super::key::check_key_listing) and fails the build on a
/// refusal.
#[must_use]
pub fn list_key_call() -> GuestCall {
    script_call(
        false,
        "marshal-list-key",
        LIST_KEY,
        &[DOCKER_KEY_PATH],
        None,
        QUICK_STEP_BUDGET,
    )
}

/// the apt source for Docker's repository, in the form Docker documents.
#[must_use]
pub fn docker_sources() -> String {
    format!(
        "Types: deb\nURIs: {DOCKER_REPOSITORY_URI}\nSuites: {DOCKER_SUITE}\nComponents: {DOCKER_COMPONENT}\nArchitectures: {DOCKER_ARCHITECTURE}\nSigned-By: {DOCKER_KEY_PATH}\n"
    )
}

/// step 4: installs the apt source, creates the `docker` group with
/// [`DOCKER_GROUP_ID`], installs the four packages at their pinned versions, holds
/// them, and adds the guest user to the group, which takes effect when the guest
/// agent next starts. It takes the proof that the key passed the
/// check, so nothing from Docker's repository is installed before that.
#[must_use]
pub fn install_docker_call(_key: &VerifiedKey) -> GuestCall {
    let packages: Vec<String> = DOCKER_PACKAGES
        .iter()
        .map(PackagePin::apt_specification)
        .collect();
    let docker_gid = DOCKER_GROUP_ID.to_string();
    let mut arguments = vec![DOCKER_SOURCES_PATH, GUEST_USER, docker_gid.as_str()];
    arguments.extend(packages.iter().map(String::as_str));
    script_call(
        true,
        "marshal-install-docker",
        INSTALL_DOCKER,
        &arguments,
        Some(docker_sources().into_bytes()),
        PACKAGE_STEP_BUDGET,
    )
}

/// step 5: installs the launcher at [`LAUNCHER_PATH`], owned by root, mode 0755.
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

/// step 6: locks the guest user's password and masks the SSH service and socket.
/// The guest agent does not use SSH, so it keeps working.
#[must_use]
pub fn disable_ssh_call() -> GuestCall {
    script_call(
        true,
        "marshal-disable-ssh",
        DISABLE_SSH,
        &[GUEST_USER],
        None,
        QUICK_STEP_BUDGET,
    )
}

/// writes the files the guest has not yet written to its disk, and fails when the
/// root filesystem reports a write error. `tart stop` ends the VM at once, with no
/// shutdown inside it, so the build VM runs this before it stops, or the files
/// written last would be missing from the golden VM.
#[must_use]
pub fn flush_disk_call() -> GuestCall {
    script_call(
        true,
        "marshal-flush-disk",
        "sync && sync -f /",
        &[],
        None,
        FLUSH_BUDGET,
    )
}

/// the command that runs `docker` with `arguments` as root in the build VM. The
/// guest agent's user joins the `docker` group in step 4, but the group applies only
/// when the agent next starts, which the build VM does not do, so the image build
/// runs through `sudo` instead of restarting the VM.
#[must_use]
pub fn build_vm_docker_command(arguments: &[String]) -> Vec<String> {
    let mut command: Vec<String> = ["sudo", "-n", "docker"].map(str::to_string).into();
    command.extend(arguments.iter().cloned());
    command
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::golden::key::check_key_listing;
    use crate::runtime::docker_args::JIT_VARIABLE;

    const KEY_LISTING: &str = include_str!("fixtures/docker-key-listing.txt");

    const SCRIPTS: [(&str, &str); 7] = [
        ("launcher", LAUNCHER),
        ("prepare", PREPARE),
        ("fetch-key", FETCH_KEY),
        ("list-key", LIST_KEY),
        ("install-docker", INSTALL_DOCKER),
        ("install-launcher", INSTALL_LAUNCHER),
        ("disable-ssh", DISABLE_SSH),
    ];

    fn words(list: &[&str]) -> Vec<String> {
        list.iter().map(ToString::to_string).collect()
    }

    fn verified() -> VerifiedKey {
        check_key_listing(KEY_LISTING).unwrap()
    }

    #[test]
    fn the_build_vm_runs_docker_through_sudo() {
        assert_eq!(
            build_vm_docker_command(&words(&["info"])),
            words(&["sudo", "-n", "docker", "info"])
        );
    }

    #[test]
    fn the_calls_are_exact() {
        assert_eq!(
            flush_disk_call().command,
            words(&[
                "sudo",
                "-n",
                "bash",
                "-c",
                "sync && sync -f /",
                "marshal-flush-disk"
            ])
        );
        assert_eq!(
            prepare_call().command,
            [
                words(&["sudo", "-n", "bash", "-c"]),
                vec![PREPARE.to_string()],
                words(&["marshal-prepare"]),
            ]
            .concat()
        );
        assert_eq!(
            fetch_key_call().command,
            [
                words(&["sudo", "-n", "bash", "-c"]),
                vec![FETCH_KEY.to_string()],
                words(&[
                    "marshal-fetch-key",
                    "https://download.docker.com/linux/ubuntu/gpg",
                    "/etc/apt/keyrings/docker.asc",
                ]),
            ]
            .concat()
        );
        assert_eq!(
            list_key_call().command,
            [
                words(&["bash", "-c"]),
                vec![LIST_KEY.to_string()],
                words(&["marshal-list-key", "/etc/apt/keyrings/docker.asc"]),
            ]
            .concat()
        );
        assert_eq!(
            install_docker_call(&verified()).command,
            [
                words(&["sudo", "-n", "bash", "-c"]),
                vec![INSTALL_DOCKER.to_string()],
                words(&[
                    "marshal-install-docker",
                    "/etc/apt/sources.list.d/docker.sources",
                    "admin",
                    "123",
                    "docker-ce=5:29.9.3-1~ubuntu.24.04~noble",
                    "docker-ce-cli=5:29.9.3-1~ubuntu.24.04~noble",
                    "containerd.io=2.4.2-3~ubuntu.24.04~noble",
                    "docker-buildx-plugin=0.38.1-1~ubuntu.24.04~noble",
                ]),
            ]
            .concat()
        );
        assert_eq!(
            install_launcher_call().command,
            [
                words(&["sudo", "-n", "bash", "-c"]),
                vec![INSTALL_LAUNCHER.to_string()],
                words(&["marshal-install-launcher", "/usr/local/bin/marshal-launch"]),
            ]
            .concat()
        );
        assert_eq!(
            disable_ssh_call().command,
            [
                words(&["sudo", "-n", "bash", "-c"]),
                vec![DISABLE_SSH.to_string()],
                words(&["marshal-disable-ssh", "admin"]),
            ]
            .concat()
        );
    }

    #[test]
    fn only_the_installing_steps_take_standard_input() {
        let key = verified();
        assert_eq!(prepare_call().input, None);
        assert_eq!(fetch_key_call().input, None);
        assert_eq!(list_key_call().input, None);
        assert_eq!(disable_ssh_call().input, None);
        assert_eq!(
            install_launcher_call().input,
            Some(LAUNCHER.as_bytes().to_vec())
        );
        assert_eq!(
            install_docker_call(&key).input,
            Some(docker_sources().into_bytes())
        );
    }

    #[test]
    fn the_apt_source_is_the_one_docker_documents() {
        assert_eq!(
            docker_sources(),
            "Types: deb\n\
             URIs: https://download.docker.com/linux/ubuntu\n\
             Suites: noble\n\
             Components: stable\n\
             Architectures: arm64\n\
             Signed-By: /etc/apt/keyrings/docker.asc\n"
        );
    }

    #[test]
    fn nothing_before_the_key_check_names_docker_s_repository() {
        for (name, script) in [
            ("prepare", PREPARE),
            ("fetch-key", FETCH_KEY),
            ("list-key", LIST_KEY),
        ] {
            assert!(!script.contains("docker"), "{name}");
        }
        let before_check = [prepare_call(), fetch_key_call(), list_key_call()];
        for call in before_check {
            assert!(
                call.command
                    .iter()
                    .all(|word| !word.contains(DOCKER_SOURCES_PATH))
            );
            assert!(
                call.command
                    .iter()
                    .filter(|word| word.contains(DOCKER_REPOSITORY_URI))
                    .all(|word| word == DOCKER_KEY_URL)
            );
        }
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
    fn only_the_launcher_names_the_jit_variable_and_it_never_prints_it() {
        for (name, script) in SCRIPTS {
            assert_eq!(script.contains(JIT_VARIABLE), name == "launcher", "{name}");
        }
        let uses: Vec<&str> = LAUNCHER
            .lines()
            .filter(|line| line.contains("jit") && !line.starts_with('#'))
            .map(str::trim)
            .collect();
        assert_eq!(
            uses,
            [
                "jit=$(head -c \"$((limit + 1))\")",
                "if [ -z \"$jit\" ]; then",
                "echo \"marshal-launch: standard input held no jit configuration\" >&2",
                "if [ \"${#jit}\" -gt \"$limit\" ]; then",
                "echo \"marshal-launch: the jit configuration on standard input is longer than $limit bytes\" >&2",
                "ACTIONS_RUNNER_INPUT_JITCONFIG=$jit",
            ]
        );
        assert!(LAUNCHER.contains(&format!("export {JIT_VARIABLE}\n")));
        assert!(LAUNCHER.ends_with("exec docker \"$@\"\n"));
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

    #[test]
    fn no_call_carries_the_jit_variable() {
        let key = verified();
        let calls = [
            prepare_call(),
            fetch_key_call(),
            list_key_call(),
            install_docker_call(&key),
            install_launcher_call(),
            disable_ssh_call(),
        ];
        for call in calls {
            for word in call.command.iter().skip(4) {
                assert!(!word.contains(JIT_VARIABLE), "{word}");
            }
        }
    }

    #[test]
    fn the_launcher_is_installed_where_the_pins_say() {
        assert_eq!(
            install_launcher_call().command.last().map(String::as_str),
            Some(LAUNCHER_PATH)
        );
    }
}
