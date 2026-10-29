//! the shim between the CLI and one job container's `docker run` client, on Unix; it
//! runs until the client ends or its standard input, a liveness pipe whose write end
//! only the CLI holds, closes; a diagnostic container is ended and kept, so its
//! evidence survives for the next finalisation; only the client gets the JIT
//! configuration, through the environment the shim inherits, never the removal's
//! client; see `docs/operations/cli.md`.

use std::ffi::{OsStr, OsString};
use std::os::fd::AsFd;
use std::os::unix::process::ExitStatusExt;
use std::process::{ExitCode, ExitStatus, Stdio};
use std::rc::Rc;
use std::time::Duration;

use tokio::process::Command;
use tokio::sync::oneshot;

use crate::runtime::backend::Backend;
use crate::runtime::clock::SystemClock;
use crate::runtime::docker::{DockerBackend, DockerConfig};
use crate::runtime::log::StderrLog;

/// the hidden subcommand that runs the shim.
pub const SHIM_COMMAND: &str = "shim";

/// the flag that runs the shim in diagnostic mode.
pub const DIAGNOSTIC_FLAG: &str = "--diagnostic";

const REMOVE_BUDGET: Duration = Duration::from_secs(5);

const CANNOT_RUN: u8 = 125;

/// the exit code after the pipe closed and the container was removed, or ended.
const CLI_GONE: u8 = 0;

/// the exit code after the pipe closed and the removal or the kill failed or ran
/// out of time.
const REMOVAL_FAILED: u8 = 1;

/// runs the shim for the container `name`, whose client is `program` with
/// `arguments`; with `diagnostic`, in diagnostic mode.
#[must_use]
pub fn run(name: &str, diagnostic: bool, program: &OsStr, arguments: &[OsString]) -> ExitCode {
    let runtime = match crate::start::single_threaded_runtime() {
        Ok(runtime) => runtime,
        Err(error) => {
            println!("shim: the runtime cannot start: {error}");
            return ExitCode::from(CANNOT_RUN);
        }
    };
    runtime.block_on(supervise(name, diagnostic, program, arguments))
}

/// waits on a thread of its own for standard input to end, the liveness pipe closing.
fn input_closed() -> oneshot::Receiver<()> {
    let (closed, receiver) = oneshot::channel();
    std::thread::spawn(move || {
        let _ = std::io::copy(&mut std::io::stdin(), &mut std::io::sink());
        let _ = closed.send(());
    });
    receiver
}

/// the client's exit code, or 128 plus the signal that ended it.
fn exit_code(status: &ExitStatus) -> u8 {
    let code = status
        .code()
        .or_else(|| status.signal().map(|signal| 128 + signal))
        .unwrap_or(i32::from(CANNOT_RUN));
    u8::try_from(code).unwrap_or(CANNOT_RUN)
}

async fn supervise(
    name: &str,
    diagnostic: bool,
    program: &OsStr,
    arguments: &[OsString],
) -> ExitCode {
    let joined = match std::io::stdout().as_fd().try_clone_to_owned() {
        Ok(stdout) => stdout,
        Err(error) => {
            println!("shim: standard output cannot be shared with the client: {error}");
            return ExitCode::from(CANNOT_RUN);
        }
    };
    let mut client = match Command::new(program)
        .args(arguments)
        .stdin(Stdio::null())
        .stderr(Stdio::from(joined))
        .process_group(0)
        .kill_on_drop(true)
        .spawn()
    {
        Ok(client) => client,
        Err(error) => {
            println!("shim: the docker client could not start: {error}");
            return ExitCode::from(CANNOT_RUN);
        }
    };
    let mut closed = input_closed();
    // a closed pipe outranks the client's end, even one that came at the same time,
    // so the container is removed whenever the CLI is gone
    let ended = tokio::select! {
        biased;
        _ = &mut closed => None,
        status = client.wait() => Some(status),
    };
    match ended {
        Some(status) if closed.try_recv().is_err() => match status {
            Ok(status) => ExitCode::from(exit_code(&status)),
            Err(error) => {
                println!("shim: the docker client could not be waited for: {error}");
                ExitCode::from(CANNOT_RUN)
            }
        },
        _ => {
            let cleaned = clean_up(name, diagnostic, program).await;
            let _ = client.kill().await;
            ExitCode::from(if cleaned { CLI_GONE } else { REMOVAL_FAILED })
        }
    }
}

/// force-removes the container, or ends a diagnostic one and keeps it, within
/// [`REMOVE_BUDGET`].
async fn clean_up(name: &str, diagnostic: bool, program: &OsStr) -> bool {
    let config = DockerConfig {
        program: program.to_os_string(),
        ..DockerConfig::default()
    };
    let backend = DockerBackend::new(config, SystemClock, Rc::new(StderrLog::default()));
    let names = [name.to_string()];
    let cleaned = if diagnostic {
        backend.kill_containers(&names, REMOVE_BUDGET).await
    } else {
        backend.remove_containers(&names, REMOVE_BUDGET).await
    };
    cleaned.is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_removal_budget_is_five_seconds() {
        assert_eq!(REMOVE_BUDGET, Duration::from_secs(5));
    }

    #[test]
    fn a_client_ended_by_a_signal_exits_with_128_plus_the_signal() {
        assert_eq!(exit_code(&ExitStatus::from_raw(9)), 137);
        assert_eq!(exit_code(&ExitStatus::from_raw(3 << 8)), 3);
    }
}
