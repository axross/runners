//! what the fake `tart`, `softnet` and `docker` share: recording each call into a
//! directory, reading its script files, and the scripted hang, failure and flood.

use std::fmt::Write as _;
use std::fs;
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub const JIT_VARIABLE: &str = "ACTIONS_RUNNER_INPUT_JITCONFIG";
const LONGEST_WAIT: Duration = Duration::from_secs(60);
const FLOOD_BYTES: usize = 200_000;

/// records a call as `inv-<n>.argv` (the arguments, NUL separated), `.pid`, `.pgid`
/// (on Unix), `.env` and `.alive`, and returns the call's path stem and the held
/// lock. `variables` are the environment variables whose values `.env` lists.
/// `.argv` appears last and whole, so a reader that finds it finds the rest.
pub fn record(
    dir: &Path,
    arguments: &[String],
    variables: &[&str],
) -> Result<(PathBuf, fs::File), String> {
    let (stem, alive) = claim_invocation(dir)?;
    fs::write(stem.with_extension("env"), environment_lines(variables))
        .map_err(|error| error.to_string())?;
    publish_arguments(&stem, arguments)?;
    Ok((stem, alive))
}

/// claims the next `inv-<n>` stem, takes the exclusive lock on its `.alive` file
/// and writes its `.pid` and `.pgid`; the caller writes the rest and then
/// [`publish_arguments`].
pub fn claim_invocation(dir: &Path) -> Result<(PathBuf, fs::File), String> {
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
        return Ok((stem, alive));
    }
    Err("no invocation number".to_string())
}

/// renames the arguments into place as `.argv`, so a call a test can name already
/// holds its lock and has written the rest; a reader never sees them half written.
pub fn publish_arguments(stem: &Path, arguments: &[String]) -> Result<(), String> {
    let pending = stem.with_extension("argv.tmp");
    fs::write(&pending, arguments.join("\0")).map_err(|error| error.to_string())?;
    fs::rename(&pending, stem.with_extension("argv")).map_err(|error| error.to_string())
}

fn environment_lines(variables: &[&str]) -> String {
    let mut lines = String::new();
    for name in variables {
        if let Some(value) = std::env::var_os(name) {
            let _ = writeln!(lines, "{name}={}", value.to_string_lossy());
        }
    }
    if std::env::var_os(JIT_VARIABLE).is_some() {
        lines.push_str("JIT=present\n");
    }
    let _ = writeln!(lines, "stdin_terminal={}", std::io::stdin().is_terminal());
    let _ = writeln!(lines, "stdout_terminal={}", std::io::stdout().is_terminal());
    let _ = writeln!(lines, "stderr_terminal={}", std::io::stderr().is_terminal());
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

pub fn exists(dir: &Path, flag: &str) -> bool {
    dir.join(flag).exists()
}

/// a script file's text without surrounding whitespace.
pub fn text(dir: &Path, file: &str) -> Option<String> {
    fs::read_to_string(dir.join(file))
        .ok()
        .map(|text| text.trim().to_string())
}

pub fn sleep_while(mut condition: impl FnMut() -> bool) {
    let started = Instant::now();
    while condition() && started.elapsed() < LONGEST_WAIT {
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// the scripted hang, failure or flood of a subcommand; `Some` ends the call.
pub fn scripted(dir: &Path, subcommand: &str) -> Option<u8> {
    if exists(dir, &format!("hang-{subcommand}")) {
        sleep_while(|| true);
        return Some(1);
    }
    if exists(dir, &format!("flood-{subcommand}")) {
        let chunk = vec![b'a'; FLOOD_BYTES];
        let _ = std::io::stdout().write_all(&chunk);
        let _ = std::io::stderr().write_all(&chunk);
    }
    let failure = format!("fail-{subcommand}");
    if exists(dir, &failure) {
        let message = text(dir, &failure)
            .filter(|message| !message.is_empty())
            .unwrap_or_else(|| "a scripted failure".to_string());
        let _ = writeln!(std::io::stderr(), "{message}");
        return Some(1);
    }
    None
}
