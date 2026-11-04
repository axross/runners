//! the `tart` client: every call is a child process in its own process group that
//! is killed when its future is dropped, with its standard output piped and its
//! output cut off at [`MAX_OUTPUT_BYTES`]; every call but `run` is bounded by a time.

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::io;
use std::process::{ExitStatus, Stdio};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, Command};
use tokio::task::JoinHandle;

use crate::runtime::docker::client_command;
use crate::runtime::docker_args::JIT_VARIABLE;
use crate::runtime::scrub::scrub_error_text;

use super::args::{
    clone_arguments, delete_arguments, exec_arguments, force_stop_arguments, is_digest_reference,
    list_arguments, pull_arguments, rename_arguments, run_arguments, set_arguments, stop_arguments,
};
use super::list::{VmEntry, parse_vm_list};

/// the most bytes of each of a call's standard output and standard error that are
/// kept; the rest is read and dropped.
pub const MAX_OUTPUT_BYTES: usize = 64 * 1024;

/// the variable that stops Tart from deleting cached images to make room for a pull
/// or a clone.
pub const NO_AUTO_PRUNE_VARIABLE: &str = "TART_NO_AUTO_PRUNE";

/// the client a release build always runs, found by the operating system's search.
pub const TART_PROGRAM: &str = "tart";

/// the variable that names the client in a debug build; a release build ignores it.
pub const TEST_TART_VARIABLE: &str = "MARSHAL_TEST_TART";
const READ_CHUNK_BYTES: usize = 8 * 1024;

/// resolves the client to run: a release build always returns [`TART_PROGRAM`], so a
/// release binary cannot be made to run another program, and a debug build runs
/// `requested` for the tests.
#[must_use]
pub fn resolve_program(requested: Option<&OsStr>) -> OsString {
    #[cfg(debug_assertions)]
    if let Some(requested) = requested {
        return requested.to_os_string();
    }
    #[cfg(not(debug_assertions))]
    let _ = requested;
    OsString::from(TART_PROGRAM)
}

/// the client this process runs: `MARSHAL_TEST_TART` in a debug build, else
/// [`TART_PROGRAM`].
#[must_use]
pub fn program_from_environment() -> OsString {
    resolve_program(std::env::var_os(TEST_TART_VARIABLE).as_deref())
}

/// the time bounds on the calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TartLimits {
    /// the bound on a listing, a clone, a resize or a rename; a stop, a deletion
    /// and an `exec` take the budget their caller passes.
    pub command_budget: Duration,
    /// the bound on a pull of the base image, which is far larger than any other
    /// transfer.
    pub pull_budget: Duration,
}

impl Default for TartLimits {
    fn default() -> Self {
        Self {
            command_budget: Duration::from_secs(120),
            pull_budget: Duration::from_secs(30 * 60),
        }
    }
}

/// how to reach the `tart` client.
#[derive(Debug, Clone)]
pub struct TartConfig {
    pub program: OsString,
    /// variables to add to every call's environment, none of which can be the JIT
    /// variable, `TART_NO_AUTO_PRUNE` or `PATH`.
    pub environment: Vec<(OsString, OsString)>,
    /// the `PATH` every call gets, which is also the one Tart finds `softnet` on;
    /// this process's own, unless a test names another.
    pub path: Option<OsString>,
    pub limits: TartLimits,
}

impl Default for TartConfig {
    fn default() -> Self {
        Self {
            program: OsString::from(TART_PROGRAM),
            environment: Vec::new(),
            path: std::env::var_os("PATH"),
            limits: TartLimits::default(),
        }
    }
}

/// why a call did not give an answer.
#[derive(Debug)]
pub enum TartError {
    /// the call was not made: an operand Tart would read as an option, or a pull
    /// that is not by digest.
    Refused(String),
    /// the client could not start.
    Unavailable(io::Error),
    /// the client did not finish within its budget and was ended.
    TimedOut(Duration),
    /// the client ended with a failure; the text is its output on one line, cut.
    Failed { what: String, text: String },
    /// the client answered something the reader refuses.
    Unreadable(String),
}

impl fmt::Display for TartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Refused(reason) | Self::Unreadable(reason) => f.write_str(reason),
            Self::Unavailable(error) => write!(f, "the tart client could not start: {error}"),
            Self::TimedOut(budget) => write!(
                f,
                "the tart client did not finish within {} s",
                budget.as_secs()
            ),
            Self::Failed { what, text } => write!(f, "{what}: {text}"),
        }
    }
}

impl std::error::Error for TartError {}

/// what one call printed, each stream cut at [`MAX_OUTPUT_BYTES`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Captured {
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub stdout_cut: bool,
    pub stderr_cut: bool,
}

impl Captured {
    #[must_use]
    pub fn succeeded(&self) -> bool {
        self.code == Some(0)
    }

    /// the client's output for an error message, on one line and cut as a
    /// registration error is.
    #[must_use]
    pub fn error_text(&self) -> String {
        scrub_error_text(&format!("{} {}", self.stderr, self.stdout), "")
    }

