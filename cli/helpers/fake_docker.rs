//! A stand-in for the `docker` command line client, for the tests that run the real
//! container backend.
//!
//! It is a compiled helper, not a script, because Windows finds only `.exe` programs.
//! `FAKE_DOCKER_DIR` names a directory it records into and reads its script from:
//!
//! - every call writes `inv-<n>.argv` (the arguments, NUL separated) and
//!   `inv-<n>.pid`, `inv-<n>.pgid` (on Unix), and `inv-<n>.jit` when `ACTIONS_RUNNER_INPUT_JITCONFIG` is set;
//!   it also holds an exclusive lock on `inv-<n>.alive` until it ends, so a test
//!   that takes that lock sees the call end, however it ends. It takes that lock
//!   before it writes its arguments, so a call a test can see already holds it;
//! - `containers` holds the fake engine's containers, one
//!   `name<TAB>label,label<TAB>state` per line, which `ps`, `rm`, `stop`, `kill`
//!   and `run` read and change. A container labelled as diagnostics evidence runs
//!   without `--rm`, so ending it leaves it in the file with the state `exited`;
//!   ending any other removes it;
//! - the files `info_failures` (a count, decremented per call, each failure
//!   saying on standard error that the daemon cannot be reached), `ps_fails`,
//!   `ps_label_fails`, `rm_fails`, `rm_hangs`, `stop_fails`, `stop_hangs`,
//!   `volume_fails`, `probe_mode` (`fail`, `hang`, `flood`, `garbage`),
//!   `probe_cpus`, `run_mode` (`hold`, `longline`), `run_prints_jit` and
//!   `run_exit` change what the call does; a `stop_fails` that holds `longline`
//!   also prints a long line to standard error.
//!
//! A `stop_hangs` stop ignores the grace period: it waits without stopping
//! anything, as a container that ignores its stop signal makes `docker stop` wait.
//! A `run_prints_jit` job container prints its JIT configuration to standard
//! output and standard error, as a careless runner could.
//!
//! A `hold` job container stays until its entry leaves `containers` or exits,
//! then exits 137, so `rm -f`, `stop` and `kill` end it as they would end a real
//! one. Every wait is
//! bounded, so a test that fails does not leave the helper running. A `longline` job
//! container prints one line of [`LONG_LINE_BYTES`] bytes, the letters `a` to `z` in
//! turn, then exits as an ordinary job does.
//!
//! Images:
//!
//! - without an `images` file, every image exists, with this CLI's contract, the
//!   runner the Dockerfile declares, and a build time of now;
//! - an `images` file holds the fake engine's images instead, one
//!   `reference<TAB>id<TAB>label<TAB>label...` per line, each label `key=value`,
//!   which `image inspect`, `image ls`, `image rm`, `build` and `tag` read and
//!   change;
//! - `build` writes its standard input to `inv-<n>.stdin` and tags a new image
//!   with its `--label`s;
//! - the files `info_hangs`, `inspect_hangs`, `build_hangs` (holding `tagged` to
//!   tag before it hangs), `build_fails`, `tag_hangs` (holding `after` to tag
//!   before it hangs), `tag_fails`, `image_rm_fails`, `prune_hangs` and
//!   `prune_fails` change what those calls do.
//!
//! Diagnostic exports:
//!
//! - `inspect --format <format> <container>` prints a container's identity in the
//!   allowlisted shape; `identity_mode` (`fragmented`, `unfiltered`, `long`,
//!   `fail`, `hang`) changes it;
//! - `cp <container>:/tmp/runner-diagnostics/. -` prints the collector's archive:
//!   its directory entry and `metrics.txt`, whose text is `metrics_text` when that
//!   file exists and ends with `final=observed` otherwise; `cp_mode` (`raw`, which
//!   adds `raw-1.log`, `gap`, without the final observation, `bad`, with a
//!   traversal name, `fail`, `hang`, `slow`, which waits 2 s first) changes it;
//! - `kill_fails` fails a `kill`;
//! - `export` stands in for an exporter child that ignores its liveness pipe: it
//!   waits without reading its standard input.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use runners_cli::image::contract::{
    BUILT_LABEL, CONTRACT_LABEL, CONTRACT_VERSION, RUNNER_RELEASED_LABEL, RUNNER_VERSION_LABEL,
};
use runners_cli::image::definition::declared_label;

