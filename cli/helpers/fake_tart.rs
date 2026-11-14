//! a stand-in for the `tart` client, compiled because Windows finds only `.exe`
//! programs, for the tests that run the real Tart client code as a process.
//! `FAKE_TART_DIR` names a directory it records into and reads its script from:
//!
//! - every call writes `inv-<n>.argv` (the arguments, NUL separated), `inv-<n>.pid`,
//!   `inv-<n>.pgid` (on Unix) and `inv-<n>.env`, which holds the lines
//!   `TART_NO_AUTO_PRUNE=<value>` and `PATH=<value>` when those variables are set,
//!   `JIT=present` when `ACTIONS_RUNNER_INPUT_JITCONFIG` is set (its value is never
//!   written), and `stdin_terminal=`, `stdout_terminal=` and `stderr_terminal=`
//!   with `true` or `false`. It holds an exclusive lock on `inv-<n>.alive` until it
//!   ends, taken before the arguments are written, so a test that takes that lock
//!   sees the call end, however it ends;
//! - `--version` prints the file `version_output` exactly as it holds it, else
//!   `2.41.1` and a newline, as Tart 2.41.1 does; scripts name it `version`;
//! - `vms` holds the fake Tart's VMs, one
//!   `name<TAB>state<TAB>source<TAB>cpu<TAB>memory MB<TAB>disk GB` per line, which
//!   `list`, `clone`, `set`, `run`, `stop`, `delete`, `pull` and `rename` read and
//!   change; the state is `stopped` or `running`, the source `local` or `OCI`, and
//!   the last three fields are empty until `set` gives them. `list --format json`
//!   prints them in the shape Tart prints, including its escaped slashes;
//! - `exec` records its command in the arguments, reads its standard input to the
//!   end and writes it to `inv-<n>.stdin` only with `-i`, prints `exec_stdout`, and
//!   exits with `exec_exit`. It refuses `-t`, and a VM that is not running. With a
//!   `docker_program` file that names the fake `docker`, it instead runs that program
//!   in the VM's place for a command that starts with `docker`, or with the launcher
//!   `/usr/local/bin/marshal-launch`, or with `sudo -n docker`, which the golden VM's
//!   image build runs as root: the program gets the command's arguments, its
//!   `FAKE_DOCKER_DIR` is the `docker` directory beside this one, shared by all VMs,
//!   and it exits when this call ends. The launcher's standard input becomes the
//!   program's `ACTIONS_RUNNER_INPUT_JITCONFIG`, as the real launcher does, and an
//!   empty one fails with exit code 64;
//! - an `exec -i` of the launcher with one absolute path, the runner's directory in
//!   a macOS job VM, stands in for the macOS runner: it refuses an empty standard
//!   input with exit code 64, appends the VM's name to `macos_runs`, holds while
//!   `macos_hold` exists and the VM runs, and exits with `macos_exit` (default 0);
//! - a provisioning call, which runs `bash -c <script> <name>` (after `sudo -n` when it
//!   is privileged), takes its script from files that carry its `<name>`, such as
//!   `marshal-list-key`: `exec_stdout-<name>` and `exec_exit-<name>` replace the
//!   generic `exec_stdout` and `exec_exit`, and `fail-exec-<name>`, `hang-exec-<name>`
//!   and `flood-exec-<name>` work as the subcommand scripts below do;
//! - `run` marks its VM running and holds until the VM is no longer running, then
//!   exits 0. `run_mode` set to `exit` makes it print `run_error` to standard error
//!   and exit with `run_exit` (default 1) without starting anything;
//! - `stop` refuses a VM that is not running, and with `stop_slow` present waits
//!   its `--timeout` seconds first, as Tart waits for a VM that ignores the
//!   interrupt; with `stop_ignored` present a stop with a timeout above zero waits
//!   that long and then fails without stopping the VM, and only a stop with no
//!   timeout stops it;
//! - `delete` refuses a running VM and a VM that does not exist;
//! - `get <name> --format json` prints an object whose `OS` is the file `os-<name>`,
//!   else `darwin`, and fails for a VM that does not exist;
//! - any subcommand called `<sub>` (`version` for `--version`) fails with `fail-<sub>` present, printing the
//!   file's text, or a default one, to standard error; hangs with `hang-<sub>`
//!   present; and prints 200 000 bytes to each of standard output and standard
//!   error first with `flood-<sub>` present. Every wait is bounded, so a test that
//!   fails does not leave the helper running.

