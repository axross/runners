//! the exact `docker` argument lists of the run contract, the PowerShell supervisor's
//! own from `hosts/windows-docker-desktop/docker-commands.ps1` and its helper
//! functions, except the stop call (see `docs/operations/cli.md`); the tests hold only
//! the launcher guard and the probe's reader script equal to that file.

use super::backend::{ContainerSpec, Mount};
use crate::image::check::labels_format;
use crate::validate::PlannedVolume;

/// the command a job container runs after the launcher verifies its CPUs.
pub const RUN_COMMAND: &str = "/home/runner/run.sh";

/// the variable that carries the JIT configuration to the runner; the client is
/// told the name only, and reads the value from its own environment.
pub const JIT_VARIABLE: &str = "ACTIONS_RUNNER_INPUT_JITCONFIG";

/// the label that marks a container as diagnostics evidence, which cleanup never
/// removes.
pub const DIAGNOSTIC_LABEL: &str = "runners.diagnostic-lifecycle=1";

/// the label that marks a CPU probe container.
pub const PROBE_LABEL: &str = "runners.cpu-probe=1";

/// what the probe container runs to print the CPUs it may use.
pub const PROBE_READER: &str = "while read -r key value; do if [ \"$key\" = Cpus_allowed_list: ]; then printf \"%s\\n\" \"$value\"; exit; fi; done < /proc/self/status; exit 1";

/// the launcher's checks before `exec "$@"`: the container's CPU list must equal the
/// selected one.
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