    /// this answer when the client succeeded, else a [`TartError::Failed`] that
    /// begins with `what`.
    pub fn checked(self, what: &str) -> Result<Self, TartError> {
        if self.succeeded() {
            return Ok(self);
        }
        Err(TartError::Failed {
            what: what.to_string(),
            text: self.error_text(),
        })
    }
}

#[derive(Debug, Default, Clone)]
struct Cut {
    bytes: Vec<u8>,
    truncated: bool,
}

type Sink = Arc<Mutex<Cut>>;

/// reads to the end, keeping the first [`MAX_OUTPUT_BYTES`] and dropping the rest.
async fn drain<R: AsyncRead + Unpin>(mut reader: R, sink: Sink) {
    let mut chunk = [0_u8; READ_CHUNK_BYTES];
    loop {
        let read = match reader.read(&mut chunk).await {
            Ok(0) | Err(_) => return,
            Ok(read) => read,
        };
        let mut cut = sink.lock().unwrap_or_else(PoisonError::into_inner);
        let room = MAX_OUTPUT_BYTES.saturating_sub(cut.bytes.len());
        let kept = read.min(room);
        if let Some(part) = chunk.get(..kept) {
            cut.bytes.extend_from_slice(part);
        }
        cut.truncated |= kept < read;
    }
}

fn snapshot(sink: &Sink) -> Cut {
    sink.lock().unwrap_or_else(PoisonError::into_inner).clone()
}

fn captured(code: Option<i32>, stdout: &Sink, stderr: &Sink) -> Captured {
    let (stdout, stderr) = (snapshot(stdout), snapshot(stderr));
    Captured {
        code,
        stdout: String::from_utf8_lossy(&stdout.bytes).into_owned(),
        stderr: String::from_utf8_lossy(&stderr.bytes).into_owned(),
        stdout_cut: stdout.truncated,
        stderr_cut: stderr.truncated,
    }
}

fn operand(value: &str) -> Result<&str, TartError> {
    if value.is_empty() || value.starts_with('-') {
        return Err(TartError::Refused(format!(
            "{value:?} cannot be passed to tart as a name"
        )));
    }
    Ok(value)
}

/// a `tart run` that is still going: its output so far is readable at any time.
pub struct RunningVm {
    child: Child,
    stdout: Sink,
    stderr: Sink,
    readers: Vec<JoinHandle<()>>,
}

impl RunningVm {
    #[must_use]
    pub fn id(&self) -> Option<u32> {
        self.child.id()
    }

    /// waits for the client to end.
    pub async fn wait(&mut self) -> io::Result<ExitStatus> {
        self.child.wait().await
    }

    /// the client's status if it has ended.
    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        self.child.try_wait()
    }

    /// kills the client and waits for it.
    pub async fn kill(&mut self) -> io::Result<()> {
        self.child.kill().await
    }

    /// what the client has printed so far, with `code` as its exit code.
    #[must_use]
    pub fn output(&self, code: Option<i32>) -> Captured {
        captured(code, &self.stdout, &self.stderr)
    }
}

impl Drop for RunningVm {
    fn drop(&mut self) {
        for reader in &self.readers {
            reader.abort();
        }
    }
}

/// the `tart` client.
#[derive(Debug, Clone)]
pub struct TartClient {
    config: TartConfig,
}

impl TartClient {
    #[must_use]
    pub fn new(config: TartConfig) -> Self {
        Self { config }
    }

    #[must_use]
    pub fn limits(&self) -> TartLimits {
        self.config.limits
    }

    /// a call's command: the Docker client's, with `TART_NO_AUTO_PRUNE` set, this
    /// process's `PATH`, and the JIT variable removed last so nothing configured
    /// restores it.
    fn command(&self, arguments: &[String]) -> Command {
        let mut command = client_command(&self.config.program, &[], arguments);
        for (name, value) in &self.config.environment {
            command.env(name, value);
        }
        command.env(NO_AUTO_PRUNE_VARIABLE, "1");
        match &self.config.path {
            Some(path) => command.env("PATH", path),
            None => command.env_remove("PATH"),
        };
        command.env_remove(JIT_VARIABLE);
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
        command
    }

