//! The exact `docker` argument lists of the run contract.
//!
//! These reproduce `docker-commands.ps1` and the supervisor's helper functions
//! argument for argument. The tests compare them with the PowerShell source.

use super::backend::{ContainerSpec, Mount};
use crate::validate::PlannedVolume;

/// The command a job container runs after the launcher verifies its CPUs.
pub const RUN_COMMAND: &str = "/home/runner/run.sh";

/// The variable that carries the JIT configuration to the runner. The client is
/// told the name only, and reads the value from its own environment.
pub const JIT_VARIABLE: &str = "ACTIONS_RUNNER_INPUT_JITCONFIG";

/// The label that marks a container as diagnostics evidence, which cleanup never
/// removes.
pub const DIAGNOSTIC_LABEL: &str = "runners.diagnostic-lifecycle=1";

/// The label that marks a CPU probe container.
pub const PROBE_LABEL: &str = "runners.cpu-probe=1";

/// What the probe container runs to print the CPUs it may use.
pub const PROBE_READER: &str = "while read -r key value; do if [ \"$key\" = Cpus_allowed_list: ]; then printf \"%s\\n\" \"$value\"; exit; fi; done < /proc/self/status; exit 1";

/// The launcher script's checks, before `exec "$@"`: the container's own CPU list
/// must equal the one selected, or the runner does not start.
#[must_use]
pub fn guard_script() -> String {
    [
        "set -eu",
        "expected=$1",
        "shift",
        "allowed=",
        "if [ ! -r /proc/self/status ]; then",
        "  printf 'CPU affinity readback unavailable.\\n' >&2",
        "  exit 125",
        "fi",
        "while read -r key value; do",
        "  if [ \"$key\" = Cpus_allowed_list: ]; then allowed=$value; break; fi",
        "done < /proc/self/status",
        "if [ \"$allowed\" != \"$expected\" ]; then",
        "  printf 'CPU affinity readback mismatch; runner startup refused.\\n' >&2",
        "  exit 125",
        "fi",
        "printf 'CPU affinity verified: %s\\n' \"$allowed\"",
    ]
    .join("\n")
}

fn launcher() -> String {
    format!("{}\nexec \"$@\"", guard_script())
}

fn mount_arguments(mounts: &[Mount]) -> Vec<String> {
    mounts
        .iter()
        .flat_map(|mount| {
            [
                "--mount".to_string(),
                format!(
                    "type=volume,source={},target={}",
                    mount.volume, mount.target
                ),
            ]
        })
        .collect()
}

/// The arguments of the one `docker run` that starts a job container. The limits
/// and the name come before the image, because Docker reads what follows the image
/// as the container's own command.
#[must_use]
pub fn job_run_arguments(spec: &ContainerSpec) -> Vec<String> {
    let memory = format!("{}g", spec.memory_gb);
    let mut arguments: Vec<String> = [
        "run",
        "--rm",
        "--pull",
        "never",
        "--name",
        &spec.name,
        "--cpus",
        &spec.cpus,
        "--cpuset-cpus",
        &spec.cpuset,
        "--memory",
        &memory,
        "--memory-swap",
        &memory,
    ]
    .iter()
    .map(|argument| (*argument).to_string())
    .collect();
    arguments.extend(mount_arguments(&spec.mounts));
    arguments.extend(
        [
            "-e",
            JIT_VARIABLE,
            &spec.image,
            "bash",
            "-c",
            &launcher(),
            "--",
            &spec.cpuset,
            &spec.run_command,
        ]
        .iter()
        .map(|argument| (*argument).to_string()),
    );
    arguments
}

fn strings(arguments: &[&str]) -> Vec<String> {
    arguments
        .iter()
        .map(|argument| (*argument).to_string())
        .collect()
}

#[must_use]
pub fn info_arguments() -> Vec<String> {
    strings(&["info"])
}

/// Lists a pool's containers by name prefix.
#[must_use]
pub fn pool_list_arguments(pool: &str) -> Vec<String> {
    strings(&[
        "ps",
        "-a",
        "--filter",
        &format!("name={pool}-"),
        "--format",
        "{{.Names}}",
    ])
}

/// Lists those of a pool's containers that carry the diagnostics label.
#[must_use]
pub fn pool_diagnostic_list_arguments(pool: &str) -> Vec<String> {
    strings(&[
        "ps",
        "-a",
        "--filter",
        &format!("name={pool}-"),
        "--filter",
        &format!("label={DIAGNOSTIC_LABEL}"),
        "--format",
        "{{.Names}}",
    ])
}

#[must_use]
pub fn remove_arguments(names: &[String]) -> Vec<String> {
    let mut arguments = strings(&["rm", "-f"]);
    arguments.extend(names.iter().cloned());
    arguments
}

#[must_use]
pub fn stop_arguments(grace_seconds: u64, names: &[String]) -> Vec<String> {
    let mut arguments = strings(&["stop", "--time", &grace_seconds.to_string()]);
    arguments.extend(names.iter().cloned());
    arguments
}

