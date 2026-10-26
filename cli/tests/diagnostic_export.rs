//! Finalising a diagnostic container through the container backend: the exporter
//! child, this CLI run again, against the fake `docker`, writing into a real sink.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod support;

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use marshal_cli::diagnostics::ExportRequest;
use marshal_cli::diagnostics::exporter::COMPLETE_TEXT;
use marshal_cli::diagnostics::identity::IDENTITY_HEADER;
use marshal_cli::runtime::backend::{
    Backend, Deadlines, DiagnosticRecord, ExportOutcome, Finalized,
};
use marshal_cli::runtime::diagnostic;
use support::docker::{FAKE_DOCKER, Fake, Invocation};
use support::{BINARY, TempDir};
use tokio::time::Instant;

const NAME: &str = "d-1-20240305060708009";
const OTHER: &str = "d-2-20240305060708009";
const LABEL: &str = "runners.diagnostic-lifecycle=1";

/// The identity the fake `docker` reads back.
fn readback() -> String {
    format!(
        "sha256:{}|1500000000|17179869184|17179869184|150000|100000|0-3|false|0",
        "0".repeat(64)
    )
}

/// Diagnostic containers the fake engine holds, ended and kept.
fn kept(fake: &Fake, names: &[&str]) {
    let lines: Vec<String> = names
        .iter()
        .map(|name| format!("{name}\t{LABEL}\texited"))
        .collect();
    fake.script("containers", &lines.join("\n"));
}

/// A sink private to the user, named without links, as the checks require.
fn private_sink(scratch: &TempDir) -> PathBuf {
    let sink = scratch.path().join("sink");
    fs::create_dir(&sink).unwrap();
    set_mode(&sink, 0o700);
    fs::canonicalize(sink).unwrap()
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: u32) {}

#[cfg(unix)]
fn mode(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path).unwrap().permissions().mode() & 0o777
}

fn record(sink: &Path, raw_records: bool) -> DiagnosticRecord {
    DiagnosticRecord {
        pool: "d".to_string(),
        directory: sink.to_str().unwrap().to_string(),
        raw_records,
    }
}

/// Finalises `name` on a clock that starts now, as the job end does.
async fn finalize(fake: &Fake, record: &DiagnosticRecord, name: &str) -> Finalized {
    let retained = fake.backend.retained();
    if !retained.contains(name) {
        retained.adopt(name, record);
    }
    let claim = retained.claim(name).unwrap();
    diagnostic::finalize(&fake.backend, &claim, Deadlines::from(Instant::now())).await
}

/// The pool's bundles in the sink, sorted.
fn bundles(sink: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = fs::read_dir(sink)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("d-bundle-")
        })
        .collect();
    found.sort();
    found
}

fn calls_of<'a>(invocations: &'a [Invocation], command: &str) -> Vec<&'a Invocation> {
    invocations
        .iter()
        .filter(|call| call.arguments.first().map(String::as_str) == Some(command))
        .collect()
}

/// The number of the first call to the fake `docker` that ran `command`.
fn call_number(fake: &Fake, command: &str) -> usize {
    fake.invocations()
        .iter()
        .position(|call| call.arguments.first().map(String::as_str) == Some(command))
        .unwrap_or_else(|| panic!("docker {command} was never run"))
}

/// Whether call `number` of the fake `docker` ends within `within`, which the
/// lock it holds while it lives shows.
fn ended_within(fake: &Fake, number: usize, within: Duration) -> bool {
    let alive = fake.dir.path().join(format!("inv-{number:06}.alive"));
    let (ended, wait) = mpsc::channel();
    std::thread::spawn(move || {
        let file = fs::File::open(alive).unwrap();
        file.lock().unwrap();
        let _ = ended.send(());
    });
    wait.recv_timeout(within).is_ok()
}