    /// runs `tart` with `arguments` to its end within `budget`, writing `input` to
    /// its standard input when there is some, and returns what it printed.
    pub async fn capture(
        &self,
        arguments: &[String],
        input: Option<&[u8]>,
        budget: Duration,
    ) -> Result<Captured, TartError> {
        let mut command = self.command(arguments);
        command.stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        });
        let mut child = command.spawn().map_err(TartError::Unavailable)?;
        let (stdout, stderr) = (Sink::default(), Sink::default());
        let (stdin_pipe, stdout_pipe, stderr_pipe) =
            (child.stdin.take(), child.stdout.take(), child.stderr.take());
        let call = async {
            let write = async {
                if let (Some(mut pipe), Some(input)) = (stdin_pipe, input) {
                    // a client that ends without reading all of it shows in its status
                    let _ = pipe.write_all(input).await;
                }
            };
            let read_stdout = async {
                if let Some(pipe) = stdout_pipe {
                    drain(pipe, stdout.clone()).await;
                }
            };
            let read_stderr = async {
                if let Some(pipe) = stderr_pipe {
                    drain(pipe, stderr.clone()).await;
                }
            };
            tokio::join!(write, read_stdout, read_stderr);
            child.wait().await
        };
        let status = tokio::time::timeout(budget, call)
            .await
            .map_err(|_| TartError::TimedOut(budget))?
            .map_err(TartError::Unavailable)?;
        Ok(captured(status.code(), &stdout, &stderr))
    }

    /// the VMs Tart holds, local ones and cached images. A listing cut at
    /// [`MAX_OUTPUT_BYTES`] is refused.
    pub async fn list(&self) -> Result<Vec<VmEntry>, TartError> {
        let answer = self
            .capture(&list_arguments(), None, self.config.limits.command_budget)
            .await?
            .checked("tart list failed")?;
        if answer.stdout_cut {
            return Err(TartError::Unreadable(
                "the tart listing is longer than the bound".to_string(),
            ));
        }
        parse_vm_list(answer.stdout.as_bytes())
            .map_err(|error| TartError::Unreadable(error.to_string()))
    }

    /// clones the VM or cached image `source` to the new local VM `new_name`.
    pub async fn clone_vm(&self, source: &str, new_name: &str) -> Result<(), TartError> {
        let arguments = clone_arguments(operand(source)?, operand(new_name)?);
        self.simple(&arguments, "tart clone failed").await
    }

    /// sets a local VM's vCPU count, memory and disk size; a disk can grow, not
    /// shrink.
    pub async fn set_resources(
        &self,
        name: &str,
        cpus: u32,
        memory_gb: u32,
        disk_gb: u32,
    ) -> Result<(), TartError> {
        let arguments = set_arguments(operand(name)?, cpus, memory_gb, disk_gb);
        self.simple(&arguments, "tart set failed").await
    }

    /// renames a local VM.
    pub async fn rename(&self, name: &str, new_name: &str) -> Result<(), TartError> {
        let arguments = rename_arguments(operand(name)?, operand(new_name)?);
        self.simple(&arguments, "tart rename failed").await
    }

    /// deletes a VM within `budget`.
    pub async fn delete(&self, name: &str, budget: Duration) -> Result<(), TartError> {
        self.capture(&delete_arguments(operand(name)?), None, budget)
            .await?
            .checked("tart delete failed")
            .map(drop)
    }

    /// pulls the image `reference` names, which must be by digest.
    pub async fn pull(&self, reference: &str) -> Result<(), TartError> {
        if !is_digest_reference(reference) {
            return Err(TartError::Refused(format!(
                "{reference:?} is not an image reference by digest"
            )));
        }
        self.capture(
            &pull_arguments(reference),
            None,
            self.config.limits.pull_budget,
        )
        .await?
        .checked("tart pull failed")
        .map(drop)
    }

    /// stops a running VM: Tart interrupts it, waits `grace`, then kills it. The
    /// call ends after `budget`, which should exceed `grace`.
    pub async fn stop(
        &self,
        name: &str,
        grace: Duration,
        budget: Duration,
    ) -> Result<(), TartError> {
        let arguments = stop_arguments(operand(name)?, grace);
        self.capture(&arguments, None, budget)
            .await?
            .checked("tart stop failed")
            .map(drop)
    }

    /// stops a running VM without a wait.
    pub async fn force_stop(&self, name: &str, budget: Duration) -> Result<(), TartError> {
        self.capture(&force_stop_arguments(operand(name)?), None, budget)
            .await?
            .checked("tart stop failed")
            .map(drop)
    }

    /// runs `command` in a running VM through the guest agent and returns what it
    /// printed whatever its exit code. With `input` it passes `-i` and writes the
    /// input to the command's standard input; without, it passes neither `-i` nor
    /// `-t`.
    pub async fn exec(
        &self,
        name: &str,
        command: &[String],
        input: Option<&[u8]>,
        budget: Duration,
    ) -> Result<Captured, TartError> {
        let arguments = exec_arguments(operand(name)?, input.is_some(), command);
        self.capture(&arguments, input, budget).await
    }

    /// starts `tart run` for the local VM `name`, headless and on Softnet; the call
    /// lasts as long as the VM, so it has no budget, and the client is killed when
    /// the returned value is dropped.
    pub fn spawn_run(&self, name: &str) -> Result<RunningVm, TartError> {
        let mut command = self.command(&run_arguments(operand(name)?));
        command.stdin(Stdio::null());
        let mut child = command.spawn().map_err(TartError::Unavailable)?;
        let (stdout, stderr) = (Sink::default(), Sink::default());
        let mut readers = Vec::new();
        if let Some(stream) = child.stdout.take() {
            readers.push(tokio::spawn(drain(stream, stdout.clone())));
        }
        if let Some(stream) = child.stderr.take() {
            readers.push(tokio::spawn(drain(stream, stderr.clone())));
        }
        Ok(RunningVm {
            child,
            stdout,
            stderr,
            readers,
        })
    }

    async fn simple(&self, arguments: &[String], failure: &str) -> Result<(), TartError> {
        self.capture(arguments, None, self.config.limits.command_budget)
            .await?
            .checked(failure)
            .map(drop)
    }
}
