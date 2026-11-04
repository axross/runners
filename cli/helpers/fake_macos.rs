//! a stand-in for the macOS programs the signals run, `caffeinate`, `route` and `arp`,
//! compiled because Windows finds only `.exe` programs. One binary plays all three,
//! told apart by their arguments: `-n get default` is `route`, `-n <address>` is
//! `arp`, and `-i -w <pid>` is `caffeinate`. `FAKE_MACOS_DIR` names a directory it
//! records into and reads its script from:
//!
//! - every call writes `inv-<n>.argv` (the arguments, NUL separated), `inv-<n>.pid`,
//!   `inv-<n>.pgid` (on Unix) and `inv-<n>.env`, which holds `JIT=present` when
//!   `ACTIONS_RUNNER_INPUT_JITCONFIG` is set (its value is never written) and
//!   `stdin_terminal=` and `stdout_terminal=` with `true` or `false`. It holds an
//!   exclusive lock on `inv-<n>.alive` until it ends, taken before the arguments are
//!   written, so a test that takes that lock sees the call end, however it ends;
//! - `route` prints `route_stdout` and exits with `route_exit` (default 0); `arp`
//!   prints `arp_stdout` and exits with `arp_exit` (default 0);
//! - `caffeinate` holds until the file `caffeinate_stop` exists, then exits 0. With
//!   `caffeinate_exit` present it exits at once with the code in the file instead;
//! - any tool called `<tool>` fails with `fail-<tool>` present, printing the file's
//!   text, or a default one, to standard error and exiting 1; hangs with
//!   `hang-<tool>` present; and prints 200 000 bytes to standard output first with
//!   `flood-<tool>` present. Every wait is bounded, so a test that fails does not
//!   leave the helper running.

use std::fmt::Write as _;
use std::fs;
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

const JIT_VARIABLE: &str = "ACTIONS_RUNNER_INPUT_JITCONFIG";
const LONGEST_WAIT: Duration = Duration::from_secs(60);
const FLOOD_BYTES: usize = 200_000;

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
    let _held_until_exit = record(&dir, &arguments)?;
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

fn record(dir: &Path, arguments: &[String]) -> Result<fs::File, String> {
    for number in 0..100_000 {
        let stem = dir.join(format!("inv-{number:06}"));
        let claimed = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(stem.with_extension("claim"));
        if claimed.is_err() {
            continue;
        }
        let alive =
            fs::File::create(stem.with_extension("alive")).map_err(|error| error.to_string())?;
        alive.lock().map_err(|error| error.to_string())?;
        fs::write(stem.with_extension("pid"), std::process::id().to_string())
            .map_err(|error| error.to_string())?;
        record_process_group(&stem);
        fs::write(stem.with_extension("env"), environment_lines())
            .map_err(|error| error.to_string())?;
        publish_arguments(&stem, arguments)?;
        return Ok(alive);
    }
    Err("no invocation number".to_string())
}

/// renames the arguments into place as `.argv`, so a call a test can name already
/// holds its lock and has written the rest; a reader never sees them half written.
fn publish_arguments(stem: &Path, arguments: &[String]) -> Result<(), String> {
    let pending = stem.with_extension("argv.tmp");
    fs::write(&pending, arguments.join("\0")).map_err(|error| error.to_string())?;
    fs::rename(&pending, stem.with_extension("argv")).map_err(|error| error.to_string())
}

fn environment_lines() -> String {
    let mut lines = String::new();
    if std::env::var_os(JIT_VARIABLE).is_some() {
        lines.push_str("JIT=present\n");
    }
    let _ = writeln!(lines, "stdin_terminal={}", std::io::stdin().is_terminal());
    let _ = writeln!(lines, "stdout_terminal={}", std::io::stdout().is_terminal());
    lines
}

/// records the process group this call runs in, where `ps` can tell it.
#[cfg(unix)]
fn record_process_group(stem: &Path) {
    let output = std::process::Command::new("ps")
        .args(["-o", "pgid=", "-p", &std::process::id().to_string()])
        .output();
    if let Ok(output) = output {
        let _ = fs::write(stem.with_extension("pgid"), output.stdout);
    }
}

#[cfg(not(unix))]
fn record_process_group(_stem: &Path) {}

fn exists(dir: &Path, flag: &str) -> bool {
    dir.join(flag).exists()
}

fn text(dir: &Path, file: &str) -> Option<String> {
    fs::read_to_string(dir.join(file)).ok()
}

fn sleep_while(mut condition: impl FnMut() -> bool) {
    let started = Instant::now();
    while condition() && started.elapsed() < LONGEST_WAIT {
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// the scripted hang, failure or flood of a tool; `Some` ends the call.
fn scripted(dir: &Path, tool: &str) -> Option<u8> {
    if exists(dir, &format!("hang-{tool}")) {
        sleep_while(|| true);
        return Some(1);
    }
    if exists(dir, &format!("flood-{tool}")) {
        let _ = std::io::stdout().write_all(&vec![b'a'; FLOOD_BYTES]);
    }
    let failure = format!("fail-{tool}");
    if exists(dir, &failure) {
        let message = text(dir, &failure)
            .map(|message| message.trim().to_string())
            .filter(|message| !message.is_empty())
            .unwrap_or_else(|| "a scripted failure".to_string());
        let _ = writeln!(std::io::stderr(), "{message}");
        return Some(1);
    }
    None
}

fn answer(dir: &Path, tool: &str) -> Result<u8, String> {
    let output = text(dir, &format!("{tool}_stdout")).unwrap_or_default();
    std::io::stdout()
        .write_all(output.as_bytes())
        .map_err(|error| error.to_string())?;
    let code = text(dir, &format!("{tool}_exit")).map_or(Ok(0), |code| {
        code.trim().parse::<u8>().map_err(|error| error.to_string())
    })?;
    Ok(code)
}

fn caffeinate(dir: &Path) -> Result<u8, String> {
    if let Some(code) = text(dir, "caffeinate_exit") {
        return code.trim().parse::<u8>().map_err(|error| error.to_string());
    }
    sleep_while(|| !exists(dir, "caffeinate_stop"));
    Ok(0)
}
