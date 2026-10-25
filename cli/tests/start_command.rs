//! `start`, `stop`, `restart` and `status` as real processes, on a test root, with
//! the fake `docker` as the client `start` runs, and a loopback GitHub stub.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod support;

use std::fs;
use std::time::{Duration, Instant};

use runners_cli::image::contract::{CONTRACT_VERSION, RUNNER_VERSION_LABEL};
use runners_cli::image::definition::DOCKERFILE;
use support::host::{Host, JIT, Started, TOKEN, registered, texts};
use support::stub::{Reply, Stub};
use support::{TempDir, repository_root};

/// Long enough for a fresh instance to prepare its pool and run its container.
const RUNNING_WITHIN: Duration = Duration::from_secs(20);

/// Starts an instance whose job container holds until it is removed, and waits
/// until its slot runs the container.
fn running(host: &Host, stub: &Stub) -> Started {
    host.script("run_mode", "hold");
    let started = host.start(stub);
    wait_for_job(host);
    started
}

/// Waits, within [`RUNNING_WITHIN`], until the slot runs its container and the
/// fake engine holds it, and returns the status that showed it. The slot reports
/// the container before its client starts, so the status alone would race the
/// fake's record.
fn wait_for_job(host: &Host) -> String {
    let started = Instant::now();
    let status = host.wait_for_status("running container r-1-", RUNNING_WITHIN);
    while job_containers(host).is_empty() {
        assert!(
            started.elapsed() < RUNNING_WITHIN,
            "the fake docker never recorded the job container; its calls: {:?}",
            host.invocations()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    status
}

/// The runner version the embedded Dockerfile declares, which the fake engine's
/// images carry.
fn declared_runner_version() -> String {
    let text = String::from_utf8_lossy(DOCKERFILE);
    text.split_once(&format!("{RUNNER_VERSION_LABEL}=\""))
        .and_then(|(_, rest)| rest.split_once('"'))
        .map(|(version, _)| version.to_string())
        .expect("the Dockerfile declares no runner version")
}

fn job_containers(host: &Host) -> Vec<String> {
    host.containers()
        .into_iter()
        .filter(|name| name.starts_with("r-1-"))
        .collect()
}

#[test]
fn a_start_without_its_configuration_is_a_usage_error() {
    let host = Host::new();
    let output = host.client(&["start"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(host.invocations().is_empty());
}

#[test]
fn an_invalid_configuration_exits_1_and_changes_nothing() {
    let host = Host::new();
    let config = host.root.write("invalid.json", b"{}");
    let output = host
        .command(&["start", "--profile", "axpc", "--config"])
        .arg(&config)
        .output()
        .unwrap();
    let (_, stderr) = texts(&output);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains("Invalid host configuration"), "{stderr}");
    assert!(host.invocations().is_empty());
    assert!(host.log_files().is_empty(), "no log file before validation");
}

#[test]
fn the_axmc_profile_validates_and_then_exits_1_naming_phase_6() {
    let host = Host::new();
    let fixture =
        repository_root().join("tests/host-configuration/rust/accepted/axmc-defaults.json");
    let config = host.root.write("axmc.json", &fs::read(fixture).unwrap());
    let output = host
        .command(&["start", "--profile", "axmc", "--config"])
        .arg(&config)
        .output()
        .unwrap();
    let (_, stderr) = texts(&output);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains("Phase 6"), "{stderr}");
    assert!(host.invocations().is_empty());
}

#[test]
fn no_startable_pool_exits_1() {
    let host = Host::new();
    let missing = host.root.write(
        "missing-token.json",
        support::host::config_for("C:\\\\missing.token").as_bytes(),
    );
    let stub = Stub::start(Vec::new());
    let finished = host.start_with(&missing, &stub).finish(RUNNING_WITHIN);
    assert_eq!(finished.status.code(), Some(1), "{}", finished.stderr);
    assert!(
        finished.stderr.contains("no pool could start"),
        "{}",
        finished.stderr
    );
}

#[test]
fn with_no_instance_stop_restart_and_status_say_so_and_exit_1() {
    let host = Host::new();
    for client in ["stop", "restart", "status"] {
        let output = host.client(&[client]);
        let (_, stderr) = texts(&output);
        assert_eq!(output.status.code(), Some(1), "{client}: {stderr}");
        assert!(
            stderr.contains("No instance is running"),
            "{client}: {stderr}"
        );
    }
}

/// An `instance-helper` holding the host's lock and serving its channel, in `mode`
/// `serve` or `mute`. It ends when dropped, which closes its standard input.
struct Helper(std::process::Child);

impl Helper {
    fn serving(host: &Host, mode: &str) -> Self {
        let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_instance-helper"))
            .args([mode, "--root"])
            .arg(host.state())
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let mut line = String::new();
        std::io::BufRead::read_line(
            &mut std::io::BufReader::new(child.stdout.as_mut().unwrap()),
            &mut line,
        )
        .unwrap();
        let helper = Self(child);
        assert!(!line.starts_with("refused"), "{line}");
        helper
    }
}

impl Drop for Helper {
    fn drop(&mut self) {
        drop(self.0.stdin.take());
        let _ = self.0.wait();
    }
}

#[test]
fn status_gives_up_after_5_seconds_when_the_instance_does_not_answer() {
    let host = Host::new();
    let helper = Helper::serving(&host, "mute");
    let started = Instant::now();
    let output = host.client(&["status"]);
    let waited = started.elapsed();
    drop(helper);
    let (_, stderr) = texts(&output);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains("did not answer in time"), "{stderr}");
    assert!(waited >= Duration::from_secs(5), "{waited:?}");
    assert!(waited < Duration::from_secs(10), "{waited:?}");
}

#[test]
fn stop_gives_up_after_35_seconds_when_the_instance_does_not_end() {
    let host = Host::new();
    let helper = Helper::serving(&host, "serve");
    let started = Instant::now();
    let output = host.client(&["stop"]);
    let waited = started.elapsed();
    drop(helper);
    let (stdout, stderr) = texts(&output);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert!(stdout.contains("Stopping the instance."), "{stdout}");
    assert!(
        stderr.contains("the instance has not ended within 35 s"),
        "{stderr}"
    );
    assert!(waited >= Duration::from_secs(35), "{waited:?}");
    assert!(waited < Duration::from_secs(45), "{waited:?}");
}

#[test]
fn a_run_keeps_the_token_and_the_jit_config_out_of_every_output_it_writes() {
    let host = Host::new();
    host.script("run_mode", "hold");
    host.script("run_prints_jit", "");
    let stub = Stub::start(vec![
        Reply::status(
            "500 Internal Server Error",
            &format!("bad credentials {TOKEN}"),
        ),
        registered(),
    ]);
    let started = host.start(&stub);
    let status = wait_for_job(&host);
    let stop = host.client(&["stop"]);
    let finished = started.finish(Duration::from_secs(40));

    assert_eq!(finished.status.code(), Some(0), "{}", finished.stderr);
    assert_eq!(stop.status.code(), Some(0));
    assert!(job_containers(&host).is_empty());

    let canonical = fs::canonicalize(&host.config).unwrap();
    let lines: Vec<&str> = status.lines().collect();
    assert_eq!(lines[0], "state: running");
    assert_eq!(lines[1], format!("config: {}", canonical.display()));
    assert_eq!(lines[2], "profile: axpc");
    assert_eq!(lines[3], "pool: r: running");
    assert!(
        lines[4].starts_with("slot: r slot 1: running container r-1-"),
        "{status}"
    );
    assert_eq!(lines[5], "image: actions-runner:local");
    assert_eq!(lines[6], format!("image-contract: {CONTRACT_VERSION}"));
    assert_eq!(lines[7], "image-built: 0 days ago");
    assert!(
        lines[8].starts_with(&format!("runner: {}, released ", declared_runner_version())),
        "{status}"
    );

    // the secrets did reach the places they belong
    let authorization = stub.requests()[0]
        .header("authorization")
        .unwrap()
        .to_string();
    assert_eq!(authorization, format!("Bearer {TOKEN}"));
    let jits: Vec<String> = fs::read_dir(host.docker())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "jit"))
        .map(|path| fs::read_to_string(path).unwrap())
        .collect();
    assert_eq!(jits, [JIT]);

    let logs = host.log_files();
    assert_eq!(logs.len(), 1);
    let log = fs::read_to_string(&logs[0]).unwrap();
    assert!(log.contains("jit on stdout: <masked>"), "{log}");
    assert!(log.contains("jit on stderr: <masked>"), "{log}");
    assert!(
        log.contains("failed to request a JIT registration"),
        "{log}"
    );
    let (stop_out, stop_err) = texts(&stop);
    for (place, text) in [
        ("start's output", finished.stdout.as_str()),
        ("start's error output", finished.stderr.as_str()),
        ("the log file", log.as_str()),
        ("status", status.as_str()),
        ("stop's output", stop_out.as_str()),
        ("stop's error output", stop_err.as_str()),
    ] {
        assert!(!text.contains(TOKEN), "the token reached {place}:\n{text}");
        assert!(
            !text.contains(JIT),
            "the JIT config reached {place}:\n{text}"
        );
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode =
            |path: &std::path::Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&host.state().join("logs")), 0o700);
        assert_eq!(mode(&logs[0]), 0o600);
    }
}

