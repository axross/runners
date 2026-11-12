//! the exporter: writes a diagnostic container's bundle into its pool's sink in a
//! child process, so a hung sink or client holds only the child; its file formats and
//! exit codes are stated in `docs/operations/cli.md`.

pub mod archive;
pub mod exporter;
pub mod identity;
pub mod sink;
pub mod transport;

use std::ffi::{OsStr, OsString};
use std::path::PathBuf;
use std::time::Duration;

use crate::runtime::backend::ExportOutcome;

/// the hidden subcommand that runs the exporter.
pub const EXPORT_COMMAND: &str = "export";

/// the exit code of a complete bundle, or of a count.
pub const EXIT_COMPLETE: u8 = 0;

/// the exit code of a failed, refused or unfinished export.
pub const EXIT_FAILED: u8 = 1;

/// the exit code of a complete bundle whose final observation is missing.
pub const EXIT_GAP: u8 = 3;

/// the exit code when the sink already holds the pool's ten bundles.
pub const EXIT_SINK_FULL: u8 = 4;

/// the line the exporter prints once it has created the bundle, which lets the
/// instance admit the pool's next export.
pub const ADMITTED: &str = "admitted";

/// the start of the line that gives the pool's bundles in the sink.
pub const BUNDLES: &str = "bundles ";

/// what one run of the exporter does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportRequest {
    /// the pool, whose name starts each of its bundles' names.
    pub pool: String,
    /// the container to export, or `None` to only count the pool's bundles.
    pub container: Option<String>,
    /// the job VM the container runs in, when its evidence is read through that
    /// VM's guest agent: the client is then `tart`, else `docker`. A count reads no
    /// evidence, so it serves with either.
    pub vm: Option<String>,
    /// the sink, as the configuration names it.
    pub directory: String,
    pub raw_records: bool,
    /// the checkout the sink must lie outside, when one was found.
    pub checkout: Option<PathBuf>,
    /// how long the exporter has, counted from its start.
    pub deadline: Duration,
}

impl ExportRequest {
    /// the arguments that run this request with `client` as the client, `docker` or,
    /// for a VM, `tart`, after the program itself.
    #[must_use]
    pub fn arguments(&self, client: &OsStr) -> Vec<OsString> {
        let mut arguments: Vec<OsString> = vec![
            EXPORT_COMMAND.into(),
            "--pool".into(),
            self.pool.clone().into(),
            "--directory".into(),
            self.directory.clone().into(),
        ];
        if let Some(container) = &self.container {
            arguments.extend(["--container".into(), container.clone().into()]);
        }
        if let Some(vm) = &self.vm {
            arguments.extend(["--vm".into(), vm.clone().into()]);
        }
        if self.raw_records {
            arguments.push("--raw-records".into());
        }
        if let Some(checkout) = &self.checkout {
            arguments.extend(["--checkout".into(), checkout.clone().into_os_string()]);
        }
        let millis = u64::try_from(self.deadline.as_millis()).unwrap_or(u64::MAX);
        arguments.extend([
            "--deadline-ms".into(),
            millis.to_string().into(),
            "--".into(),
            client.to_os_string(),
        ]);
        arguments
    }
}

/// what an exporter's exit code means; an exporter without one, ended by a signal,
/// failed.
#[must_use]
pub fn outcome_of_exit(code: Option<i32>) -> ExportOutcome {
    match code.and_then(|code| u8::try_from(code).ok()) {
        Some(EXIT_COMPLETE) => ExportOutcome::Complete,
        Some(EXIT_GAP) => ExportOutcome::Gap,
        Some(EXIT_SINK_FULL) => ExportOutcome::SinkFull,
        _ => ExportOutcome::Failed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_exit_codes_map_to_complete_gap_sink_full_and_failed() {
        assert_eq!(outcome_of_exit(Some(0)), ExportOutcome::Complete);
        assert_eq!(outcome_of_exit(Some(3)), ExportOutcome::Gap);
        assert_eq!(outcome_of_exit(Some(4)), ExportOutcome::SinkFull);
        for other in [Some(1), Some(5), Some(2), Some(-1), Some(256), None] {
            assert_eq!(outcome_of_exit(other), ExportOutcome::Failed, "{other:?}");
        }
    }

    #[test]
    fn a_request_names_every_setting_and_the_client_last() {
        let request = ExportRequest {
            pool: "d".to_string(),
            container: Some("d-1-x".to_string()),
            vm: None,
            directory: "C:\\diag".to_string(),
            raw_records: true,
            checkout: Some(PathBuf::from("/checkout")),
            deadline: Duration::from_millis(17_500),
        };
        let words: Vec<String> = request
            .arguments(OsStr::new("docker"))
            .iter()
            .map(|word| word.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            words,
            [
                "export",
                "--pool",
                "d",
                "--directory",
                "C:\\diag",
                "--container",
                "d-1-x",
                "--raw-records",
                "--checkout",
                "/checkout",
                "--deadline-ms",
                "17500",
                "--",
                "docker"
            ]
        );
    }

    #[test]
    fn a_vm_request_names_the_vm_before_the_deadline_and_the_tart_client_last() {
        let request = ExportRequest {
            pool: "d".to_string(),
            container: Some("d-1-x".to_string()),
            vm: Some("marshal-job-d-1-20260101000000000".to_string()),
            directory: "/diag".to_string(),
            raw_records: false,
            checkout: None,
            deadline: Duration::from_millis(1_000),
        };
        let words: Vec<String> = request
            .arguments(OsStr::new("tart"))
            .iter()
            .map(|word| word.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            words,
            [
                "export",
                "--pool",
                "d",
                "--directory",
                "/diag",
                "--container",
                "d-1-x",
                "--vm",
                "marshal-job-d-1-20260101000000000",
                "--deadline-ms",
                "1000",
                "--",
                "tart"
            ]
        );
    }
}
