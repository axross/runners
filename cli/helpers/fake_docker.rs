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

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use runners_cli::image::contract::{
    BUILT_LABEL, CONTRACT_LABEL, CONTRACT_VERSION, RUNNER_RELEASED_LABEL, RUNNER_VERSION_LABEL,
};
use runners_cli::image::definition::DOCKERFILE;

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
        ["image", "inspect", "--format", format, reference] => inspect(&dir, format, reference),
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

/// One fake image: a reference, the ID it names, and its labels.
#[derive(Clone)]
struct Image {
    reference: String,
    id: String,
    labels: Vec<(String, String)>,
}

/// The value the Dockerfile declares for `label`.
fn declared(label: &str) -> String {
    let text = String::from_utf8_lossy(DOCKERFILE);
    let needle = format!("{label}=\"");
    text.split_once(&needle)
        .and_then(|(_, rest)| rest.split_once('"'))
        .map(|(value, _)| value.to_string())
        .unwrap_or_default()
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
                declared(RUNNER_VERSION_LABEL),
            ),
            (
                RUNNER_RELEASED_LABEL.to_string(),
                declared(RUNNER_RELEASED_LABEL),
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