#[test]
fn at_most_ten_log_files_are_kept() {
    let host = Host::new();
    let logs = host.state().join("logs");
    fs::create_dir_all(&logs).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&logs, fs::Permissions::from_mode(0o700)).unwrap();
    }
    for index in 0..10 {
        fs::write(logs.join(format!("runners-{index:017}-1.log")), b"old").unwrap();
    }
    let stub = Stub::start(vec![registered()]);
    let started = running(&host, &stub);
    assert_eq!(host.client(&["stop"]).status.code(), Some(0));
    started.finish(Duration::from_secs(40));
    let kept = host.log_files();
    assert_eq!(kept.len(), 10, "{kept:?}");
    assert!(
        !kept
            .iter()
            .any(|path| path.ends_with("runners-00000000000000000-1.log"))
    );
}

/// An instance with no container ends as soon as it is asked to stop, so its answer
/// must reach `stop` before the process exits.
#[test]
fn stop_on_an_instance_with_no_container_gets_its_answer_and_exits_0() {
    let host = Host::new();
    let stub = Stub::start(vec![Reply::status("500 Internal Server Error", "no")]);
    let started = host.start(&stub);
    host.wait_for_status("backing off", RUNNING_WITHIN);
    let stop = host.client(&["stop"]);
    let (stop_out, stop_err) = texts(&stop);
    assert_eq!(stop.status.code(), Some(0), "{stop_err}");
    assert!(stop_out.contains("The instance has stopped."), "{stop_out}");
    let finished = started.finish(Duration::from_secs(5));
    assert_eq!(finished.status.code(), Some(0), "{}", finished.stderr);
}