/// the diagnostic variant's launcher: the guard, then the collector, or the runner
/// with a gap when it is missing.
#[must_use]
pub fn diagnostic_launcher(raw_records: bool) -> String {
    let mode = if raw_records { " raw-run" } else { "" };
    format!(
        "{}\nif [ -x /usr/local/bin/runner-diagnostics ]; then exec /usr/local/bin/runner-diagnostics{mode}; \
         else printf \"Diagnostic gap: collector missing.\\n\" >&2; exec /home/runner/run.sh; fi",
        guard_script()
    )
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

/// the arguments of the one `docker run` that starts a job container, the
/// diagnostic variant for a diagnostics-enabled pool's; the options come before the
/// image, because Docker reads what follows it as the container's own command.
#[must_use]
pub fn job_run_arguments(spec: &ContainerSpec) -> Vec<String> {
    let (removal, label, launcher): (&[&str], &[&str], String) = match &spec.diagnostic {
        Some(record) => (
            &[],
            &["--label", DIAGNOSTIC_LABEL],
            diagnostic_launcher(record.raw_records),
        ),
        None => (&["--rm"], &[], launcher()),
    };
    let memory = format!("{}g", spec.memory_gb);
    let mut arguments = strings(&["run"]);
    arguments.extend(strings(removal));
    arguments.extend(strings(&["--pull", "never", "--name", &spec.name]));
    arguments.extend(strings(label));
    arguments.extend(strings(&[
        "--cpus",
        &spec.cpus,
        "--cpuset-cpus",
        &spec.cpuset,
        "--memory",
        &memory,
        "--memory-swap",
        &memory,
    ]));
    arguments.extend(mount_arguments(&spec.mounts));
    arguments.extend(strings(&[
        "-e",
        JIT_VARIABLE,
        &spec.image,
        "bash",
        "-c",
        &launcher,
        "--",
        &spec.cpuset,
        &spec.run_command,
    ]));
    arguments
}

/// where the collector writes its records, the source of the copy.
pub const DIAGNOSTIC_SOURCE: &str = "/tmp/runner-diagnostics/.";

/// the fixed allowlist of fields `identity.txt` holds, and nothing else of the
/// container's configuration.
pub const IDENTITY_FORMAT: &str = "{{.Image}}|{{.HostConfig.NanoCpus}}|{{.HostConfig.Memory}}|{{.HostConfig.MemorySwap}}|{{.HostConfig.CpuQuota}}|{{.HostConfig.CpuPeriod}}|{{.HostConfig.CpusetCpus}}|{{.State.OOMKilled}}|{{.State.ExitCode}}";

/// reads a container's identity through [`IDENTITY_FORMAT`].
#[must_use]
pub fn identity_arguments(name: &str) -> Vec<String> {
    strings(&["inspect", "--format", IDENTITY_FORMAT, name])
}

/// copies the collector's directory out of a container, as an archive on standard
/// output.
#[must_use]
pub fn copy_arguments(name: &str) -> Vec<String> {
    strings(&["cp", &format!("{name}:{DIAGNOSTIC_SOURCE}"), "-"])
}

/// ends containers at once, without removing them.
#[must_use]
pub fn kill_arguments(names: &[String]) -> Vec<String> {
    let mut arguments = strings(&["kill"]);
    arguments.extend(names.iter().cloned());
    arguments
}

pub(crate) fn strings(arguments: &[&str]) -> Vec<String> {
    arguments
        .iter()
        .map(|argument| (*argument).to_string())
        .collect()
}

#[must_use]
pub fn info_arguments() -> Vec<String> {
    strings(&["info"])
}

/// lists the containers whose names contain the pool's name and a hyphen.
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

/// lists those of a pool's containers that carry the diagnostics label.
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

/// the short root container that gives the runner user the volumes' mount points.
#[must_use]
pub fn volume_chown_arguments(image: &str, volumes: &[PlannedVolume]) -> Vec<String> {
    let mut arguments = strings(&["run", "--rm", "--pull", "never", "--user", "root"]);
    let mounts: Vec<Mount> = volumes.iter().map(Mount::from).collect();
    arguments.extend(mount_arguments(&mounts));
    arguments.extend(strings(&[image, "chown", "runner:docker"]));
    arguments.extend(volumes.iter().map(|volume| volume.mount_path.clone()));
    arguments
}

/// finds a probe container by its exact name and its label.
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

/// runs the probe: no network, no pull, no registration, no mounts, no limits.
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

/// reads the labels the image check needs, of `reference` as `docker run` resolves
/// it.
#[must_use]
pub fn image_inspect_arguments(reference: &str) -> Vec<String> {
    strings(&["image", "inspect", "--format", &labels_format(), reference])
}

/// lists the ID of the image `reference` names, and nothing when there is none.
#[must_use]
pub fn image_list_arguments(reference: &str) -> Vec<String> {
    strings(&["image", "ls", "--quiet", reference])
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
            diagnostic: None,
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

    /// the text of docker-commands.ps1, which holds the launcher guard and the probe's
    /// reader script.
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
    fn a_diagnostics_enabled_pools_container_runs_the_diagnostic_variant() {
        use crate::runtime::backend::DiagnosticRecord;
        for raw_records in [false, true] {
            let mut diagnostic = spec(Vec::new());
            diagnostic.diagnostic = Some(DiagnosticRecord {
                pool: "example-repo-one".to_string(),
                directory: "C:\\diag".to_string(),
                raw_records,
            });
            let arguments = job_run_arguments(&diagnostic);
            assert!(!arguments.iter().any(|argument| argument == "--rm"));
            assert!(
                arguments
                    .iter()
                    .any(|argument| argument == DIAGNOSTIC_LABEL)
            );
            assert_eq!(
                arguments
                    .iter()
                    .any(|argument| argument.contains("runner-diagnostics raw-run;")),
                raw_records
            );
        }
    }

    /// `diagnostic-export.ps1`, where the PowerShell exporter reads the identity and
    /// copies the collector's directory.
    fn powershell_export_source() -> String {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../hosts/windows-docker-desktop/diagnostic-export.ps1");
        std::fs::read_to_string(path).unwrap().replace("\r\n", "\n")
    }

    #[test]
    fn the_identity_and_the_copy_are_the_powershell_exporters() {
        let source = powershell_export_source();
        assert!(
            source.contains(&format!("$format = '{IDENTITY_FORMAT}'")),
            "the identity allowlist differs from diagnostic-export.ps1"
        );
        assert!(source.contains("@('inspect', '--format', $format, $Name)"));
        assert!(source.contains(&format!("@('cp', \"${{Name}}:{DIAGNOSTIC_SOURCE}\", '-')")));
        assert_eq!(
            identity_arguments("d-1-x"),
            ["inspect", "--format", IDENTITY_FORMAT, "d-1-x"]
        );
        assert_eq!(
            copy_arguments("d-1-x"),
            ["cp", "d-1-x:/tmp/runner-diagnostics/.", "-"]
        );
    }

    #[test]
    fn the_launcher_ends_with_the_exec_of_the_command() {
        assert!(launcher().ends_with("verified: %s\\n' \"$allowed\"\nexec \"$@\""));
        assert!(!launcher().contains('\r'));
    }
}
