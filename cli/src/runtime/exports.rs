//! the exporter child as a backend runs it: this CLI run again as `marshal export`,
//! with a liveness pipe as its standard input, closing which ends it, so it can kill
//! its own clients first; both backends share it, and only the client and the
//! environment the command gets differ; `docs/operations/cli.md` describes it.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::path::PathBuf;
use std::process::Stdio;
use std::rc::Rc;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::process::Command;
use tokio::time::Instant;

use crate::diagnostics::{ADMITTED, BUNDLES, ExportRequest, outcome_of_exit};

use super::backend::{DiagnosticRecord, ExportOutcome, Exported};
use super::docker_args::JIT_VARIABLE;

/// how long an exporter gets to end after its liveness pipe closes at the export
/// deadline, before it is killed.
pub const EXPORTER_TERMINATION: Duration = Duration::from_secs(1);

const MAX_EXPORTER_LINE_BYTES: u64 = 64;

/// the admission lock of one pool and sink, held by an export until its child has
/// created the bundle.
pub(crate) type Admission = Rc<tokio::sync::Mutex<()>>;

/// a backend's exporter children and the admission locks of the sinks they write.
pub(crate) struct Exports {
    /// the program run as the exporter child: this CLI; without one, every export
    /// fails, and its container is still removed.
    program: Option<PathBuf>,
    /// the checkout every sink must lie outside, when one was found.
    checkout: Option<PathBuf>,
    admissions: RefCell<BTreeMap<(String, String), Admission>>,
}

impl Exports {
    pub fn new(program: Option<PathBuf>, checkout: Option<PathBuf>) -> Self {
        Self {
            program,
            checkout,
            admissions: RefCell::new(BTreeMap::new()),
        }
    }

    /// the admission lock of `record`'s pool and sink.
    pub fn admission(&self, record: &DiagnosticRecord) -> Admission {
        self.admissions
            .borrow_mut()
            .entry((record.pool.clone(), record.directory.clone()))
            .or_default()
            .clone()
    }

    /// the exporter request for `record`'s sink; no `container` only counts its
    /// bundles, and a `vm` reads the container's evidence through that VM's guest
    /// agent.
    pub fn request(
        &self,
        record: &DiagnosticRecord,
        container: Option<String>,
        vm: Option<String>,
        deadline: Instant,
    ) -> ExportRequest {
        ExportRequest {
            pool: record.pool.clone(),
            container,
            vm,
            directory: record.directory.clone(),
            raw_records: record.raw_records,
            checkout: self.checkout.clone(),
            deadline: deadline.saturating_duration_since(Instant::now()),
        }
    }

    /// the command that runs the exporter child for `request` with `client`, without
    /// the JIT variable and with a liveness pipe as its standard input; its standard
    /// error is this process's, and it writes only a panic's location there. `None`
    /// when the backend has no program to run.
    pub fn command(&self, request: &ExportRequest, client: &OsStr) -> Option<Command> {
        let mut command = Command::new(self.program.as_ref()?);
        command
            .args(request.arguments(client))
            .env_remove(JIT_VARIABLE)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(false);
        #[cfg(unix)]
        command.process_group(0);
        #[cfg(windows)]
        command.creation_flags(super::docker::CREATE_NEW_PROCESS_GROUP);
        Some(command)
    }

    /// runs the exporter child `command` starts until it ends or `deadline` passes;
    /// `admission` is released once the child has created the bundle or ended.
    pub async fn run(
        &self,
        command: Option<Command>,
        deadline: Instant,
        mut admission: Option<tokio::sync::MutexGuard<'_, ()>>,
    ) -> Exported {
        let Some(mut child) = command.and_then(|mut command| command.spawn().ok()) else {
            return Exported::FAILED;
        };
        let liveness = child.stdin.take();
        let stdout = child.stdout.take();
        let (admitted, bundles) = (Cell::new(false), Cell::new(None));
        let reading = async {
            if let Some(stdout) = stdout {
                let mut reader = BufReader::new(stdout);
                loop {
                    let mut line = Vec::new();
                    let read = (&mut reader)
                        .take(MAX_EXPORTER_LINE_BYTES)
                        .read_until(b'\n', &mut line)
                        .await
                        .unwrap_or(0);
                    if read == 0 {
                        break;
                    }
                    let text = String::from_utf8_lossy(&line);
                    let text = text.trim_end();
                    if text == ADMITTED {
                        admitted.set(true);
                        admission.take();
                    } else if let Some(count) = text.strip_prefix(BUNDLES) {
                        bundles.set(count.parse().ok());
                    }
                }
            }
            admission.take();
            child.wait().await.ok()
        };
        let outcome = match tokio::time::timeout_at(deadline, reading).await {
            Ok(Some(status)) => outcome_of_exit(status.code()),
            Ok(None) => ExportOutcome::Failed,
            Err(_) => {
                drop(liveness);
                if tokio::time::timeout(EXPORTER_TERMINATION, child.wait())
                    .await
                    .is_err()
                {
                    let _ = child.kill().await;
                }
                ExportOutcome::Failed
            }
        };
        Exported {
            outcome,
            admitted: admitted.get(),
            bundles: bundles.get(),
        }
    }
}
