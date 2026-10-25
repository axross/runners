//! A stand-in for the `docker` command line client, for the tests that run the real
//! container backend.
//!
//! It is a compiled helper, not a script, because Windows finds only `.exe` programs.
//! `FAKE_DOCKER_DIR` names a directory it records into and reads its script from:
//!
//! - every call writes `inv-<n>.argv` (the arguments, NUL separated) and
//!   `inv-<n>.pid`, `inv-<n>.pgid` (on Unix), and `inv-<n>.jit` when `ACTIONS_RUNNER_INPUT_JITCONFIG` is set;
//! - `containers` holds the fake engine's containers, one `name<TAB>label,label` per
//!   line, which `ps`, `rm`, `stop` and `run` read and change;
//! - the files `info_failures` (a count, decremented per call), `ps_fails`,
//!   `ps_label_fails`, `rm_fails`, `stop_fails`, `stop_hangs`, `volume_fails`,
//!   `probe_mode` (`fail`, `hang`, `flood`, `garbage`), `probe_cpus`, `run_mode`
//!   (`hold`, `longline`), `run_prints_jit` and `run_exit` change what the call
//!   does; a `stop_fails` that holds `longline` also prints a long line to
//!   standard error.
//!
//! A `stop_hangs` stop ignores the grace period: it waits without stopping
//! anything, as a container that ignores its stop signal makes `docker stop` wait.
//! A `run_prints_jit` job container prints its JIT configuration to standard
//! output and standard error, as a careless runner could.
//!
//! A `hold` job container stays until its entry leaves `containers`, then exits
//! 137, so `rm -f` and `stop` end it as they would end a real one. Every wait is
//! bounded, so a test that fails does not leave the helper running. A `longline` job
//! container prints one line of [`LONG_LINE_BYTES`] bytes, the letters `a` to `z` in
//! turn, then exits as an ordinary job does.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

const JIT_VARIABLE: &str = "ACTIONS_RUNNER_INPUT_JITCONFIG";
const PROBE_LABEL: &str = "runners.cpu-probe=1";
const LONGEST_WAIT: Duration = Duration::from_secs(60);
const LONG_LINE_BYTES: usize = 150_000;

fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(code),
        Err(message) => {
            let _ = writeln!(std::io::stderr(), "fake-docker: {message}");
            ExitCode::from(70)
        }
    }
}

fn run() -> Result<u8, String> {
    let dir =
        PathBuf::from(std::env::var_os("FAKE_DOCKER_DIR").ok_or("FAKE_DOCKER_DIR is not set")?);
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    record(&dir, &arguments)?;
    let words: Vec<&str> = arguments.iter().map(String::as_str).collect();
    match words.as_slice() {
        ["info", ..] => info(&dir),
        ["ps", rest @ ..] => ps(&dir, rest),
        ["rm", "-f", names @ ..] => remove(&dir, names),
        ["stop", "--time", _, names @ ..] => stop(&dir, names),
        ["volume", "create", _] => Ok(failing(&dir, "volume_fails")),
        ["run", rest @ ..] => run_container(&dir, rest),
        _ => Err(format!("unexpected call: {arguments:?}")),
    }
}

fn record(dir: &Path, arguments: &[String]) -> Result<(), String> {
    for number in 0..100_000 {
        let stem = dir.join(format!("inv-{number:06}"));
        let argv = stem.with_extension("argv");
        let created = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&argv);
        let Ok(mut file) = created else {
            continue;
        };
        file.write_all(arguments.join("\0").as_bytes())
            .map_err(|error| error.to_string())?;
        fs::write(stem.with_extension("pid"), std::process::id().to_string())
            .map_err(|error| error.to_string())?;
        record_process_group(&stem);
        if let Some(value) = std::env::var_os(JIT_VARIABLE) {
            fs::write(
                stem.with_extension("jit"),
                value.to_string_lossy().as_bytes(),
            )
            .map_err(|error| error.to_string())?;
        }
        return Ok(());
    }
    Err("no invocation number".to_string())
}

/// Writes the process group this call runs in, where `ps` can tell it.
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
    fs::read_to_string(dir.join(file))
        .ok()
        .map(|text| text.trim().to_string())
}

fn failing(dir: &Path, flag: &str) -> u8 {
    u8::from(exists(dir, flag))
}

fn info(dir: &Path) -> Result<u8, String> {
    let remaining: u32 = text(dir, "info_failures")
        .and_then(|text| text.parse().ok())
        .unwrap_or(0);
    if remaining == 0 {
        return Ok(0);
    }
    fs::write(dir.join("info_failures"), (remaining - 1).to_string())
        .map_err(|error| error.to_string())?;
    Ok(1)
}

/// One fake container: its name and labels.
struct Container {
    name: String,
    labels: Vec<String>,
}

fn load(dir: &Path) -> Vec<Container> {
    text(dir, "containers")
        .unwrap_or_default()
        .lines()
        .filter_map(|line| {
            let (name, labels) = line.split_once('\t').unwrap_or((line, ""));
            (!name.is_empty()).then(|| Container {
                name: name.to_string(),
                labels: labels
                    .split(',')
                    .filter(|label| !label.is_empty())
                    .map(str::to_string)
                    .collect(),
            })
        })
        .collect()
}

fn save(dir: &Path, containers: &[Container]) -> Result<(), String> {
    let lines: Vec<String> = containers
        .iter()
        .map(|container| format!("{}\t{}", container.name, container.labels.join(",")))
        .collect();
    fs::write(dir.join("containers"), lines.join("\n")).map_err(|error| error.to_string())
}