#[test]
fn a_start_right_after_a_successful_stop_finds_the_lock_free() {
    let host = Host::new();
    let stub = Stub::start(vec![registered()]);
    let started = running(&host, &stub);
    assert_eq!(host.client(&["stop"]).status.code(), Some(0));
    let invalid = host.root.write("invalid.json", b"{}");
    let next = host
        .command(&["start", "--profile", "axpc", "--config"])
        .arg(&invalid)
        .output()
        .unwrap();
    let (_, stderr) = texts(&next);
    assert!(stderr.contains("Invalid host configuration"), "{stderr}");
    assert_eq!(
        started.finish(Duration::from_secs(5)).status.code(),
        Some(0)
    );
}

#[test]
fn a_second_start_on_a_running_instance_exits_1_naming_its_state_and_runs_nothing() {
    let host = Host::new();
    let stub = Stub::start(vec![registered()]);
    let started = running(&host, &stub);
    let before = host.invocations().len();
    let second = host
        .command(&["start", "--profile", "axpc", "--config"])
        .arg(&host.config)
        .output()
        .unwrap();
    let (_, stderr) = texts(&second);
    assert_eq!(second.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains("the instance is running"), "{stderr}");
    assert_eq!(
        host.invocations().len(),
        before,
        "the fake docker saw no call"
    );
    assert_eq!(host.client(&["stop"]).status.code(), Some(0));
    assert_eq!(
        started.finish(Duration::from_secs(40)).status.code(),
        Some(0)
    );
}

