//! a diagnostics-enabled pool under `start` as real processes, with the token and
//! JIT configuration kept out of all of it.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod support;

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use marshal_cli::diagnostics::exporter::COMPLETE_TEXT;
use support::host::{Host, JIT, Started, TOKEN, registered, texts};
use support::stub::{Reply, Stub};

/// long enough for a fresh instance to prepare its pool and run its container.
const RUNNING_WITHIN: Duration = Duration::from_secs(20);

/// the sink the configuration names, escaped for JSON, and where it is; on Unix an
/// axpc path is a relative name, read from the working directory, which is the
/// host's root.
#[cfg(unix)]
fn sink_path(root: &Path) -> (String, PathBuf) {
    (r"C:\\diag".to_string(), root.join(r"C:\diag"))
}

#[cfg(windows)]
fn sink_path(root: &Path) -> (String, PathBuf) {
    let path = root.join("diag");
    (path.to_str().unwrap().replace('\\', r"\\"), path)
}

/// a host whose pool `r` has diagnostics on, into a sink private to the user.
fn diagnostic_host() -> (Host, PathBuf) {
    let host = Host::new();
    let (json, sink) = sink_path(host.root.path());
    let config = support::host::config_for(&host.json_token()).replace(
        r#""volumes":[]}"#,
        &format!(r#""volumes":[],"diagnostics":true,"diagnosticDirectory":"{json}"}}"#),
    );
    fs::write(&host.config, config).unwrap();
    fs::create_dir(&sink).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&sink, fs::Permissions::from_mode(0o700)).unwrap();
    }
    (host, sink)
}

/// replies for one registration; the next one stalls, so the slot waits there
/// for the stop.
fn one_job() -> Stub {
    Stub::start(vec![registered(), Reply::Stall])
}

fn bundles(sink: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = fs::read_dir(sink)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    found.sort();
    found
}