mod fake_support;

use std::fmt::Write as _;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::time::{Duration, Instant};

use fake_support::{JIT_VARIABLE, exists, record, scripted, sleep_while, text};

const LAUNCHER: &str = "/usr/local/bin/marshal-launch";
const RECORDED_VARIABLES: [&str; 2] = ["TART_NO_AUTO_PRUNE", "PATH"];
const DEFAULT_VERSION: &str = "2.41.1\n";
const MAX_STDIN_BYTES: u64 = 1024 * 1024;
const RUNNING: &str = "running";
const STOPPED: &str = "stopped";
const LOCAL: &str = "local";
const OCI: &str = "OCI";

fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(code),
        Err(message) => {
            let _ = writeln!(std::io::stderr(), "fake-tart: {message}");
            ExitCode::from(70)
        }
    }
}

fn run() -> Result<u8, String> {
    let dir = PathBuf::from(std::env::var_os("FAKE_TART_DIR").ok_or("FAKE_TART_DIR is not set")?);
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let (stem, _held_until_exit) = record(&dir, &arguments, &RECORDED_VARIABLES)?;
    let words: Vec<&str> = arguments.iter().map(String::as_str).collect();
    let scripted_name = match words.first() {
        Some(&"--version") => Some("version"),
        other => other.copied(),
    };
    if let Some(code) = scripted_name.and_then(|subcommand| scripted(&dir, subcommand)) {
        return Ok(code);
    }
    match words.as_slice() {
        ["--version"] => version(&dir),
        ["list", "--format", "json"] => list(&dir),
        ["clone", source, target] => clone(&dir, source, target),
        ["set", name, options @ ..] => set(&dir, name, options),
        ["run", options @ .., name] => start(&dir, options, name),
        ["exec", rest @ ..] => exec(&dir, &stem, rest),
        ["stop", name, "--timeout", seconds] => stop(&dir, name, seconds),
        ["delete", names @ ..] => delete(&dir, names),
        ["pull", reference] => pull(&dir, reference),
        ["rename", name, target] => rename(&dir, name, target),
        ["get", name, "--format", "json"] => get(&dir, name),
        _ => Err(format!("unexpected call: {arguments:?}")),
    }
}

#[derive(Clone)]
struct Vm {
    name: String,
    state: String,
    source: String,
    cpu: String,
    memory: String,
    disk: String,
}

impl Vm {
    fn new(name: &str, source: &str) -> Self {
        Self {
            name: name.to_string(),
            state: STOPPED.to_string(),
            source: source.to_string(),
            cpu: String::new(),
            memory: String::new(),
            disk: String::new(),
        }
    }

    fn running(&self) -> bool {
        self.state == RUNNING
    }
}

fn load(dir: &Path) -> Vec<Vm> {
    text(dir, "vms")
        .unwrap_or_default()
        .lines()
        .filter_map(|line| {
            let mut fields = line.split('\t').map(str::to_string);
            let name = fields.next().filter(|name| !name.is_empty())?;
            Some(Vm {
                name,
                state: fields.next().unwrap_or_default(),
                source: fields.next().unwrap_or_default(),
                cpu: fields.next().unwrap_or_default(),
                memory: fields.next().unwrap_or_default(),
                disk: fields.next().unwrap_or_default(),
            })
        })
        .collect()
}