const JIT_VARIABLE: &str = "ACTIONS_RUNNER_INPUT_JITCONFIG";
const PROBE_LABEL: &str = "runners.cpu-probe=1";
const DIAGNOSTIC_LABEL: &str = "runners.diagnostic-lifecycle=1";
const EXITED: &str = "exited";
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
    // held until this call ends
    let _alive = record(&dir, &arguments)?;
    let words: Vec<&str> = arguments.iter().map(String::as_str).collect();
    match words.as_slice() {
        ["info", ..] => info(&dir),
        ["image", "inspect", "--format", format, reference] => inspect(&dir, format, reference),
        ["inspect", "--format", _, name] => identity(&dir, name),
        ["cp", source, "-"] => copy(&dir, source),
        ["kill", names @ ..] => kill(&dir, names),
        ["export", ..] => {
            sleep_while(|| true);
            Ok(1)
        }
        ["image", "ls", rest @ ..] => list_images(&dir, rest),
        ["image", "rm", references @ ..] => remove_images(&dir, references),
        ["image", "prune", ..] => prune(&dir),
        ["build", rest @ ..] => build(&dir, rest),
        ["tag", source, target] => tag(&dir, source, target),
        ["ps", rest @ ..] => ps(&dir, rest),
        ["rm", "-f", names @ ..] => remove(&dir, names),
        ["stop", "--time", _, names @ ..] => stop(&dir, names),
        ["volume", "create", _] => Ok(failing(&dir, "volume_fails")),
        ["run", rest @ ..] => run_container(&dir, rest),
        _ => Err(format!("unexpected call: {arguments:?}")),
    }
}

fn record(dir: &Path, arguments: &[String]) -> Result<fs::File, String> {
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
        // the arguments come last, so a call a test can name already holds its
        // lock and has written the rest
        let alive =
            fs::File::create(stem.with_extension("alive")).map_err(|error| error.to_string())?;
        alive.lock().map_err(|error| error.to_string())?;
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
        file.write_all(arguments.join("\0").as_bytes())
            .map_err(|error| error.to_string())?;
        return Ok(alive);
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
    if exists(dir, "info_hangs") {
        sleep_while(|| true);
        return Ok(1);
    }
    let remaining: u32 = text(dir, "info_failures")
        .and_then(|text| text.parse().ok())
        .unwrap_or(0);
    if remaining == 0 {
        return Ok(0);
    }
    fs::write(dir.join("info_failures"), (remaining - 1).to_string())
        .map_err(|error| error.to_string())?;
    let _ = writeln!(
        std::io::stderr(),
        "Cannot connect to the Docker daemon. Is the docker daemon running?"
    );
    Ok(1)
}

/// One fake container: its name, labels, and whether it has exited.
struct Container {
    name: String,
    labels: Vec<String>,
    exited: bool,
}

impl Container {
    /// A diagnostic container runs without `--rm`, so it outlives its end.
    fn kept(&self) -> bool {
        self.labels.iter().any(|label| label == DIAGNOSTIC_LABEL)
    }
}

fn load(dir: &Path) -> Vec<Container> {
    text(dir, "containers")
        .unwrap_or_default()
        .lines()
        .filter_map(|line| {
            let mut fields = line.split('\t');
            let name = fields.next().unwrap_or_default();
            let labels = fields.next().unwrap_or_default();
            let exited = fields.next() == Some(EXITED);
            (!name.is_empty()).then(|| Container {
                name: name.to_string(),
                labels: labels
                    .split(',')
                    .filter(|label| !label.is_empty())
                    .map(str::to_string)
                    .collect(),
                exited,
            })
        })
        .collect()
}