fn ps(dir: &Path, arguments: &[&str]) -> Result<u8, String> {
    let wants_label = arguments
        .iter()
        .any(|argument| argument.starts_with("label="));
    if exists(dir, "ps_fails") || (wants_label && exists(dir, "ps_label_fails")) {
        return Ok(1);
    }
    let filters: Vec<&str> = arguments
        .windows(2)
        .filter(|pair| pair.first() == Some(&"--filter"))
        .filter_map(|pair| pair.get(1).copied())
        .collect();
    let mut out = std::io::stdout();
    for container in load(dir) {
        let matches = filters.iter().all(|filter| match filter.split_once('=') {
            Some(("name", pattern)) => {
                match pattern.strip_prefix('^').and_then(|p| p.strip_suffix('$')) {
                    Some(exact) => container.name == exact,
                    None => container.name.contains(pattern),
                }
            }
            Some(("label", label)) => container.labels.iter().any(|have| have == label),
            _ => false,
        });
        if matches {
            let _ = writeln!(out, "{}", container.name);
        }
    }
    Ok(0)
}

fn remove(dir: &Path, names: &[&str]) -> Result<u8, String> {
    if exists(dir, "rm_fails") {
        return Ok(1);
    }
    let kept: Vec<Container> = load(dir)
        .into_iter()
        .filter(|container| !names.contains(&container.name.as_str()))
        .collect();
    save(dir, &kept)?;
    Ok(0)
}

fn stop(dir: &Path, names: &[&str]) -> Result<u8, String> {
    if exists(dir, "stop_hangs") {
        sleep_while(|| true);
        return Ok(1);
    }
    if exists(dir, "stop_fails") {
        if text(dir, "stop_fails").as_deref() == Some("longline") {
            print_long_line(&mut std::io::stderr());
        }
        return Ok(1);
    }
    remove(dir, names)
}

fn value_after<'a>(arguments: &[&'a str], flag: &str) -> Option<&'a str> {
    arguments
        .windows(2)
        .find(|pair| pair.first() == Some(&flag))
        .and_then(|pair| pair.get(1).copied())
}

fn run_container(dir: &Path, arguments: &[&str]) -> Result<u8, String> {
    if value_after(arguments, "--user") == Some("root") {
        return Ok(failing(dir, "volume_fails"));
    }
    let name = value_after(arguments, "--name").ok_or("a run without --name")?;
    if value_after(arguments, "--label") == Some(PROBE_LABEL) {
        return probe(dir);
    }
    job(dir, name)
}

fn probe(dir: &Path) -> Result<u8, String> {
    let mut out = std::io::stdout();
    let _ = writeln!(std::io::stderr(), "probe noise that must be discarded");
    match text(dir, "probe_mode").as_deref() {
        Some("fail") => Ok(3),
        Some("hang") => {
            sleep_while(|| true);
            Ok(0)
        }
        Some("flood") => {
            let chunk = [b'1'; 4096];
            for _ in 0..17 {
                if out.write_all(&chunk).is_err() {
                    break;
                }
            }
            let _ = out.flush();
            Ok(0)
        }
        Some("garbage") => {
            let _ = writeln!(out, "1;exit");
            Ok(0)
        }
        _ => {
            let cpus = text(dir, "probe_cpus").unwrap_or_else(|| "0-3".to_string());
            let _ = writeln!(out, "{cpus}");
            Ok(0)
        }
    }
}

fn job(dir: &Path, name: &str) -> Result<u8, String> {
    let mut containers = load(dir);
    containers.push(Container {
        name: name.to_string(),
        labels: Vec::new(),
    });
    save(dir, &containers)?;
    let _ = writeln!(std::io::stdout(), "fake runner started");
    let _ = writeln!(std::io::stderr(), "fake runner warning");
    if exists(dir, "run_prints_jit") {
        let jit = std::env::var(JIT_VARIABLE).unwrap_or_default();
        let _ = writeln!(std::io::stdout(), "jit on stdout: {jit}");
        let _ = writeln!(std::io::stderr(), "jit on stderr: {jit}");
    }
    if text(dir, "run_mode").as_deref() == Some("longline") {
        print_long_line(&mut std::io::stdout());
    }
    let code = if text(dir, "run_mode").as_deref() == Some("hold") {
        sleep_while(|| load(dir).iter().any(|container| container.name == name));
        137
    } else {
        let exit = text(dir, "run_exit")
            .and_then(|text| text.parse().ok())
            .unwrap_or(0);
        let kept: Vec<Container> = load(dir)
            .into_iter()
            .filter(|container| container.name != name)
            .collect();
        save(dir, &kept)?;
        exit
    };
    Ok(code)
}

/// Writes one line of [`LONG_LINE_BYTES`] bytes, the letters `a` to `z` in turn.
fn print_long_line(out: &mut impl Write) {
    let line: Vec<u8> = (b'a'..=b'z').cycle().take(LONG_LINE_BYTES).collect();
    let _ = out.write_all(&line);
    let _ = out.write_all(b"\n");
    let _ = out.flush();
}

fn sleep_while(mut condition: impl FnMut() -> bool) {
    let started = Instant::now();
    while condition() && started.elapsed() < LONGEST_WAIT {
        std::thread::sleep(Duration::from_millis(20));
    }
}