/// changes the VM store under a lock, as several calls run at once, and replaces it
/// whole so a reader never sees it half written. A change that refuses prints its
/// message as Tart does, leaves the store as it was, and answers exit code 1.
fn edit(dir: &Path, change: impl FnOnce(&mut Vec<Vm>) -> Result<(), String>) -> Result<u8, String> {
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join("vms.lock"))
        .map_err(|error| error.to_string())?;
    lock.lock().map_err(|error| error.to_string())?;
    let mut vms = load(dir);
    if let Err(message) = change(&mut vms) {
        let _ = writeln!(std::io::stderr(), "Error: {message}");
        return Ok(1);
    }
    let lines: Vec<String> = vms
        .iter()
        .map(|vm| {
            format!(
                "{}\t{}\t{}\t{}\t{}\t{}",
                vm.name, vm.state, vm.source, vm.cpu, vm.memory, vm.disk
            )
        })
        .collect();
    let staged = dir.join(format!("vms.{}", std::process::id()));
    fs::write(&staged, lines.join("\n")).map_err(|error| error.to_string())?;
    fs::rename(staged, dir.join("vms")).map_err(|error| error.to_string())?;
    Ok(0)
}

/// prints `version_output` exactly as the file holds it, else `2.41.1` and a newline.
fn version(dir: &Path) -> Result<u8, String> {
    let output = fs::read_to_string(dir.join("version_output"))
        .unwrap_or_else(|_| DEFAULT_VERSION.to_string());
    let _ = std::io::stdout().write_all(output.as_bytes());
    Ok(0)
}

fn find<'a>(vms: &'a mut [Vm], name: &str) -> Result<&'a mut Vm, String> {
    vms.iter_mut()
        .find(|vm| vm.name == name)
        .ok_or_else(|| format!("the VM {name} does not exist"))
}