/// Changes the container store under a lock, since several clients run at once.
fn update(dir: &Path, change: impl FnOnce(Vec<Container>) -> Vec<Container>) -> Result<(), String> {
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join("containers.lock"))
        .map_err(|error| error.to_string())?;
    lock.lock().map_err(|error| error.to_string())?;
    save(dir, &change(load(dir)))
}

/// Replaces the store whole, so a client that reads it never sees it half written.
fn save(dir: &Path, containers: &[Container]) -> Result<(), String> {
    let lines: Vec<String> = containers
        .iter()
        .map(|container| {
            let state = if container.exited { EXITED } else { "" };
            format!(
                "{}\t{}\t{state}",
                container.name,
                container.labels.join(",")
            )
        })
        .collect();
    let staged = dir.join(format!("containers.{}", std::process::id()));
    fs::write(&staged, lines.join("\n")).map_err(|error| error.to_string())?;
    fs::rename(staged, dir.join("containers")).map_err(|error| error.to_string())
}

/// Ends the named containers as a real engine does: one run with `--rm` goes,
/// one without stays as exited.
fn end(dir: &Path, names: &[&str]) -> Result<(), String> {
    update(dir, |containers| {
        containers
            .into_iter()
            .filter(|container| !names.contains(&container.name.as_str()) || container.kept())
            .map(|mut container| {
                if names.contains(&container.name.as_str()) {
                    container.exited = true;
                }
                container
            })
            .collect()
    })
}

fn running(dir: &Path, name: &str) -> bool {
    load(dir)
        .iter()
        .any(|container| container.name == name && !container.exited)
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
    if exists(dir, "rm_hangs") {
        sleep_while(|| true);
        return Ok(1);
    }
    if exists(dir, "rm_fails") {
        return Ok(1);
    }
    update(dir, |containers| {
        containers
            .into_iter()
            .filter(|container| !names.contains(&container.name.as_str()))
            .collect()
    })?;
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
    end(dir, names)?;
    Ok(0)
}

fn kill(dir: &Path, names: &[&str]) -> Result<u8, String> {
    if exists(dir, "kill_fails") {
        return Ok(1);
    }
    end(dir, names)?;
    Ok(0)
}

fn container_exists(dir: &Path, name: &str) -> bool {
    load(dir).iter().any(|container| container.name == name)
}

/// The allowlisted identity of a container, or what `identity_mode` asks for.
fn identity(dir: &Path, name: &str) -> Result<u8, String> {
    if !container_exists(dir, name) {
        let _ = writeln!(std::io::stderr(), "Error: No such container: {name}");
        return Ok(1);
    }
    let readback = format!(
        "sha256:{}|1500000000|17179869184|17179869184|150000|100000|0-3|false|0",
        "0".repeat(64)
    );
    let mut out = std::io::stdout();
    match text(dir, "identity_mode").as_deref() {
        Some("fragmented") => {
            let _ = write!(out, "sha256:");
            let _ = out.flush();
            std::thread::sleep(Duration::from_millis(30));
            let _ = writeln!(out, "{}", readback.trim_start_matches("sha256:"));
        }
        Some("unfiltered") => {
            let _ = writeln!(out, "4f0c3a51b8e24d6f9a7e2c1d0b9a8f7e");
        }
        Some("long") => {
            let _ = writeln!(out, "{readback}{}", " ".repeat(2000));
        }
        Some("fail") => return Ok(1),
        Some("hang") => {
            sleep_while(|| true);
            return Ok(1);
        }
        _ => {
            let _ = writeln!(out, "{readback}");
        }
    }
    Ok(0)
}

/// Copies `bytes` into `header` at `start`.
fn put(header: &mut [u8], start: usize, bytes: &[u8]) {
    if let Some(slot) = header.get_mut(start..start + bytes.len()) {
        slot.copy_from_slice(bytes);
    }
}