#[must_use]
pub fn volume_create_arguments(volume: &PlannedVolume) -> Vec<String> {
    strings(&["volume", "create", &volume.name])
}

/// The short root container that gives the runner user the volumes' mount points.
#[must_use]
pub fn volume_chown_arguments(image: &str, volumes: &[PlannedVolume]) -> Vec<String> {
    let mut arguments = strings(&["run", "--rm", "--pull", "never", "--user", "root"]);
    for volume in volumes {
        arguments.push("--mount".to_string());
        arguments.push(format!(
            "type=volume,source={},target={}",
            volume.name, volume.mount_path
        ));
    }
    arguments.extend(strings(&[image, "chown", "runner:docker"]));
    arguments.extend(volumes.iter().map(|volume| volume.mount_path.clone()));
    arguments
}

/// Finds a probe container by its exact name and its label.
#[must_use]
pub fn probe_find_arguments(probe: &str) -> Vec<String> {
    strings(&[
        "ps",
        "-a",
        "--filter",
        &format!("name=^{probe}$"),
        "--filter",
        &format!("label={PROBE_LABEL}"),
        "--format",
        "{{.Names}}",
    ])
}

/// Runs the probe: no network, no pull, no registration, no mounts, no limits.
#[must_use]
pub fn probe_run_arguments(probe: &str, image: &str) -> Vec<String> {
    strings(&[
        "run",
        "--rm",
        "--pull",
        "never",
        "--network",
        "none",
        "--name",
        probe,
        "--label",
        PROBE_LABEL,
        image,
        "bash",
        "-c",
        PROBE_READER,
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(mounts: Vec<Mount>) -> ContainerSpec {
        ContainerSpec {
            name: "example-repo-one-2-20240305060708009".to_string(),
            cpus: "1.5".to_string(),
            cpuset: "5,11".to_string(),
            memory_gb: 16,
            mounts,
            image: "actions-runner:local".to_string(),
            run_command: RUN_COMMAND.to_string(),
        }
    }

    #[test]
    fn the_job_container_arguments_are_the_run_contract() {
        let mounts = vec![Mount {
            volume: "example-repo-one-npm".to_string(),
            target: "/home/runner/.npm".to_string(),
        }];
        let arguments = job_run_arguments(&spec(mounts));
        let launcher = launcher();
        let expected: Vec<&str> = vec![
            "run",
            "--rm",
            "--pull",
            "never",
            "--name",
            "example-repo-one-2-20240305060708009",
            "--cpus",
            "1.5",
            "--cpuset-cpus",
            "5,11",
            "--memory",
            "16g",
            "--memory-swap",
            "16g",
            "--mount",
            "type=volume,source=example-repo-one-npm,target=/home/runner/.npm",
            "-e",
            "ACTIONS_RUNNER_INPUT_JITCONFIG",
            "actions-runner:local",
            "bash",
            "-c",
            &launcher,
            "--",
            "5,11",
            "/home/runner/run.sh",
        ];
        assert_eq!(arguments, expected);
    }

    #[test]
    fn a_container_without_mounts_has_no_blank_argument() {
        let arguments = job_run_arguments(&spec(Vec::new()));
        assert!(arguments.iter().all(|argument| !argument.is_empty()));
        let len = arguments.len();
        assert_eq!(
            arguments.get(len - 7).map(String::as_str),
            Some("actions-runner:local")
        );
        assert_eq!(arguments.get(len - 2).map(String::as_str), Some("5,11"));
        assert_eq!(arguments.last().map(String::as_str), Some(RUN_COMMAND));
    }

    /// The text between `@'` and `'@` of the PowerShell here-string that starts with
    /// `set -eu`, and the probe's reader script, from the supervisor's own source.
    fn powershell_source() -> String {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../hosts/windows-docker-desktop/docker-commands.ps1");
        std::fs::read_to_string(path).unwrap().replace("\r\n", "\n")
    }

    #[test]
    fn the_guard_script_is_the_powershell_guard_byte_for_byte() {
        let source = powershell_source();
        let start = source.find("@'\nset -eu\n").unwrap() + "@'\n".len();
        let end = start + source[start..].find("\n'@").unwrap();
        assert_eq!(guard_script(), &source[start..end]);
    }

    #[test]
    fn the_probe_reader_is_the_powershell_reader() {
        let source = powershell_source();
        let marker = "$reader = '";
        let start = source.find(marker).unwrap() + marker.len();
        let end = start + source[start..].find('\'').unwrap();
        assert_eq!(PROBE_READER, &source[start..end]);
    }

    #[test]
    fn the_launcher_ends_with_the_exec_of_the_command() {
        assert!(launcher().ends_with("verified: %s\\n' \"$allowed\"\nexec \"$@\""));
        assert!(!launcher().contains('\r'));
    }
}