fn json_string(text: &str) -> String {
    let mut out = String::from("\"");
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '/' => out.push_str("\\/"),
            control if control.is_control() => {
                let _ = write!(out, "\\u{:04x}", u32::from(control));
            }
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

fn list(dir: &Path) -> Result<u8, String> {
    let mut vms = load(dir);
    vms.sort_by(|left, right| {
        (left.source != LOCAL, &left.name).cmp(&(right.source != LOCAL, &right.name))
    });
    let mut out = String::from("[\n");
    let entries: Vec<String> = vms
        .iter()
        .map(|vm| {
            let fields = [
                ("Source", json_string(&vm.source)),
                ("Name", json_string(&vm.name)),
                ("Disk", "20".to_string()),
                ("Size", "3".to_string()),
                ("Accessed", "\"2026-01-01T00:00:00Z\"".to_string()),
                ("Running", vm.running().to_string()),
                ("State", json_string(&vm.state)),
            ];
            let lines: Vec<String> = fields
                .iter()
                .map(|(key, value)| format!("    \"{key}\" : {value}"))
                .collect();
            format!("  {{\n{}\n  }}", lines.join(",\n"))
        })
        .collect();
    if entries.is_empty() {
        out.push('\n');
    } else {
        out.push_str(&entries.join(",\n"));
    }
    out.push_str("\n]\n");
    let _ = std::io::stdout().write_all(out.as_bytes());
    Ok(0)
}

fn clone(dir: &Path, source: &str, target: &str) -> Result<u8, String> {
    edit(dir, |vms| {
        let mut copy = find(vms, source)?.clone();
        if vms.iter().any(|vm| vm.name == target) {
            return Err(format!("the VM {target} already exists"));
        }
        copy.name = target.to_string();
        copy.source = LOCAL.to_string();
        copy.state = STOPPED.to_string();
        vms.push(copy);
        Ok(())
    })
}

fn set(dir: &Path, name: &str, options: &[&str]) -> Result<u8, String> {
    edit(dir, |vms| {
        let vm = find(vms, name)?;
        if vm.running() || vm.source != LOCAL {
            return Err(format!("the VM {name} cannot be changed"));
        }
        for pair in options.chunks(2) {
            match pair {
                ["--cpu", value] => vm.cpu = (*value).to_string(),
                ["--memory", value] => vm.memory = (*value).to_string(),
                ["--disk-size", value] => vm.disk = (*value).to_string(),
                _ => return Err(format!("unexpected set options: {options:?}")),
            }
        }
        Ok(())
    })
}

fn start(dir: &Path, _options: &[&str], name: &str) -> Result<u8, String> {
    if text(dir, "run_mode").as_deref() == Some("exit") {
        let message = text(dir, "run_error").unwrap_or_default();
        let _ = writeln!(std::io::stderr(), "{message}");
        let code = text(dir, "run_exit")
            .and_then(|code| code.parse().ok())
            .unwrap_or(1);
        return Ok(code);
    }
    let started = edit(dir, |vms| {
        let vm = find(vms, name)?;
        if vm.running() || vm.source != LOCAL {
            return Err(format!("the VM {name} cannot be started"));
        }
        vm.state = RUNNING.to_string();
        Ok(())
    })?;
    if started != 0 {
        return Ok(started);
    }
    sleep_while(|| load(dir).iter().any(|vm| vm.name == name && vm.running()));
    Ok(0)
}

fn exec(dir: &Path, stem: &Path, rest: &[&str]) -> Result<u8, String> {
    let mut rest = rest;
    let mut interactive = false;
    while let Some((&flag, tail)) = rest.split_first() {
        match flag {
            "-i" => interactive = true,
            "-t" => {
                let _ = writeln!(std::io::stderr(), "fake-tart: exec never passes -t");
                return Ok(2);
            }
            _ => break,
        }
        rest = tail;
    }
    let Some((name, command)) = rest.split_first() else {
        return Err("exec without a VM".to_string());
    };
    let mut input = Vec::new();
    if interactive {
        let _ = std::io::stdin()
            .take(MAX_STDIN_BYTES)
            .read_to_end(&mut input);
        fs::write(stem.with_extension("stdin"), &input).map_err(|error| error.to_string())?;
    }
    if !load(dir).iter().any(|vm| vm.name == *name && vm.running()) {
        let _ = writeln!(std::io::stderr(), "Error: the VM {name} is not running");
        return Ok(1);
    }
    if let [LAUNCHER, directory] = command
        && directory.starts_with('/')
    {
        return Ok(macos_runner(dir, name, &input));
    }
    if let Some(code) = forward_to_docker(dir, command, &input)? {
        return Ok(code);
    }
    let step = provisioning_step(command);
    if let Some(code) = step.and_then(|name| scripted(dir, &format!("exec-{name}"))) {
        return Ok(code);
    }
    let file = |base: &str| match step {
        Some(name) if exists(dir, &format!("{base}-{name}")) => format!("{base}-{name}"),
        _ => base.to_string(),
    };
    if let Some(output) = text(dir, &file("exec_stdout")) {
        let _ = writeln!(std::io::stdout(), "{output}");
    }
    Ok(text(dir, &file("exec_exit"))
        .and_then(|code| code.parse().ok())
        .unwrap_or(0))
}

/// stands in for a macOS job VM's runner, which the launcher starts with the runner's
/// directory: refuses an empty JIT configuration as the launcher does, appends the
/// VM's name to `macos_runs`, holds while `macos_hold` exists and the VM runs, and
/// exits with
/// `macos_exit`, else 0.
fn macos_runner(dir: &Path, vm_name: &str, input: &[u8]) -> u8 {
    if input.is_empty() {
        let _ = writeln!(
            std::io::stderr(),
            "marshal-launch: standard input held no jit configuration"
        );
        return 64;
    }
    let mut runs = text(dir, "macos_runs").unwrap_or_default();
    runs.push_str(vm_name);
    runs.push('\n');
    let _ = fs::write(dir.join("macos_runs"), runs);
    let _ = writeln!(std::io::stdout(), "Listening for Jobs");
    sleep_while(|| {
        exists(dir, "macos_hold")
            && load(dir)
                .iter()
                .any(|vm| vm.name == vm_name && vm.running())
    });
    text(dir, "macos_exit")
        .and_then(|code| code.parse().ok())
        .unwrap_or(0)
}

/// the name of a provisioning call: the word after the script in `bash -c <script>
/// <name>`.
fn provisioning_step<'a>(command: &[&'a str]) -> Option<&'a str> {
    let at = command.iter().position(|word| *word == "bash")?;
    match command.get(at + 1..)? {
        ["-c", _script, name, ..] => Some(name),
        _ => None,
    }
}

