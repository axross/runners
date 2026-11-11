//! a stand-in for `softnet`, compiled because Windows finds only `.exe` programs, for
//! the tests that run the version read as a process. Tart finds Softnet on `PATH`, so
//! a test copies or links this program to a file called `softnet` in a directory on
//! its `PATH`; the fake reads its script from the directory it was started from, as
//! `argv[0]` names it, and `FAKE_SOFTNET_DIR` names another when it is set:
//!
//! - every call writes `inv-<n>.argv`, `.pid`, `.pgid` (on Unix), `.env` and `.alive`
//!   as the fake `tart` does, with `PATH` among the variables `.env` lists;
//! - `--version`, the only call it answers, prints the file `version_output` exactly
//!   as it holds it, else `softnet 0.25.0-0f1e2d3` and a newline, the form Softnet
//!   0.25.0 prints: its name, the version, a hyphen and the short commit;
//! - the script names `version` for a hang (`hang-version`), a failure
//!   (`fail-version`, printing the file's text) and a flood (`flood-version`, 200 000
//!   bytes to each of standard output and standard error). Every wait is bounded, so
//!   a test that fails does not leave the helper running;
//! - any other call fails with exit code 2, as Softnet's argument parser does.

mod fake_support;

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use fake_support::{record, scripted};

const DEFAULT_VERSION: &str = "softnet 0.25.0-0f1e2d3\n";

fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(code),
        Err(message) => {
            let _ = writeln!(std::io::stderr(), "fake-softnet: {message}");
            ExitCode::from(70)
        }
    }
}

fn script_directory() -> Result<PathBuf, String> {
    if let Some(directory) = std::env::var_os("FAKE_SOFTNET_DIR") {
        return Ok(PathBuf::from(directory));
    }
    let invoked = PathBuf::from(std::env::args_os().next().ok_or("no argv[0]")?);
    invoked
        .parent()
        .map(PathBuf::from)
        .ok_or_else(|| "argv[0] has no directory".to_string())
}

fn run() -> Result<u8, String> {
    let dir = script_directory()?;
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let (_stem, _held_until_exit) = record(&dir, &arguments, &["PATH"])?;
    if arguments != ["--version"] {
        let _ = writeln!(
            std::io::stderr(),
            "error: unexpected arguments {arguments:?}"
        );
        return Ok(2);
    }
    if let Some(code) = scripted(&dir, "version") {
        return Ok(code);
    }
    let output = std::fs::read_to_string(dir.join("version_output"))
        .unwrap_or_else(|_| DEFAULT_VERSION.to_string());
    let _ = std::io::stdout().write_all(output.as_bytes());
    Ok(0)
}
