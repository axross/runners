//! one bounded run of a system program: its own process group, killed when its
//! future is dropped or its budget ends, standard input null, standard output piped
//! and refused past [`MAX_OUTPUT_BYTES`], and the JIT variable cleared.

use std::ffi::{OsStr, OsString};
use std::io;
use std::process::Stdio;
use std::time::Duration;

use tokio::io::AsyncReadExt;
use tokio::process::Command;

use crate::runtime::docker::client_command;
use crate::runtime::docker_args::JIT_VARIABLE;
pub use crate::runtime::tart::MAX_OUTPUT_BYTES;

/// the budget of one `route` or `arp` call.
pub const PROBE_BUDGET: Duration = Duration::from_secs(2);

/// why a call gave no answer.
#[derive(Debug)]
pub enum ProbeError {
    Unavailable(io::Error),
    TimedOut(Duration),
    TooLong,
}

/// what a finished call printed on standard output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Probe {
    pub code: Option<i32>,
    pub stdout: String,
}

/// the program a call runs: a release build always returns `release`, so a release
/// binary cannot be made to run another program, and a debug build runs `requested`
/// for the tests.
#[must_use]
pub fn resolve_program(requested: Option<&OsStr>, release: &str) -> OsString {
    #[cfg(debug_assertions)]
    if let Some(requested) = requested {
        return requested.to_os_string();
    }
    #[cfg(not(debug_assertions))]
    let _ = requested;
    OsString::from(release)
}

/// a call to a system program: the Docker client's command, with the JIT variable
/// removed last so nothing configured restores it.
pub(super) fn system_command(
    program: &OsStr,
    environment: &[(OsString, OsString)],
    arguments: &[String],
) -> Command {
    let mut command = client_command(program, environment, arguments);
    command.env_remove(JIT_VARIABLE);
    command
}

/// runs `program` with `arguments` to its end within `budget`.
pub async fn run_bounded(
    program: &OsStr,
    environment: &[(OsString, OsString)],
    arguments: &[String],
    budget: Duration,
) -> Result<Probe, ProbeError> {
    let mut command = system_command(program, environment, arguments);
    command.stdout(Stdio::piped()).stderr(Stdio::null());
    let mut child = command.spawn().map_err(ProbeError::Unavailable)?;
    let pipe = child.stdout.take();
    let call = async {
        let mut bytes = Vec::new();
        if let Some(pipe) = pipe {
            let limit = u64::try_from(MAX_OUTPUT_BYTES).unwrap_or(u64::MAX);
            pipe.take(limit.saturating_add(1))
                .read_to_end(&mut bytes)
                .await
                .map_err(ProbeError::Unavailable)?;
        }
        if bytes.len() > MAX_OUTPUT_BYTES {
            return Err(ProbeError::TooLong);
        }
        let status = child.wait().await.map_err(ProbeError::Unavailable)?;
        Ok(Probe {
            code: status.code(),
            stdout: String::from_utf8_lossy(&bytes).into_owned(),
        })
    };
    tokio::time::timeout(budget, call)
        .await
        .map_err(|_| ProbeError::TimedOut(budget))?
}