/// runs the fake `docker` for a `docker` or launcher command when the script names
/// one; `None` for any other command.
fn forward_to_docker(dir: &Path, command: &[&str], input: &[u8]) -> Result<Option<u8>, String> {
    let Some(program) = text(dir, "docker_program") else {
        return Ok(None);
    };
    let (jit, arguments) = match command.split_first() {
        Some((&"docker", arguments)) | Some((&"sudo", ["-n", "docker", arguments @ ..])) => {
            (None, arguments)
        }
        Some((&LAUNCHER, arguments)) => {
            // the launcher's `$(...)` drops trailing newlines
            let jit = String::from_utf8_lossy(input)
                .trim_end_matches('\n')
                .to_string();
            if jit.is_empty() {
                let _ = writeln!(
                    std::io::stderr(),
                    "marshal-launch: standard input held no jit configuration"
                );
                return Ok(Some(64));
            }
            (Some(jit), arguments)
        }
        _ => return Ok(None),
    };
    let docker_dir = dir.join("docker");
    fs::create_dir_all(&docker_dir).map_err(|error| error.to_string())?;
    let mut docker = Command::new(program);
    docker
        .args(arguments)
        .env("FAKE_DOCKER_DIR", &docker_dir)
        .env("FAKE_DOCKER_WATCH_PARENT", "1")
        .env_remove(JIT_VARIABLE)
        .stdin(Stdio::null());
    if let Some(jit) = jit {
        docker.env(JIT_VARIABLE, jit);
    }
    let status = docker.status().map_err(|error| error.to_string())?;
    Ok(Some(
        status
            .code()
            .and_then(|code| u8::try_from(code).ok())
            .unwrap_or(137),
    ))
}

fn stop(dir: &Path, name: &str, seconds: &str) -> Result<u8, String> {
    let running = load(dir).iter().any(|vm| vm.name == name && vm.running());
    if !running {
        let _ = writeln!(std::io::stderr(), "Error: the VM {name} is not running");
        return Ok(1);
    }
    if exists(dir, "stop_ignored") && seconds != "0" {
        let wait = Duration::from_secs(seconds.parse().map_err(|_| "a bad timeout")?);
        let started = Instant::now();
        sleep_while(|| started.elapsed() < wait);
        let _ = writeln!(std::io::stderr(), "Error: the VM {name} did not stop");
        return Ok(1);
    }
    if exists(dir, "stop_slow") {
        let wait = Duration::from_secs(seconds.parse().map_err(|_| "a bad timeout")?);
        let started = Instant::now();
        sleep_while(|| started.elapsed() < wait);
    }
    edit(dir, |vms| {
        find(vms, name)?.state = STOPPED.to_string();
        Ok(())
    })
}

fn delete(dir: &Path, names: &[&str]) -> Result<u8, String> {
    edit(dir, |vms| {
        for name in names {
            if find(vms, name)?.running() {
                return Err(format!("the VM {name} is running"));
            }
        }
        vms.retain(|vm| !names.contains(&vm.name.as_str()));
        Ok(())
    })
}

fn pull(dir: &Path, reference: &str) -> Result<u8, String> {
    edit(dir, |vms| {
        if !vms.iter().any(|vm| vm.name == reference) {
            vms.push(Vm::new(reference, OCI));
        }
        Ok(())
    })
}

/// prints the VM's `OS` field as Tart 2.41.1 does: the file `os-<name>`, else
/// `darwin`; a VM that does not exist fails.
fn get(dir: &Path, name: &str) -> Result<u8, String> {
    if !load(dir).iter().any(|vm| vm.name == name) {
        return Err(format!("the VM {name} does not exist"));
    }
    let os = text(dir, &format!("os-{name}")).unwrap_or_else(|| "darwin".to_string());
    println!(
        "{{\n  \"CPU\" : 4,\n  \"OS\" : {},\n  \"State\" : \"stopped\"\n}}",
        json_string(os.trim())
    );
    Ok(0)
}

fn rename(dir: &Path, name: &str, target: &str) -> Result<u8, String> {
    edit(dir, |vms| {
        if vms.iter().any(|vm| vm.name == target) {
            return Err(format!("the VM {target} already exists"));
        }
        let vm = find(vms, name)?;
        if vm.running() || vm.source != LOCAL {
            return Err(format!("the VM {name} cannot be renamed"));
        }
        vm.name = target.to_string();
        Ok(())
    })
}