fn complete(bundles: u32) -> Finalized {
    Finalized {
        export: ExportOutcome::Complete,
        removed: true,
        kept: false,
        bundles: Some(bundles),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn an_ended_container_yields_one_complete_bundle_and_is_removed() {
    let fake = Fake::exporting();
    let scratch = TempDir::new();
    let sink = private_sink(&scratch);
    kept(&fake, &[NAME]);
    let finalized = finalize(&fake, &record(&sink, false), NAME).await;
    assert_eq!(finalized, complete(1));
    assert!(fake.containers().is_empty(), "the container was removed");
    assert!(!fake.backend.retained().contains(NAME));

    let found = bundles(&sink);
    assert_eq!(found.len(), 1);
    let bundle = &found[0];
    let suffix = bundle
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .strip_prefix("d-bundle-")
        .unwrap()
        .to_string();
    assert!(
        suffix.len() == 32 && suffix.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "{suffix}"
    );
    assert_eq!(
        support::files(bundle),
        ["complete.txt", "identity.txt", "metrics.txt"]
    );
    assert_eq!(
        fs::read_to_string(bundle.join("identity.txt")).unwrap(),
        format!("{IDENTITY_HEADER}\n{}\n", readback())
    );
    assert_eq!(
        fs::read_to_string(bundle.join("metrics.txt")).unwrap(),
        "sample=1\nfinal=observed\n"
    );
    assert_eq!(
        fs::read_to_string(bundle.join("complete.txt")).unwrap(),
        COMPLETE_TEXT
    );
    #[cfg(unix)]
    {
        assert_eq!(mode(bundle), 0o700);
        for file in support::files(bundle) {
            assert_eq!(mode(&bundle.join(&file)), 0o600, "{file}");
        }
    }

    let invocations = fake.invocations();
    let inspect = calls_of(&invocations, "inspect");
    assert_eq!(inspect.len(), 1);
    assert_eq!(inspect[0].arguments[3], NAME);
    let copy = calls_of(&invocations, "cp");
    assert_eq!(
        copy[0].arguments,
        ["cp", &format!("{NAME}:/tmp/runner-diagnostics/."), "-"]
    );
    assert_eq!(
        calls_of(&invocations, "rm").last().unwrap().arguments,
        ["rm", "-f", NAME]
    );
    // the exporter's clients carry no registration and run in groups of their own
    for call in inspect.iter().chain(&copy) {
        assert_eq!(call.jit, None);
        #[cfg(unix)]
        assert_eq!(call.pgid, Some(call.pid), "{:?}", call.arguments);
    }
    assert!(fake.log.lines().is_empty(), "{:?}", fake.log.lines());
}

#[tokio::test(flavor = "current_thread")]
async fn raw_records_are_written_only_when_the_record_turns_them_on() {
    let fake = Fake::exporting();
    let scratch = TempDir::new();
    let sink = private_sink(&scratch);
    fake.script("cp_mode", "raw");
    kept(&fake, &[NAME]);
    assert_eq!(
        finalize(&fake, &record(&sink, true), NAME).await,
        complete(1)
    );
    let first = bundles(&sink).remove(0);
    assert_eq!(
        support::files(&first),
        ["complete.txt", "identity.txt", "metrics.txt", "raw-1.log"]
    );
    assert_eq!(
        fs::read_to_string(first.join("raw-1.log")).unwrap(),
        "raw record\n"
    );

    // with raw records off the same archive is refused, and the container still goes
    kept(&fake, &[OTHER]);
    let finalized = finalize(&fake, &record(&sink, false), OTHER).await;
    assert_eq!(finalized.export, ExportOutcome::Failed);
    assert!(finalized.removed);
    let partial = bundles(&sink)
        .into_iter()
        .find(|bundle| bundle != &first)
        .unwrap();
    assert_eq!(support::files(&partial), ["identity.txt", "metrics.txt"]);
}

#[tokio::test(flavor = "current_thread")]
async fn a_gap_is_a_complete_bundle_without_the_final_observation() {
    let fake = Fake::exporting();
    let scratch = TempDir::new();
    let sink = private_sink(&scratch);
    fake.script("cp_mode", "gap");
    kept(&fake, &[NAME]);
    let finalized = finalize(&fake, &record(&sink, false), NAME).await;
    assert_eq!(
        finalized,
        Finalized {
            export: ExportOutcome::Gap,
            ..complete(1)
        }
    );
    let bundle = &bundles(&sink)[0];
    assert_eq!(
        support::files(bundle),
        ["complete.txt", "identity.txt", "metrics.txt"]
    );
    assert_eq!(
        fs::read_to_string(bundle.join("metrics.txt")).unwrap(),
        "sample=1\n"
    );
}

/// A failed copy, a refused archive, a full sink, a missing one and, on Unix, one
/// that is not private: each export fails or is refused, and the container is
/// removed all the same.
#[tokio::test(flavor = "current_thread")]
async fn the_container_is_removed_whatever_the_export_does() {
    for (case, expected) in [
        ("cp_mode=fail", ExportOutcome::Failed),
        ("cp_mode=bad", ExportOutcome::Failed),
        ("full", ExportOutcome::SinkFull),
        ("missing", ExportOutcome::Failed),
        #[cfg(unix)]
        ("not private", ExportOutcome::Failed),
    ] {
        let fake = Fake::exporting();
        let scratch = TempDir::new();
        let mut sink = private_sink(&scratch);
        match case {
            "cp_mode=fail" => fake.script("cp_mode", "fail"),
            "cp_mode=bad" => fake.script("cp_mode", "bad"),
            "full" => {
                for index in 1..=10 {
                    fs::create_dir(sink.join(format!("d-bundle-partial-{index}"))).unwrap();
                }
            }
            "missing" => sink = sink.join("missing"),
            _ => set_mode(&sink, 0o755),
        }
        kept(&fake, &[NAME]);
        let finalized = finalize(&fake, &record(&sink, false), NAME).await;
        assert_eq!(finalized.export, expected, "{case}");
        assert!(finalized.removed, "{case}");
        assert!(fake.containers().is_empty(), "{case}");
        match case {
            "full" => {
                assert_eq!(finalized.bundles, Some(10));
                assert_eq!(bundles(&sink).len(), 10, "no eleventh bundle");
                assert!(calls_of(&fake.invocations(), "cp").is_empty());
            }
            "missing" => assert!(!sink.exists(), "a missing sink is not created"),
            "not private" => assert!(fs::read_dir(&sink).unwrap().next().is_none()),
            _ => {
                let found = bundles(&sink);
                assert_eq!(found.len(), 1, "{case}: the partial bundle stays");
                assert!(
                    !support::files(&found[0]).contains(&"complete.txt".to_string()),
                    "{case}"
                );
            }
        }
    }
}

/// The PowerShell test's identity cases: the allowlisted readback, fragmented
/// output, and an unfiltered value, which is refused without being repeated;
/// output of a kibibyte or more and a failed inspect are refused too.
#[tokio::test(flavor = "current_thread")]
async fn the_identity_readback_gives_the_powershell_outcomes() {
    let marker = "4f0c3a51b8e24d6f9a7e2c1d0b9a8f7e";
    for (mode, accepted) in [
        ("fragmented", true),
        ("unfiltered", false),
        ("long", false),
        ("fail", false),
    ] {
        let fake = Fake::exporting();
        let scratch = TempDir::new();
        let sink = private_sink(&scratch);
        fake.script("identity_mode", mode);
        kept(&fake, &[NAME]);
        let finalized = finalize(&fake, &record(&sink, false), NAME).await;
        let bundle = &bundles(&sink)[0];
        if accepted {
            assert_eq!(finalized, complete(1), "{mode}");
            assert_eq!(
                fs::read_to_string(bundle.join("identity.txt")).unwrap(),
                format!("{IDENTITY_HEADER}\n{}\n", readback()),
                "{mode}"
            );
        } else {
            assert_eq!(finalized.export, ExportOutcome::Failed, "{mode}");
            assert!(finalized.removed, "{mode}");
            assert!(
                support::files(bundle).is_empty(),
                "{mode}: {:?}",
                support::files(bundle)
            );
            assert!(
                calls_of(&fake.invocations(), "cp").is_empty(),
                "{mode}: nothing is copied without an identity"
            );
        }
        assert!(
            fake.log.lines().iter().all(|line| !line.contains(marker)),
            "{mode}"
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_failed_removal_is_retried_by_the_next_finalisation_without_a_second_bundle() {
    let fake = Fake::exporting();
    let scratch = TempDir::new();
    let sink = private_sink(&scratch);
    let record = record(&sink, false);
    fake.script("rm_fails", "");
    kept(&fake, &[NAME]);
    let first = finalize(&fake, &record, NAME).await;
    assert_eq!(
        first,
        Finalized {
            removed: false,
            ..complete(1)
        }
    );
    assert!(fake.backend.retained().contains(NAME));
    assert_eq!(fake.containers(), [NAME]);

    fs::remove_file(fake.dir.path().join("rm_fails")).unwrap();
    let second = finalize(&fake, &record, NAME).await;
    assert_eq!(
        second,
        Finalized {
            export: ExportOutcome::AlreadyExported,
            removed: true,
            kept: false,
            bundles: None,
        }
    );
    assert!(fake.containers().is_empty());
    assert!(!fake.backend.retained().contains(NAME));
    assert_eq!(bundles(&sink).len(), 1, "no second bundle");
    let invocations = fake.invocations();
    assert_eq!(calls_of(&invocations, "cp").len(), 1);
    assert_eq!(calls_of(&invocations, "rm").len(), 2);
}

/// Two slots' containers export at once into a sink that holds nine of the pool's
/// bundles: the admission lock lets one take the tenth, and the other is refused.
#[tokio::test(flavor = "current_thread")]
async fn an_eleventh_bundle_is_refused_with_two_slots_exporting_at_once() {
    let fake = Fake::exporting();
    let scratch = TempDir::new();
    let sink = private_sink(&scratch);
    for index in 1..=9 {
        fs::create_dir(sink.join(format!("d-bundle-partial-{index}"))).unwrap();
    }
    // another pool's bundles do not count
    fs::create_dir(sink.join("other-bundle-x")).unwrap();
    let record = record(&sink, false);
    kept(&fake, &[NAME, OTHER]);
    let retained = fake.backend.retained();
    retained.adopt(NAME, &record);
    retained.adopt(OTHER, &record);
    let (first, second) = (
        retained.claim(NAME).unwrap(),
        retained.claim(OTHER).unwrap(),
    );
    let deadlines = Deadlines::from(Instant::now());
    let (one, two) = tokio::join!(
        diagnostic::finalize(&fake.backend, &first, deadlines),
        diagnostic::finalize(&fake.backend, &second, deadlines)
    );
    let mut outcomes = [one.export, two.export];
    outcomes.sort_by_key(|outcome| format!("{outcome:?}"));
    assert_eq!(outcomes, [ExportOutcome::Complete, ExportOutcome::SinkFull]);
    assert!(one.removed && two.removed);
    assert_eq!(one.bundles, Some(10));
    assert_eq!(two.bundles, Some(10));
    assert_eq!(bundles(&sink).len(), 10);
    assert!(fake.containers().is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn a_count_reads_the_pools_bundles_or_gives_none() {
    let fake = Fake::exporting();
    let scratch = TempDir::new();
    let sink = private_sink(&scratch);
    for index in 1..=3 {
        fs::create_dir(sink.join(format!("d-bundle-{index}"))).unwrap();
    }
    let deadline = Instant::now() + Duration::from_secs(24);
    assert_eq!(
        fake.backend
            .count_bundles(&record(&sink, false), deadline)
            .await,
        Some(3)
    );
    assert_eq!(bundles(&sink).len(), 3, "a count writes nothing");
    assert_eq!(
        fake.backend
            .count_bundles(&record(&sink.join("missing"), false), deadline)
            .await,
        None
    );
    let none = Fake::new();
    assert_eq!(
        none.backend
            .count_bundles(&record(&sink, false), deadline)
            .await,
        None,
        "without an exporter nothing is counted"
    );
}

/// A copy that never ends: the export ends at 18 s, its client with it, and the
/// removal follows at once.
#[tokio::test(flavor = "current_thread")]
async fn a_stalled_copy_ends_the_export_by_18_seconds_and_its_client_with_it() {
    let fake = Fake::exporting();
    let scratch = TempDir::new();
    let sink = private_sink(&scratch);
    fake.script("cp_mode", "hang");
    kept(&fake, &[NAME]);
    let started = Instant::now();
    let finalized = finalize(&fake, &record(&sink, false), NAME).await;
    let took = started.elapsed();
    assert_eq!(finalized.export, ExportOutcome::Failed);
    assert!(finalized.removed);
    assert!(
        took >= Duration::from_secs(18) && took < Duration::from_millis(19_500),
        "{took:?}"
    );
    assert!(
        ended_within(&fake, call_number(&fake, "cp"), Duration::from_secs(2)),
        "the copy client outlived its export"
    );
    let bundle = &bundles(&sink)[0];
    assert!(!support::files(bundle).contains(&"complete.txt".to_string()));
}

/// A copy and a removal that never end: the finalisation ends by 24 s, and every
/// client it started has ended.
#[tokio::test(flavor = "current_thread")]
async fn a_stalled_copy_and_removal_end_the_finalisation_by_24_seconds() {
    let fake = Fake::exporting();
    let scratch = TempDir::new();
    let sink = private_sink(&scratch);
    fake.script("cp_mode", "hang");
    fake.script("rm_hangs", "");
    kept(&fake, &[NAME]);
    let started = Instant::now();
    let finalized = finalize(&fake, &record(&sink, false), NAME).await;
    let took = started.elapsed();
    assert_eq!(finalized.export, ExportOutcome::Failed);
    assert!(!finalized.removed);
    assert!(
        took >= Duration::from_secs(24) && took < Duration::from_millis(25_500),
        "{took:?}"
    );
    assert!(fake.backend.retained().contains(NAME));
    for command in ["cp", "rm"] {
        assert!(
            ended_within(&fake, call_number(&fake, command), Duration::from_secs(2)),
            "docker {command} outlived its budget"
        );
    }
}

/// An exporter that ignores its liveness pipe: the pipe closes at 18 s, the child
/// is killed a second later, and its end is seen through the lock it held.
#[tokio::test(flavor = "current_thread")]
async fn an_exporter_that_overruns_is_killed_a_second_after_its_pipe_closes() {
    let fake = Fake::exporting_with(FAKE_DOCKER);
    let scratch = TempDir::new();
    let sink = private_sink(&scratch);
    kept(&fake, &[NAME]);
    let started = Instant::now();
    let finalized = finalize(&fake, &record(&sink, false), NAME).await;
    let took = started.elapsed();
    assert_eq!(finalized.export, ExportOutcome::Failed);
    assert!(finalized.removed);
    assert!(
        took >= Duration::from_secs(19) && took < Duration::from_millis(20_500),
        "{took:?}"
    );
    let exporter = call_number(&fake, "export");
    assert!(
        ended_within(&fake, exporter, Duration::from_millis(100)),
        "the exporter outlived its kill"
    );
    let call = &fake.invocations()[exporter];
    assert!(
        call.arguments
            .windows(2)
            .any(|pair| pair == ["--container", NAME])
    );
    #[cfg(unix)]
    assert_eq!(
        call.pgid,
        Some(call.pid),
        "the exporter has a group of its own"
    );
}

/// The exporter child itself: when its standard input closes, as it does when its
/// instance ends however it ends, it kills its client and exits.
#[test]
fn an_exporter_whose_pipe_closes_ends_with_its_clients() {
    let fake = Fake::new();
    let scratch = TempDir::new();
    let sink = private_sink(&scratch);
    fake.script("cp_mode", "hang");
    kept(&fake, &[NAME]);
    let request = ExportRequest {
        pool: "d".to_string(),
        container: Some(NAME.to_string()),
        directory: sink.to_str().unwrap().to_string(),
        raw_records: false,
        checkout: None,
        deadline: Duration::from_secs(60),
    };
    let mut child = Command::new(BINARY)
        .args(request.arguments(FAKE_DOCKER.as_ref()))
        .env("FAKE_DOCKER_DIR", fake.dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    assert_eq!(lines.next().unwrap().unwrap(), "admitted");
    assert_eq!(lines.next().unwrap().unwrap(), "bundles 1");
    // the copy client starts once the identity is written
    let started = std::time::Instant::now();
    while calls_of(&fake.invocations(), "cp").is_empty() {
        assert!(started.elapsed() < Duration::from_secs(10), "no copy ran");
        std::thread::sleep(Duration::from_millis(20));
    }
    let copy = call_number(&fake, "cp");

    drop(child.stdin.take());
    let (ended, wait) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = ended.send(child.wait_with_output().unwrap());
    });
    let output = wait
        .recv_timeout(Duration::from_secs(3))
        .expect("the exporter outlived its pipe");
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stderr.is_empty(), "{:?}", output.stderr);
    assert!(
        ended_within(&fake, copy, Duration::from_secs(2)),
        "the copy client outlived the exporter"
    );
}

/// Pool `d`, with diagnostics on into `sink`, and its token in `scratch`.
fn diagnostic_pool(scratch: &TempDir, sink: &Path) -> marshal_cli::runtime::pool::PoolSpec {
    let config = r#"{"imageName":"actions-runner:local","repositories":[{"owner":"o",
        "repository":"d","name":"d","slots":1,"tokenPath":"C:\\d.token","volumes":[],
        "diagnostics":true,"diagnosticDirectory":"C:\\evidence"}]}"#;
    let plan = marshal_cli::validate::validate(
        config.as_bytes(),
        marshal_cli::profile::Profile::Axpc,
        None,
    )
    .outcome
    .unwrap();
    let mut pool = marshal_cli::runtime::pool::pools_from_plan(&plan)
        .unwrap()
        .remove(0);
    pool.token_path = scratch.write_token("d.token", b"ghp_diagnostictoken\n");
    pool.diagnostic = Some(record(sink, false));
    pool
}

#[tokio::test(flavor = "current_thread")]
async fn at_startup_a_labelled_container_of_a_diagnostics_pool_is_stopped_exported_and_removed() {
    let fake = Fake::exporting();
    let scratch = TempDir::new();
    let sink = private_sink(&scratch);
    fake.script("containers", &format!("{NAME}\t{LABEL}\t"));
    let pool = diagnostic_pool(&scratch, &sink);
    let prepared = support::prepare_pools(
        &fake.backend,
        vec![pool],
        &*fake.log,
        &marshal_cli::runtime::board::Board::default(),
    )
    .await
    .unwrap();
    assert_eq!(prepared.len(), 1, "the pool starts");
    assert!(fake.containers().is_empty());
    assert_eq!(support::files(&bundles(&sink)[0]).len(), 3);
    let invocations = fake.invocations();
    let commands: Vec<&str> = invocations
        .iter()
        .map(|call| call.arguments[0].as_str())
        .filter(|command| ["stop", "inspect", "cp", "rm"].contains(command))
        .collect();
    assert_eq!(commands, ["stop", "inspect", "cp", "rm"]);
    assert_eq!(
        calls_of(&invocations, "stop")[0].arguments,
        ["stop", "--time", "10", NAME]
    );
    assert!(fake.log.lines().contains(&format!(
        "INFO: Diagnostic container {NAME} that an earlier run left: exported, removed."
    )));
}
