//! The exporter: what writes a diagnostic container's bundle into its pool's sink.
//!
//! Each export runs in a child process, this CLI run again under a hidden
//! subcommand, so a hung sink or client can only hold the child, which the
//! instance ends at the export deadline. The child checks the sink, admits the
//! bundle within the pool's quota, then writes `identity.txt`, the files of the
//! collector's archive, and `complete.txt` last, in the PowerShell exporter's
//! formats. It prints only its admission and the pool's bundle count, and exits
//! with the PowerShell exporter's codes.

pub mod archive;
pub mod exporter;
pub mod identity;
pub mod sink;

use std::ffi::{OsStr, OsString};
use std::path::PathBuf;
use std::time::Duration;

use crate::runtime::backend::ExportOutcome;

/// The hidden subcommand that runs the exporter.
pub const EXPORT_COMMAND: &str = "export";

/// The exit code of a complete bundle, or of a count.
pub const EXIT_COMPLETE: u8 = 0;

/// The exit code of a failed, refused or unfinished export.
pub const EXIT_FAILED: u8 = 1;

/// The exit code of a complete bundle whose final observation is missing.
pub const EXIT_GAP: u8 = 3;

/// The exit code when the sink already holds the pool's ten bundles.
pub const EXIT_SINK_FULL: u8 = 4;

/// The line the exporter prints once it has created the bundle, which lets the
/// instance admit the pool's next export.
pub const ADMITTED: &str = "admitted";

/// The start of the line that gives the pool's bundles in the sink.
pub const BUNDLES: &str = "bundles ";

/// What one run of the exporter does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportRequest {
    /// The pool, whose name starts each of its bundles' names.
    pub pool: String,
    /// The container to export, or `None` to only count the pool's bundles.
    pub container: Option<String>,
    /// The sink, as the configuration names it.
    pub directory: String,
    pub raw_records: bool,
    /// The checkout the sink must lie outside, when one was found.
    pub checkout: Option<PathBuf>,
    /// How long the exporter has, counted from its start.
    pub deadline: Duration,
}

impl ExportRequest {
    /// The arguments that run this request with `docker` as the client, after the
    /// program itself.
    #[must_use]
    pub fn arguments(&self, docker: &OsStr) -> Vec<OsString> {
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
            docker.to_os_string(),
        ]);
        arguments
    }
}

/// What an exporter's exit code means; an exporter without one, ended by a signal,
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
}