#[test]
fn start_resumes_a_stopped_instance_only_with_its_own_path_and_a_valid_configuration() {
    let host = Host::new();
    let stub = Stub::start(vec![registered(), registered()]);
    let started = running(&host, &stub);
    let valid = fs::read(&host.config).unwrap();
    let other = TempDir::new();
    let other_config = other.write("host.json", &valid);

    fs::write(&host.config, b"{}").unwrap();
    let restart = host.client(&["restart"]);
    assert_eq!(restart.status.code(), Some(1), "{}", texts(&restart).1);
    let status = host.wait_for_status("state: stopped", Duration::from_secs(5));
    assert!(
        status.contains("reason: the configuration is not valid"),
        "{status}"
    );

    let resume = |config: &std::path::Path| {
        host.command(&["start", "--profile", "axpc", "--config"])
            .arg(config)
            .output()
            .unwrap()
    };
    let refused = resume(&other_config);
    assert_eq!(refused.status.code(), Some(1), "{}", texts(&refused).1);
    assert!(texts(&refused).1.contains("another configuration path"));

    let still_invalid = resume(&host.config);
    assert_eq!(
        still_invalid.status.code(),
        Some(1),
        "{}",
        texts(&still_invalid).1
    );
    host.wait_for_status("state: stopped", Duration::from_secs(5));

    fs::write(&host.config, &valid).unwrap();
    let resumed = resume(&host.config);
    assert_eq!(resumed.status.code(), Some(0), "{}", texts(&resumed).1);
    wait_for_job(&host);

    assert_eq!(host.client(&["stop"]).status.code(), Some(0));
    assert_eq!(
        started.finish(Duration::from_secs(40)).status.code(),
        Some(0)
    );
}

/// What `validate` and `start` print when no checkout was found above the file.
const NO_CHECKOUT_NOTE: &str = "no checkout of this project was found";

/// The configuration path `start` is given, under a checkout in the host's root,
/// and the `diagnosticDirectory` the test compares with that checkout. On Unix the
/// path reaches the file through a link out of the checkout, so only a lexical walk
/// finds the checkout; an axpc directory is a Windows path there, so it is never
/// inside the checkout and only the note shows the walk.
#[cfg(unix)]
fn under_a_checkout(host: &Host) -> (std::path::PathBuf, String) {
    fs::create_dir_all(host.path("elsewhere")).unwrap();
    std::os::unix::fs::symlink("../elsewhere", host.path("checkout/config")).unwrap();
    (
        host.path("checkout/config/host.json"),
        r"C:\diag".to_string(),
    )
}

/// On Windows the directory lies inside the checkout, so the configuration is
/// invalid whenever the checkout is found.
#[cfg(windows)]
fn under_a_checkout(host: &Host) -> (std::path::PathBuf, String) {
    let directory = host.path("checkout").join("diag");
    (
        host.path("checkout/host.json"),
        directory.to_str().unwrap().to_string(),
    )
}

#[test]
fn a_restart_judges_the_file_at_the_path_start_was_given_as_validate_does() {
    let host = Host::new();
    host.root
        .write("checkout/.git/HEAD", b"ref: refs/heads/main\n");
    host.root.write("checkout/cli/Cargo.toml", b"[package]\n");
    let (config, directory) = under_a_checkout(&host);
    fs::write(&config, fs::read(&host.config).unwrap()).unwrap();
    host.script("run_mode", "hold");
    let stub = Stub::start(vec![registered(), registered()]);
    let started = host.start_with(&config, &stub);
    wait_for_job(&host);

    fs::write(
        &config,
        support::host::config_with_diagnostics(&host.json_token(), &directory),
    )
    .unwrap();
    let validate = |path: &std::path::Path| {
        host.command(&["validate", "--profile", "axpc", "--config"])
            .arg(path)
            .output()
            .unwrap()
    };
    let fresh = validate(&config);
    #[cfg(unix)]
    {
        assert_eq!(fresh.status.code(), Some(0), "{}", texts(&fresh).1);
        assert!(!texts(&fresh).1.contains(NO_CHECKOUT_NOTE));
        let resolved = validate(&fs::canonicalize(&config).unwrap());
        assert!(
            texts(&resolved).1.contains(NO_CHECKOUT_NOTE),
            "the resolved path has no checkout above it"
        );
    }
    #[cfg(windows)]
    assert!(
        texts(&fresh).1.contains("must be outside the checkout"),
        "{}",
        texts(&fresh).1
    );

    let restart = host.client(&["restart"]);
    assert_eq!(
        restart.status.code(),
        fresh.status.code(),
        "{}",
        texts(&restart).1
    );
    let stop = host.client(&["stop"]);
    assert_eq!(stop.status.code(), Some(0), "{}", texts(&stop).1);
    let finished = started.finish(Duration::from_secs(40));
    assert_eq!(finished.status.code(), Some(0), "{}", finished.stderr);
    let log = fs::read_to_string(&host.log_files()[0]).unwrap();
    assert_eq!(
        log.contains(NO_CHECKOUT_NOTE),
        texts(&fresh).1.contains(NO_CHECKOUT_NOTE),
        "{log}"
    );
}

