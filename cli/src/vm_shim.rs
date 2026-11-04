//! the shim between the CLI and one VM's `tart run`, on Unix; it holds the `tart run`
//! process until it ends or its standard input, a liveness pipe whose write end only
//! the CLI holds, closes, and then stops and deletes the VM; it holds no lock the CLI
//! holds and it is made for any VM the CLI boots, a job VM or a build VM; see
//! `docs/operations/cli.md`.

use std::ffi::OsStr;
use std::io::Write;
use std::process::ExitCode;
use std::time::Duration;

use crate::runtime::tart::{RunningVm, TartClient, TartConfig};
use crate::shim::{CANNOT_RUN, exit_code, input_closed};

/// the hidden subcommand that runs the VM shim.
pub const VM_SHIM_COMMAND: &str = "vm-shim";

/// the grace the graceful stop gives a VM that is asked to shut down.
pub const STOP_GRACE: Duration = Duration::from_secs(5);

/// the bound on the graceful stop call, which exceeds [`STOP_GRACE`] so Tart can
/// end its own wait.
pub const STOP_BUDGET: Duration = Duration::from_secs(8);

/// the bound on the forced stop that follows a graceful stop that failed.
pub const FORCE_BUDGET: Duration = Duration::from_secs(3);

/// the bound on the deletion.
pub const DELETE_BUDGET: Duration = Duration::from_secs(4);

/// the exit code after the pipe closed and the VM was stopped and deleted.
const CLI_GONE: u8 = 0;

/// the exit code after the pipe closed and the deletion failed or ran out of time.
const DELETION_FAILED: u8 = 1;

/// the longest the shim takes to stop and delete its VM once the pipe has closed,
/// the three calls' bounds together.
#[must_use]
pub fn cleanup_budget() -> Duration {
    STOP_BUDGET + FORCE_BUDGET + DELETE_BUDGET
}

/// runs the shim for the VM `vm`, whose `tart run` is started with the `tart`
/// client `program`; the `PATH` is this process's, as the CLI gave it.
#[must_use]
pub fn run(vm: &str, program: &OsStr) -> ExitCode {
    let runtime = match crate::start::single_threaded_runtime() {
        Ok(runtime) => runtime,
        Err(error) => {
            println!("shim: the runtime cannot start: {error}");
            return ExitCode::from(CANNOT_RUN);
        }
    };
    let client = TartClient::new(TartConfig {
        program: program.to_os_string(),
        ..TartConfig::default()
    });
    runtime.block_on(supervise(&client, vm))
}

async fn supervise(client: &TartClient, vm: &str) -> ExitCode {
    let mut running = match client.spawn_run(vm) {
        Ok(running) => running,
        Err(error) => {
            println!("shim: tart run could not start: {error}");
            return ExitCode::from(CANNOT_RUN);
        }
    };
    let mut closed = input_closed();
    // a closed pipe outranks the end of `tart run`, even one that came at the same
    // time, so the VM is deleted whenever the CLI is gone
    let ended = tokio::select! {
        biased;
        _ = &mut closed => None,
        status = running.wait() => Some(status),
    };
    match ended {
        Some(status) if closed.try_recv().is_err() => match status {
            Ok(status) => {
                let code = exit_code(&status);
                relay_output(&running, status.code());
                ExitCode::from(code)
            }
            Err(error) => {
                println!("shim: tart run could not be waited for: {error}");
                ExitCode::from(CANNOT_RUN)
            }
        },
        _ => {
            let deleted = clean_up(client, vm).await;
            let _ = running.kill().await;
            ExitCode::from(if deleted { CLI_GONE } else { DELETION_FAILED })
        }
    }
}

/// writes what `tart run` printed, standard error first, to the shim's standard
/// output, where the CLI reads it as Tart's text; each stream is already cut at the
/// client's bound.
fn relay_output(running: &RunningVm, code: Option<i32>) {
    let output = running.output(code);
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{}", output.stderr);
    let _ = writeln!(out, "{}", output.stdout);
    let _ = out.flush();
}

/// stops the VM gracefully, then forcibly when that failed, and deletes it, each call
/// within its own bound; true when the deletion succeeded.
async fn clean_up(client: &TartClient, vm: &str) -> bool {
    if client.stop(vm, STOP_GRACE, STOP_BUDGET).await.is_err() {
        let _ = client.force_stop(vm, FORCE_BUDGET).await;
    }
    client.delete(vm, DELETE_BUDGET).await.is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cleanup_takes_fifteen_seconds_at_the_most() {
        assert_eq!(cleanup_budget(), Duration::from_secs(15));
        assert!(STOP_BUDGET > STOP_GRACE);
    }
}