fn wait_until(what: &str, within: Duration, mut condition: impl FnMut() -> bool) {
    let started = Instant::now();
    while !condition() {
        assert!(started.elapsed() < within, "never: {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// the job containers the fake engine holds, ended or not.
fn job_containers(host: &Host) -> Vec<String> {
    host.containers()
        .into_iter()
        .filter(|name| name.starts_with("r-1-"))
        .collect()
}

fn job_removal_tried(host: &Host) -> bool {
    calls_of(host, "rm")
        .iter()
        .any(|words| words.iter().any(|word| word.starts_with("r-1-")))
}

fn calls_of(host: &Host, command: &str) -> Vec<Vec<String>> {
    host.invocations()
        .into_iter()
        .filter(|words| words.first().map(String::as_str) == Some(command))
        .collect()
}

fn d_token(host: &Host) -> PathBuf {
    let token = host.json_token().replace(r"\\", r"\");
    host.path(&token.replace("r.token", "d.token"))
}

fn stop(host: &Host, started: Started) -> support::host::Finished {
    let stop = host.client(&["stop"]);
    assert_eq!(stop.status.code(), Some(0), "{}", texts(&stop).1);
    started.finish(Duration::from_secs(40))
}

fn log_text(host: &Host) -> String {
    host.log_files()
        .iter()
        .map(|file| fs::read_to_string(file).unwrap())
        .collect()
}

#[test]
fn a_finished_diagnostic_job_leaves_a_complete_bundle_and_no_container() {
    let (host, sink) = diagnostic_host();
    let stub = one_job();
    let started = host.start(&stub);
    wait_until("the bundle is complete", RUNNING_WITHIN, || {
        bundles(&sink)
            .first()
            .is_some_and(|bundle| bundle.join("complete.txt").exists())
    });
    wait_until("the container is removed", RUNNING_WITHIN, || {
        job_containers(&host).is_empty()
    });
    let bundle = &bundles(&sink)[0];
    assert_eq!(
        support::files(bundle),
        ["complete.txt", "identity.txt", "metrics.txt"]
    );
    assert_eq!(
        fs::read_to_string(bundle.join("complete.txt")).unwrap(),
        COMPLETE_TEXT
    );
    let runs = calls_of(&host, "run");
    let job = runs
        .iter()
        .find(|words| {
            words
                .iter()
                .any(|word| word == "runners.diagnostic-lifecycle=1")
        })
        .expect("the job ran the diagnostic variant");
    assert!(!job.iter().any(|word| word == "--rm"));
    let finished = stop(&host, started);
    assert_eq!(finished.status.code(), Some(0), "{}", finished.stderr);
    assert!(
        !finished.stderr.contains("Diagnostic gap"),
        "{}",
        finished.stderr
    );
}

#[test]
fn an_export_gap_is_a_warning_and_the_container_is_still_removed() {
    let (host, sink) = diagnostic_host();
    host.script("cp_mode", "gap");
    let stub = one_job();
    let started = host.start(&stub);
    wait_until("the container is finalised", RUNNING_WITHIN, || {
        job_removal_tried(&host) && job_containers(&host).is_empty()
    });
    let finished = stop(&host, started);
    assert_eq!(
        finished.status.code(),
        Some(0),
        "an export gap is not a failure"
    );
    assert!(
        finished
            .stderr
            .contains(": final observation unavailable (abrupt exit or observation limit)."),
        "{}",
        finished.stderr
    );
    assert_eq!(bundles(&sink).len(), 1);
}

/// neither the token nor the JIT configuration, which the job prints, reaches a
/// bundle, the terminal, the log file or an answer.
#[test]
fn the_token_and_jit_configuration_reach_no_bundle_terminal_log_or_answer() {
    let (host, sink) = diagnostic_host();
    host.script("run_prints_jit", "");
    let stub = one_job();
    let started = host.start(&stub);
    wait_until("the container is finalised", RUNNING_WITHIN, || {
        !bundles(&sink).is_empty() && job_containers(&host).is_empty()
    });
    let mut seen = Vec::new();
    for args in [&["diagnostics"][..], &["status"]] {
        let output = host.client(args);
        let (out, err) = texts(&output);
        seen.push(out);
        seen.push(err);
    }
    let finished = stop(&host, started);
    seen.push(finished.stdout);
    seen.push(finished.stderr);
    seen.push(log_text(&host));
    for bundle in bundles(&sink) {
        for file in support::files(&bundle) {
            seen.push(String::from_utf8_lossy(&fs::read(bundle.join(file)).unwrap()).into_owned());
        }
    }
    for (index, text) in seen.iter().enumerate() {
        assert!(
            !text.contains(TOKEN) && !text.contains(JIT),
            "text {index} holds a secret:\n{text}"
        );
    }
    let calls = host.invocations();
    assert!(!calls.is_empty());
    for (number, words) in calls.iter().enumerate() {
        let jit = host.docker().join(format!("inv-{number:06}.jit")).exists();
        assert_eq!(
            jit,
            words[0] == "run" && words.iter().any(|word| word == "-e"),
            "{words:?}"
        );
    }
}

#[test]
fn diagnostics_without_an_instance_exits_1() {
    let host = Host::new();
    let output = host.client(&["diagnostics"]);
    let (_, stderr) = texts(&output);
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr.contains("No instance is running."), "{stderr}");
}

/// when the job end exports a container but cannot remove it, `diagnostics` fails
/// while the removal does, then removes it and exits 0 with the pool's counts.
#[test]
fn diagnostics_removes_a_container_whose_removal_failed_and_prints_the_counts() {
    let (host, sink) = diagnostic_host();
    host.script("rm_fails", "");
    let stub = one_job();
    let started = host.start(&stub);
    wait_until("the job end's removal fails", RUNNING_WITHIN, || {
        log_text(&host).contains("it stays retained, and the next finalisation removes it")
    });
    assert!(job_removal_tried(&host) && !bundles(&sink).is_empty());
    let name = job_containers(&host).remove(0);

    let again = host.client(&["diagnostics"]);
    let (out, err) = texts(&again);
    assert_eq!(again.status.code(), Some(1), "{out}{err}");
    assert_eq!(
        out,
        format!(
            "pool r: 0 bundles written, 9 of 10 remaining\n\
             container {name}: removal failed; it stays retained\n"
        )
    );

    fs::remove_file(host.docker().join("rm_fails")).unwrap();
    let removed = host.client(&["diagnostics"]);
    let (out, err) = texts(&removed);
    assert_eq!(removed.status.code(), Some(0), "{out}{err}");
    assert_eq!(out, "pool r: 0 bundles written, 9 of 10 remaining\n");
    assert!(job_containers(&host).is_empty());
    assert_eq!(bundles(&sink).len(), 1, "no second bundle");

    let finished = stop(&host, started);
    assert_eq!(finished.status.code(), Some(0), "{}", finished.stderr);
}

/// pool `r` has diagnostics off and a labelled container an earlier run left, so it
/// is refused and the container is left to the manual recovery; pool `d` runs, but
/// its sink is gone, so its count is unknown.
#[test]
fn diagnostics_fails_for_a_container_left_to_the_manual_recovery_and_shows_an_unknown_count() {
    let host = Host::new();
    let (json, sink) = sink_path(host.root.path());
    fs::write(
        &host.config,
        support::host::config_with_diagnostics(&host.json_token(), &json.replace(r"\\", r"\")),
    )
    .unwrap();
    support::write_token_file(&d_token(&host), format!("{TOKEN}\n").as_bytes());
    let left = "r-1-20240101000000000";
    host.script(
        "containers",
        &format!("{left}\trunners.diagnostic-lifecycle=1\texited"),
    );
    let stub = Stub::start(vec![registered(), Reply::Stall]);
    let started = host.start(&stub);
    host.wait_for_status("pool: d: running", RUNNING_WITHIN);
    assert!(!sink.exists());

    let output = host.client(&["diagnostics"]);
    let (out, err) = texts(&output);
    assert_eq!(output.status.code(), Some(1), "{out}{err}");
    assert!(
        out.contains("pool d: 0 bundles written, unknown of 10 remaining\n"),
        "{out}"
    );
    assert!(
        out.contains(&format!(
            "container {left}: left to the manual recovery, because pool r has diagnostics off\n"
        )),
        "{out}"
    );
    assert_eq!(host.containers().first().map(String::as_str), Some(left));
    let finished = stop(&host, started);
    assert!(
        finished
            .stderr
            .contains("Recover before disabling diagnostics"),
        "{}",
        finished.stderr
    );
}

#[cfg(unix)]
mod signals {
    use super::*;
    use support::host::{send_group_signal, send_signal};

    /// a Ctrl+C in the terminal reaches the whole foreground group, but the exporter
    /// child runs in a group of its own, so its export goes on while the instance
    /// stops and the bundle is complete.
    #[test]
    fn a_first_sigint_during_a_job_end_export_still_yields_a_complete_bundle() {
        let (host, sink) = diagnostic_host();
        host.script("cp_mode", "slow");
        let stub = one_job();
        let started = host.start_as_group_leader(&stub);
        wait_until("the copy runs", RUNNING_WITHIN, || {
            !calls_of(&host, "cp").is_empty()
        });
        send_group_signal(started.pid(), "INT");
        let finished = started.finish(Duration::from_secs(30));
        assert_eq!(finished.status.code(), Some(0), "{}", finished.stderr);
        let found = bundles(&sink);
        assert_eq!(found.len(), 1);
        assert_eq!(
            support::files(&found[0]),
            ["complete.txt", "identity.txt", "metrics.txt"]
        );
        assert!(job_containers(&host).is_empty());
    }

    /// a second SIGINT keeps the running diagnostic container, ended; `start`
    /// exits 1 naming it, and the next `start` exports, removes and reports it.
    #[test]
    fn a_second_sigint_ends_and_keeps_the_container_for_the_next_start() {
        let (host, sink) = diagnostic_host();
        host.script("run_mode", "hold");
        host.script("stop_hangs", "");
        let stub = one_job();
        let started = host.start(&stub);
        host.wait_for_status("running container r-1-", RUNNING_WITHIN);
        wait_until("the job container runs", RUNNING_WITHIN, || {
            !job_containers(&host).is_empty()
        });
        let name = job_containers(&host).remove(0);
        send_signal(started.pid(), "INT");
        wait_until("the stop runs docker stop", Duration::from_secs(5), || {
            !calls_of(&host, "stop").is_empty()
        });
        send_signal(started.pid(), "INT");
        let finished = started.finish(Duration::from_secs(30));
        assert_eq!(finished.status.code(), Some(1), "{}", finished.stderr);
        let last = finished
            .stderr
            .lines()
            .last()
            .unwrap_or_default()
            .to_string();
        assert!(last.contains(&name), "{last}");
        assert_eq!(job_containers(&host), std::slice::from_ref(&name), "kept");
        assert!(
            calls_of(&host, "kill")
                .iter()
                .any(|words| words.contains(&name))
        );
        assert!(bundles(&sink).is_empty());

        next_start_finalises(&host, &sink, &name);
    }

    /// killing `start` outright closes the shim's pipe, and in its diagnostic mode
    /// the shim ends the container with `docker kill` and keeps it. standard error is
    /// shared with the shims, so it closes only when every shim has ended.
    #[test]
    fn a_killed_start_leaves_the_diagnostic_container_ended_for_the_next_start() {
        let (host, sink) = diagnostic_host();
        host.script("run_mode", "hold");
        let stub = one_job();
        let mut started = host.start(&stub);
        wait_until("the job container runs", RUNNING_WITHIN, || {
            !job_containers(&host).is_empty()
        });
        let name = job_containers(&host).remove(0);
        started.kill();
        assert!(
            started.wait_for_stderr_to_close(Duration::from_secs(15)),
            "a shim is still running"
        );
        assert_eq!(job_containers(&host), std::slice::from_ref(&name), "kept");
        assert_eq!(
            calls_of(&host, "kill").last().unwrap(),
            &["kill".to_string(), name.clone()]
        );
        assert!(
            calls_of(&host, "rm")
                .iter()
                .all(|words| !words.contains(&name))
        );
        drop(started);

        next_start_finalises(&host, &sink, &name);
    }

    fn next_start_finalises(host: &Host, sink: &Path, name: &str) {
        for flag in ["run_mode", "stop_hangs"] {
            let _ = fs::remove_file(host.docker().join(flag));
        }
        let stub = Stub::start(vec![Reply::Stall]);
        let started = host.start(&stub);
        let reported =
            format!("Diagnostic container {name} that an earlier run left: exported, removed.");
        wait_until("the kept container is reported", RUNNING_WITHIN, || {
            log_text(host).contains(&reported)
        });
        assert!(job_containers(host).is_empty());
        let finished = stop(host, started);
        assert_eq!(finished.status.code(), Some(0), "{}", finished.stderr);
        let found = bundles(sink);
        assert_eq!(found.len(), 1);
        assert!(found[0].join("complete.txt").exists());
    }
}

/// the exporter ends through its liveness pipe on Unix and the Job Object on Windows;
/// standard error, which it shares, closes only when it has ended.
#[test]
fn an_exporter_whose_instance_is_killed_ends_with_its_clients() {
    let (host, _sink) = diagnostic_host();
    host.script("cp_mode", "hang");
    let stub = one_job();
    let mut started = host.start(&stub);
    wait_until("the copy runs", RUNNING_WITHIN, || {
        !calls_of(&host, "cp").is_empty()
    });
    let copy = host
        .invocations()
        .iter()
        .position(|words| words[0] == "cp")
        .unwrap();
    let alive = host.docker().join(format!("inv-{copy:06}.alive"));
    started.kill();
    assert!(
        started.wait_for_stderr_to_close(Duration::from_secs(5)),
        "the exporter outlived its instance"
    );
    let (ended, wait) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        fs::File::open(alive).unwrap().lock().unwrap();
        let _ = ended.send(());
    });
    assert!(
        wait.recv_timeout(Duration::from_secs(2)).is_ok(),
        "the copy client outlived the exporter"
    );
}

/// a stop with an ordinary and a diagnostic container whose `docker stop` ignores
/// the grace period ends within 30 s with both removed and the bundle written.
#[test]
fn a_stop_with_diagnostic_and_ordinary_containers_ends_within_30_seconds() {
    let host = Host::new();
    let (json, sink) = sink_path(host.root.path());
    fs::write(
        &host.config,
        support::host::config_with_diagnostics(&host.json_token(), &json.replace(r"\\", r"\")),
    )
    .unwrap();
    fs::create_dir(&sink).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&sink, fs::Permissions::from_mode(0o700)).unwrap();
    }
    support::write_token_file(&d_token(&host), format!("{TOKEN}\n").as_bytes());
    host.script("run_mode", "hold");
    let stub = Stub::start(vec![registered(), registered(), Reply::Stall, Reply::Stall]);
    let started = host.start(&stub);
    wait_until("both containers run", RUNNING_WITHIN, || {
        host.containers().len() == 2
    });
    host.script("stop_hangs", "");
    let asked = Instant::now();
    let stop = host.client(&["stop"]);
    assert_eq!(stop.status.code(), Some(0), "{}", texts(&stop).1);
    let finished = started.finish(Duration::from_secs(40));
    let took = asked.elapsed();
    assert!(took < Duration::from_secs(30), "{took:?}");
    assert_eq!(finished.status.code(), Some(0), "{}", finished.stderr);
    assert!(host.containers().is_empty(), "{:?}", host.containers());
    let found = bundles(&sink);
    assert_eq!(found.len(), 1);
    assert!(found[0].join("complete.txt").exists());
    let stops = calls_of(&host, "stop");
    assert!(stops.iter().any(|words| words[2] == "10"), "{stops:?}");
    assert!(stops.iter().any(|words| words[2] == "15"), "{stops:?}");
}