#[test]
fn stop_ends_within_30_seconds_when_the_containers_ignore_the_grace_period() {
    let host = Host::new();
    let stub = Stub::start(vec![registered()]);
    let started = running(&host, &stub);
    host.script("stop_hangs", "");
    let asked = Instant::now();
    let stop = host.command(&["stop"]).spawn().unwrap();
    host.wait_for_status("state: stopping", Duration::from_secs(5));
    // the instance's own `docker stop` is under way before the count is taken
    let deadline = Instant::now() + Duration::from_secs(5);
    while !host
        .invocations()
        .iter()
        .any(|words| words.first().map(String::as_str) == Some("stop"))
    {
        assert!(Instant::now() < deadline, "docker stop was never run");
        std::thread::sleep(Duration::from_millis(20));
    }

    let before = host.invocations().len();
    let second = host
        .command(&["start", "--profile", "axpc", "--config"])
        .arg(&host.config)
        .output()
        .unwrap();
    let (_, stderr) = texts(&second);
    assert_eq!(second.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains("the instance is stopping"), "{stderr}");
    assert_eq!(
        host.invocations().len(),
        before,
        "the second start ran nothing"
    );

    let finished = started.finish(Duration::from_secs(40));
    let took = asked.elapsed();
    let stop = stop.wait_with_output().unwrap();
    assert_eq!(finished.status.code(), Some(0), "{}", finished.stderr);
    assert_eq!(stop.status.code(), Some(0));
    assert!(took < Duration::from_secs(30), "{took:?}");
    assert!(job_containers(&host).is_empty());
    assert!(
        host.invocations()
            .iter()
            .any(|words| words.first().map(String::as_str) == Some("rm"))
    );
}

#[cfg(unix)]
mod signals {
    use super::*;
    use support::host::send_signal;

    #[test]
    fn sigint_sigterm_and_sighup_each_stop_the_instance_and_remove_its_container() {
        for signal in ["INT", "TERM", "HUP"] {
            let host = Host::new();
            let stub = Stub::start(vec![registered()]);
            let started = running(&host, &stub);
            send_signal(started.pid(), signal);
            let finished = started.finish(Duration::from_secs(30));
            assert_eq!(
                finished.status.code(),
                Some(0),
                "{signal}: {}",
                finished.stderr
            );
            assert!(job_containers(&host).is_empty(), "{signal}");
        }
    }

    #[test]
    fn a_second_sigint_goes_straight_to_forced_removal() {
        let host = Host::new();
        let stub = Stub::start(vec![registered()]);
        let started = running(&host, &stub);
        host.script("stop_hangs", "");
        send_signal(started.pid(), "INT");
        host.wait_for_status("state: stopping", Duration::from_secs(5));
        let forced = Instant::now();
        send_signal(started.pid(), "INT");
        let finished = started.finish(Duration::from_secs(30));
        assert!(
            forced.elapsed() < Duration::from_secs(10),
            "{:?}",
            forced.elapsed()
        );
        assert_eq!(finished.status.code(), Some(0), "{}", finished.stderr);
        assert!(
            finished.stderr.contains("received again"),
            "{}",
            finished.stderr
        );
        assert!(job_containers(&host).is_empty());
    }
}

/// Killing `start` outright makes each shim remove its container. Standard error
/// is shared with the shims, so it closes only when every shim has ended.
#[cfg(unix)]
#[test]
fn a_killed_start_leaves_each_shim_to_remove_its_container() {
    let host = Host::new();
    let stub = Stub::start(vec![registered()]);
    let mut started = running(&host, &stub);
    assert_eq!(job_containers(&host).len(), 1);
    started.kill();
    assert!(
        started.wait_for_stderr_to_close(Duration::from_secs(15)),
        "a shim is still running"
    );
    assert!(job_containers(&host).is_empty(), "{:?}", host.containers());
}

/// Killing `start` outright ends every child in its Job Object, which the test
/// sees through a handle to the job container's client.
#[cfg(windows)]
#[test]
fn a_killed_start_ends_every_child_in_its_job_object() {
    use winsafe::{self as w, co};
    let host = Host::new();
    let stub = Stub::start(vec![registered()]);
    let mut started = running(&host, &stub);
    let client =
        w::HPROCESS::OpenProcess(co::PROCESS::SYNCHRONIZE, false, host.job_client_pid()).unwrap();
    started.kill();
    assert_eq!(
        client.WaitForSingleObject(Some(10_000)).unwrap(),
        co::WAIT::OBJECT_0,
        "the client outlived start"
    );
}
