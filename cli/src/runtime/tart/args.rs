//! the exact argument lists of the `tart` calls; the flag names are those of Tart
//! 2.41.1, and `docs/operations/cli.md` is the place a version change is recorded.

use std::time::Duration;

/// the rules every VM boots with: DNS to the gateway passes, and `out` leaves
/// replies to the host's own connections alone.
pub const SOFTNET_ALLOW: &str = "out proto udp @host port 53,out proto tcp @host port 53";

/// the rules every VM boots with: everything else to the gateway, and multicast.
pub const SOFTNET_BLOCK: &str = "out @host,224.0.0.0/4";

const MEGABYTES_PER_GIGABYTE: u64 = 1024;

fn strings<const N: usize>(words: [&str; N]) -> Vec<String> {
    words.into_iter().map(str::to_string).collect()
}

/// `tart --version`.
#[must_use]
pub fn version_arguments() -> Vec<String> {
    strings(["--version"])
}

/// `tart list --format json`.
#[must_use]
pub fn list_arguments() -> Vec<String> {
    strings(["list", "--format", "json"])
}

/// `tart get <name> --format json`, whose `OS` field names the VM's operating
/// system.
#[must_use]
pub fn get_arguments(name: &str) -> Vec<String> {
    strings(["get", name, "--format", "json"])
}

/// `tart clone <source> <new name>`.
#[must_use]
pub fn clone_arguments(source: &str, new_name: &str) -> Vec<String> {
    strings(["clone", source, new_name])
}

/// `tart set <name> --cpu <n> --memory <MB> --disk-size <GB>`; Tart counts memory
/// in megabytes.
#[must_use]
pub fn set_arguments(name: &str, cpus: u32, memory_gb: u32, disk_gb: u32) -> Vec<String> {
    let memory_megabytes = u64::from(memory_gb) * MEGABYTES_PER_GIGABYTE;
    vec![
        "set".to_string(),
        name.to_string(),
        "--cpu".to_string(),
        cpus.to_string(),
        "--memory".to_string(),
        memory_megabytes.to_string(),
        "--disk-size".to_string(),
        disk_gb.to_string(),
    ]
}

/// `tart run` headless, without a clipboard, audio or USB accessories, on Softnet
/// with [`SOFTNET_ALLOW`] and [`SOFTNET_BLOCK`]; no shared directory, no Rosetta
/// and no other network option.
#[must_use]
pub fn run_arguments(name: &str) -> Vec<String> {
    strings([
        "run",
        "--no-graphics",
        "--no-clipboard",
        "--no-audio",
        "--no-usb-accessories",
        "--net-softnet",
        "--net-softnet-allow",
        SOFTNET_ALLOW,
        "--net-softnet-block",
        SOFTNET_BLOCK,
        name,
    ])
}

/// `tart exec [-i] <name> <command...>`; `-i` attaches this process's standard
/// input to the command, and `-t` is never passed.
#[must_use]
pub fn exec_arguments(name: &str, interactive: bool, command: &[String]) -> Vec<String> {
    let mut arguments = vec!["exec".to_string()];
    if interactive {
        arguments.push("-i".to_string());
    }
    arguments.push(name.to_string());
    arguments.extend(command.iter().cloned());
    arguments
}

/// `tart stop <name> --timeout <seconds>`: Tart interrupts the VM, waits that many
/// whole seconds, then kills it.
#[must_use]
pub fn stop_arguments(name: &str, grace: Duration) -> Vec<String> {
    vec![
        "stop".to_string(),
        name.to_string(),
        "--timeout".to_string(),
        grace.as_secs().to_string(),
    ]
}

/// `tart stop <name> --timeout 0`: Tart has no force flag, and a zero wait kills the
/// VM straight after the interrupt.
#[must_use]
pub fn force_stop_arguments(name: &str) -> Vec<String> {
    stop_arguments(name, Duration::ZERO)
}

/// `tart delete <name>`.
#[must_use]
pub fn delete_arguments(name: &str) -> Vec<String> {
    strings(["delete", name])
}

/// `tart pull <reference>`.
#[must_use]
pub fn pull_arguments(reference: &str) -> Vec<String> {
    strings(["pull", reference])
}

/// `tart rename <name> <new name>`.
#[must_use]
pub fn rename_arguments(name: &str, new_name: &str) -> Vec<String> {
    strings(["rename", name, new_name])
}

/// whether `reference` is `<repository>@sha256:<64 lowercase hex digits>`, where the
/// repository is lowercase letters, digits and `. _ - / :`, and does not start with
/// a hyphen.
#[must_use]
pub fn is_digest_reference(reference: &str) -> bool {
    let Some((repository, digest)) = reference.rsplit_once("@sha256:") else {
        return false;
    };
    let repository_ok = !repository.is_empty()
        && !repository.starts_with('-')
        && repository.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"._-/:".contains(&byte)
        });
    repository_ok
        && digest.len() == 64
        && digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