/// One ustar header for a file or directory entry of `size` bytes.
fn tar_header(name: &str, size: usize, kind: u8) -> Vec<u8> {
    let mut header = vec![0u8; 512];
    put(&mut header, 0, name.as_bytes());
    put(&mut header, 100, b"0000644");
    put(&mut header, 124, format!("{size:011o}").as_bytes());
    put(&mut header, 156, &[kind]);
    put(&mut header, 257, b"ustar");
    put(&mut header, 148, b"        ");
    let sum: u32 = header.iter().map(|byte| u32::from(*byte)).sum();
    put(&mut header, 148, format!("{sum:06o}\0").as_bytes());
    header
}

fn tar_file(archive: &mut Vec<u8>, name: &str, contents: &[u8]) {
    archive.extend(tar_header(name, contents.len(), b'0'));
    archive.extend(contents);
    archive.resize(archive.len().div_ceil(512) * 512, 0);
}

/// The collector's directory as `docker cp` streams it, or what `cp_mode` asks for.
fn copy(dir: &Path, source: &str) -> Result<u8, String> {
    let name = source
        .strip_suffix(":/tmp/runner-diagnostics/.")
        .ok_or_else(|| format!("unexpected copy source: {source}"))?;
    if !container_exists(dir, name) {
        let _ = writeln!(std::io::stderr(), "Error: No such container: {name}");
        return Ok(1);
    }
    let mode = text(dir, "cp_mode").unwrap_or_default();
    match mode.as_str() {
        "fail" => return Ok(1),
        "hang" => {
            sleep_while(|| true);
            return Ok(1);
        }
        "slow" => std::thread::sleep(Duration::from_secs(2)),
        _ => {}
    }
    let metrics = fs::read_to_string(dir.join("metrics_text")).unwrap_or_else(|_| {
        if mode == "gap" {
            "sample=1\n".to_string()
        } else {
            "sample=1\nfinal=observed\n".to_string()
        }
    });
    let mut archive = tar_header("./", 0, b'5');
    let metrics_name = if mode == "bad" {
        "../metrics.txt"
    } else {
        "./metrics.txt"
    };
    tar_file(&mut archive, metrics_name, metrics.as_bytes());
    if mode == "raw" {
        tar_file(&mut archive, "./raw-1.log", b"raw record\n");
    }
    archive.extend([0u8; 1024]);
    let mut out = std::io::stdout();
    let _ = out.write_all(&archive);
    let _ = out.flush();
    Ok(0)
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
    let label = value_after(arguments, "--label");
    if label == Some(PROBE_LABEL) {
        return probe(dir);
    }
    job(dir, name, label == Some(DIAGNOSTIC_LABEL))
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

fn job(dir: &Path, name: &str, diagnostic: bool) -> Result<u8, String> {
    update(dir, |mut containers| {
        containers.push(Container {
            name: name.to_string(),
            labels: if diagnostic {
                vec![DIAGNOSTIC_LABEL.to_string()]
            } else {
                Vec::new()
            },
            exited: false,
        });
        containers
    })?;
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
        sleep_while(|| running(dir, name));
        137
    } else {
        let exit = text(dir, "run_exit")
            .and_then(|text| text.parse().ok())
            .unwrap_or(0);
        end(dir, &[name])?;
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

/// One fake image: a reference, the ID it names, and its labels.
#[derive(Clone)]
struct Image {
    reference: String,
    id: String,
    labels: Vec<(String, String)>,
}

fn now_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

/// The image every reference names when there is no `images` file.
fn default_image(reference: &str) -> Image {
    Image {
        reference: reference.to_string(),
        id: "sha256:default".to_string(),
        labels: vec![
            (CONTRACT_LABEL.to_string(), CONTRACT_VERSION.to_string()),
            (BUILT_LABEL.to_string(), now_seconds().to_string()),
            (
                RUNNER_VERSION_LABEL.to_string(),
                declared_label(RUNNER_VERSION_LABEL)
                    .unwrap_or_default()
                    .to_string(),
            ),
            (
                RUNNER_RELEASED_LABEL.to_string(),
                declared_label(RUNNER_RELEASED_LABEL)
                    .unwrap_or_default()
                    .to_string(),
            ),
        ],
    }
}

/// The images, or `None` when there is no `images` file and every image exists.
fn load_images(dir: &Path) -> Option<Vec<Image>> {
    let text = fs::read_to_string(dir.join("images")).ok()?;
    Some(
        text.lines()
            .filter_map(|line| {
                let mut fields = line.split('\t');
                let reference = fields.next().filter(|reference| !reference.is_empty())?;
                let id = fields.next().unwrap_or_default();
                Some(Image {
                    reference: reference.to_string(),
                    id: id.to_string(),
                    labels: fields
                        .filter_map(|label| label.split_once('='))
                        .map(|(key, value)| (key.to_string(), value.to_string()))
                        .collect(),
                })
            })
            .collect(),
    )
}

fn save_images(dir: &Path, images: &[Image]) -> Result<(), String> {
    let lines: Vec<String> = images
        .iter()
        .map(|image| {
            let mut fields = vec![image.reference.clone(), image.id.clone()];
            fields.extend(
                image
                    .labels
                    .iter()
                    .map(|(key, value)| format!("{key}={value}")),
            );
            fields.join("\t")
        })
        .collect();
    fs::write(dir.join("images"), lines.join("\n")).map_err(|error| error.to_string())
}

fn find_image(dir: &Path, reference: &str) -> Option<Image> {
    match load_images(dir) {
        None => Some(default_image(reference)),
        Some(images) => images
            .into_iter()
            .find(|image| image.reference == reference),
    }
}

fn json_string(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Prints the image's labels as the CLI's format asks: each
/// `{{json (index .Config.Labels "<key>")}}` becomes the label's value as JSON, or
/// `""` when the image has no such label.
fn inspect(dir: &Path, format: &str, reference: &str) -> Result<u8, String> {
    if exists(dir, "inspect_hangs") {
        sleep_while(|| true);
        return Ok(1);
    }
    let Some(image) = find_image(dir, reference) else {
        let _ = writeln!(std::io::stderr(), "Error: No such image: {reference}");
        return Ok(1);
    };
    let mut printed = String::new();
    let mut rest = format;
    let opening = "{{json (index .Config.Labels \"";
    while let Some((before, after)) = rest.split_once(opening) {
        let (key, after) = after
            .split_once("\")}}")
            .ok_or_else(|| format!("unexpected format: {format}"))?;
        let value = image
            .labels
            .iter()
            .find(|(have, _)| have == key)
            .map_or("", |(_, value)| value.as_str());
        printed.push_str(before);
        printed.push_str(&json_string(value));
        rest = after;
    }
    printed.push_str(rest);
    if printed.contains("{{") {
        return Err(format!("unexpected format: {format}"));
    }
    let _ = writeln!(std::io::stdout(), "{printed}");
    Ok(0)
}

/// `image ls --quiet <reference>`, or `image ls` with `reference=` and `label=`
/// filters, printing each matching reference.
fn list_images(dir: &Path, arguments: &[&str]) -> Result<u8, String> {
    let mut out = std::io::stdout();
    if let ["--quiet", reference] = arguments {
        if let Some(image) = find_image(dir, reference) {
            let _ = writeln!(out, "{}", image.id);
        }
        return Ok(0);
    }
    let filters: Vec<&str> = arguments
        .windows(2)
        .filter(|pair| pair.first() == Some(&"--filter"))
        .filter_map(|pair| pair.get(1).copied())
        .collect();
    for image in load_images(dir).unwrap_or_default() {
        let matches = filters.iter().all(|filter| match filter.split_once('=') {
            Some(("reference", pattern)) => match pattern.strip_suffix('*') {
                Some(prefix) => image.reference.starts_with(prefix),
                None => image.reference == pattern,
            },
            Some(("label", label)) => image
                .labels
                .iter()
                .any(|(key, value)| format!("{key}={value}") == label),
            _ => false,
        });
        if matches {
            let _ = writeln!(out, "{}", image.reference);
        }
    }
    Ok(0)
}

fn remove_images(dir: &Path, references: &[&str]) -> Result<u8, String> {
    if exists(dir, "image_rm_fails") {
        return Ok(1);
    }
    let images = load_images(dir).unwrap_or_default();
    if references
        .iter()
        .any(|reference| !images.iter().any(|image| image.reference == *reference))
    {
        let _ = writeln!(std::io::stderr(), "Error: No such image");
        return Ok(1);
    }
    let kept: Vec<Image> = images
        .into_iter()
        .filter(|image| !references.contains(&image.reference.as_str()))
        .collect();
    save_images(dir, &kept)?;
    Ok(0)
}

fn prune(dir: &Path) -> Result<u8, String> {
    if exists(dir, "prune_hangs") {
        sleep_while(|| true);
        return Ok(1);
    }
    Ok(failing(dir, "prune_fails"))
}

/// Reads the context from standard input into `inv-<n>.stdin`, then tags a new
/// image with the build's labels.
fn build(dir: &Path, arguments: &[&str]) -> Result<u8, String> {
    let mut context = Vec::new();
    std::io::Read::read_to_end(&mut std::io::stdin(), &mut context)
        .map_err(|error| error.to_string())?;
    let stem = latest_invocation(dir)?;
    fs::write(stem.with_extension("stdin"), &context).map_err(|error| error.to_string())?;
    let _ = writeln!(std::io::stdout(), "fake build output");
    let hangs = text(dir, "build_hangs");
    if hangs.as_deref() == Some("") {
        sleep_while(|| true);
        return Ok(1);
    }
    if exists(dir, "build_fails") {
        return Ok(1);
    }
    let tag = value_after(arguments, "--tag").ok_or("a build without --tag")?;
    let labels = arguments
        .windows(2)
        .filter(|pair| pair.first() == Some(&"--label"))
        .filter_map(|pair| pair.get(1).and_then(|label| label.split_once('=')))
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect();
    let id = format!(
        "sha256:built-{}",
        stem.file_name().unwrap_or_default().to_string_lossy()
    );
    let mut images = load_images(dir).unwrap_or_default();
    images.retain(|image| image.reference != tag);
    images.push(Image {
        reference: tag.to_string(),
        id,
        labels,
    });
    save_images(dir, &images)?;
    if hangs.is_some() {
        sleep_while(|| true);
        return Ok(1);
    }
    Ok(0)
}

/// The stem of the newest `inv-<n>` this call recorded.
fn latest_invocation(dir: &Path) -> Result<PathBuf, String> {
    let pid = std::process::id().to_string();
    (0..100_000)
        .map(|number| dir.join(format!("inv-{number:06}")))
        .take_while(|stem| stem.with_extension("argv").exists())
        .filter(|stem| {
            fs::read_to_string(stem.with_extension("pid"))
                .ok()
                .as_deref()
                == Some(pid.as_str())
        })
        .last()
        .ok_or_else(|| "this call was not recorded".to_string())
}

fn tag(dir: &Path, source: &str, target: &str) -> Result<u8, String> {
    let hangs = text(dir, "tag_hangs");
    if hangs.as_deref() == Some("") {
        sleep_while(|| true);
        return Ok(1);
    }
    if exists(dir, "tag_fails") {
        return Ok(1);
    }
    let mut images = load_images(dir).unwrap_or_default();
    let Some(found) = images
        .iter()
        .find(|image| image.reference == source)
        .cloned()
    else {
        let _ = writeln!(std::io::stderr(), "Error: No such image: {source}");
        return Ok(1);
    };
    images.retain(|image| image.reference != target);
    images.push(Image {
        reference: target.to_string(),
        ..found
    });
    save_images(dir, &images)?;
    if hangs.is_some() {
        sleep_while(|| true);
    }
    Ok(0)
}
