//! a stand-in for the macOS programs the signals run, `caffeinate`, `route` and `arp`,
//! compiled because Windows finds only `.exe` programs. One binary plays all three,
//! told apart by their arguments: `-n get default` is `route`, `-n <address>` is
//! `arp`, and `-i -w <pid>` is `caffeinate`. `FAKE_MACOS_DIR` names a directory it
//! records into and reads its script from:
//!
//! - every call writes `inv-<n>.argv` (the arguments, NUL separated), `inv-<n>.pid`,
//!   `inv-<n>.pgid` (on Unix) and `inv-<n>.env`, which holds `JIT=present` when
//!   `ACTIONS_RUNNER_INPUT_JITCONFIG` is set (its value is never written) and
//!   `stdin_terminal=`, `stdout_terminal=` and `stderr_terminal=` with `true` or
//!   `false`. It holds an exclusive lock on `inv-<n>.alive` until it ends, taken before
//!   the arguments are written, so a test that takes that lock sees the call end,
//!   however it ends;
//! - `route` prints `route_stdout` and exits with `route_exit` (default 0); `arp`
//!   prints `arp_stdout` and exits with `arp_exit` (default 0);
//! - `caffeinate` holds until the file `caffeinate_stop` exists, then exits 0. With
//!   `caffeinate_exit` present it exits at once with the code in the file instead;
//! - any tool called `<tool>` fails with `fail-<tool>` present, printing the file's
//!   text, or a default one, to standard error and exiting 1; hangs with
//!   `hang-<tool>` present; and prints 200 000 bytes to each of standard output and
//!   standard error first with `flood-<tool>` present. Every wait is bounded, so a test
//!   that fails does not leave the helper running.

mod fake_support;

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use fake_support::{exists, record, scripted, sleep_while, text};

fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(code),
        Err(message) => {
            let _ = writeln!(std::io::stderr(), "fake-macos: {message}");
            ExitCode::from(70)
        }
    }
}

fn run() -> Result<u8, String> {
    let dir = PathBuf::from(std::env::var_os("FAKE_MACOS_DIR").ok_or("FAKE_MACOS_DIR is not set")?);
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let (_, _held_until_exit) = record(&dir, &arguments, &[])?;
    let words: Vec<&str> = arguments.iter().map(String::as_str).collect();
    let tool = match words.as_slice() {
        ["-n", "get", "default"] => "route",
        ["-n", _] => "arp",
        ["-i", "-w", _] => "caffeinate",
        _ => return Err(format!("unexpected call: {arguments:?}")),
    };
    if let Some(code) = scripted(&dir, tool) {
        return Ok(code);
    }
    match tool {
        "caffeinate" => caffeinate(&dir),
        _ => answer(&dir, tool),
    }
}

fn answer(dir: &Path, tool: &str) -> Result<u8, String> {
    let output = fs::read_to_string(dir.join(format!("{tool}_stdout"))).unwrap_or_default();
    std::io::stdout()
        .write_all(output.as_bytes())
        .map_err(|error| error.to_string())?;
    let code = text(dir, &format!("{tool}_exit")).map_or(Ok(0), |code| {
        code.parse::<u8>().map_err(|error| error.to_string())
    })?;
    Ok(code)
}

fn caffeinate(dir: &Path) -> Result<u8, String> {
    if let Some(code) = text(dir, "caffeinate_exit") {
        return code.parse::<u8>().map_err(|error| error.to_string());
    }
    sleep_while(|| !exists(dir, "caffeinate_stop"));
    Ok(0)
}
