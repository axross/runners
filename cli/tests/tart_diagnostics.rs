//! finalising a diagnostic container that runs in a job VM through the Tart backend:
//! the exporter child (this CLI run again) reads the evidence with `tart exec <vm>
//! docker inspect` and `docker cp` against the fake `tart` and the fake `docker`, and
//! writes it into a real sink; then the VM is deleted. Each test names the acceptance
//! criterion (AC: "The VM transport exports the diagnostic variant's evidence within
//! the bundle limits, token-free; the stop timeline's gaps behave as for containers;
//! and `diagnostics` finalises a retained VM as it does a container") or the decision
//! (D4: a diagnostic VM is deleted without export at a forced stop, a crash and the
//! next `start`) it checks.
#![cfg(unix)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod support;

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use marshal_cli::diagnostics::exporter::COMPLETE_TEXT;
use marshal_cli::diagnostics::identity::IDENTITY_HEADER;
use marshal_cli::runtime::backend::{
    Backend, ContainerSpec, Deadlines, DiagnosticRecord, ExportOutcome, HostPreparation,
    StopRequest,
};
use marshal_cli::runtime::clock::Clock;
use marshal_cli::runtime::diagnostic::{self, StopClock, container_steps, finalize_at_job_end};
use marshal_cli::runtime::name::job_container_name;
use marshal_cli::runtime::secret::Secret;
use support::rig::{POOL, Rig, SLOT, TestClock, golden_name, host, job_vm, request};
use support::{TempDir, files};
use tokio::time::Instant;

const IMAGE: &str = "actions-runner:local";
const JIT: &str = "jit-0123456789";
const TOKEN: &str = "ghp_slottoken";

fn private_sink(scratch: &TempDir) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let sink = scratch.path().join("sink");
    fs::create_dir(&sink).unwrap();
    fs::set_permissions(&sink, fs::Permissions::from_mode(0o700)).unwrap();
    fs::canonicalize(sink).unwrap()
}

fn record(sink: &Path) -> DiagnosticRecord {
    DiagnosticRecord {
        pool: POOL.to_string(),
        directory: sink.to_str().unwrap().to_string(),
        raw_records: false,
    }
}

fn bundles(sink: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = fs::read_dir(sink)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(&format!("{POOL}-bundle-"))
        })
        .collect();
    found.sort();
    found
}

/// prepares the slot's VM and runs one diagnostic job in it to its end: the container
/// is retained and its VM is up.
async fn ended_diagnostic_job(rig: &Rig, record: &DiagnosticRecord) -> String {
    assert_eq!(
        rig.backend.prepare_job_host(&request()).await,
        HostPreparation::Ready,
        "{:?}",
        rig.log.lines()
    );
    let name = job_container_name(POOL, SLOT, TestClock.now());
    let spec = ContainerSpec {
        name: name.clone(),
        cpus: "4".to_string(),
        cpuset: "0-3".to_string(),
        memory_gb: 16,
        mounts: Vec::new(),
        image: IMAGE.to_string(),
        run_command: "/home/runner/run.sh".to_string(),
        diagnostic: Some(record.clone()),
    };
    let exit = rig
        .backend
        .run_container(&spec, &Secret::new(JIT.to_string()))
        .await
        .unwrap();
    assert_eq!(exit, 0);
    assert!(rig.backend.retained().contains(&name));
    assert!(rig.fake.vm_names().contains(&job_vm()), "the VM is kept");
    name
}

/// the position of the first `tart` call that satisfies `matches`.
fn position(calls: &[Vec<String>], matches: impl Fn(&[String]) -> bool) -> usize {
    calls
        .iter()
        .position(|call| matches(call))
        .unwrap_or_else(|| panic!("no such call in {calls:?}"))
}

/// every file of `directory`, read, with its path.
fn texts(directory: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut found = Vec::new();
    for entry in fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            found.extend(texts(&path));
        } else {
            let bytes = fs::read(&path).unwrap();
            found.push((path, bytes));
        }
    }
    found
}

fn contains(haystack: &[u8], needle: &str) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle.as_bytes())
}

/// AC: the VM transport exports the diagnostic variant's evidence, token-free, and
/// the finalisation then deletes the VM.
#[tokio::test(flavor = "current_thread")]
async fn a_finalisation_exports_the_evidence_through_the_guest_agent_token_free_then_deletes_the_vm()
 {
    let rig = Rig::exporting();
    let scratch = TempDir::new();
    let sink = private_sink(&scratch);
    let name = ended_diagnostic_job(&rig, &record(&sink)).await;

    finalize_at_job_end(&rig.backend, &*rig.log, &name, Instant::now()).await;

    assert!(!rig.backend.retained().contains(&name));
    assert_eq!(rig.fake.vm_names(), [golden_name()], "the VM is deleted");
    assert!(rig.log.lines().iter().all(|line| !line.contains("gap")));
    let found = bundles(&sink);
    assert_eq!(found.len(), 1);
    let bundle = &found[0];
    assert_eq!(
        files(bundle),
        ["complete.txt", "identity.txt", "metrics.txt"]
    );
    assert_eq!(
        fs::read_to_string(bundle.join("complete.txt")).unwrap(),
        COMPLETE_TEXT
    );
    assert_eq!(
        fs::read_to_string(bundle.join("metrics.txt")).unwrap(),
        "sample=1\nfinal=observed\n"
    );
    assert!(
        fs::read_to_string(bundle.join("identity.txt"))
            .unwrap()
            .starts_with(&format!("{IDENTITY_HEADER}\n")),
    );

    let vm = job_vm();
    let calls = rig.tart_calls();
    let in_vm = |call: &[String], word: &str| {
        call.first().map(String::as_str) == Some("exec")
            && call.get(1) == Some(&vm)
            && call.get(2).map(String::as_str) == Some("docker")
            && call.get(3).map(String::as_str) == Some(word)
    };
    let inspect = position(&calls, |call| in_vm(call, "inspect"));
    let copy = position(&calls, |call| in_vm(call, "cp"));
    let delete = position(&calls, |call| call == ["delete", &vm]);
    assert!(inspect < copy && copy < delete, "{calls:?}");
    assert_eq!(
        calls[copy][4..],
        [format!("{name}:/tmp/runner-diagnostics/."), "-".to_string()]
    );
    for call in rig.fake.invocations() {
        let reads_evidence = call.arguments.get(2).map(String::as_str) == Some("docker")
            && call.arguments.first().map(String::as_str) == Some("exec");
        if reads_evidence {
            assert!(!call.jit_present, "{:?}", call.arguments);
            assert_ne!(call.arguments.get(1).map(String::as_str), Some("-i"));
        }
    }
    for (path, bytes) in texts(&sink) {
        for secret in [JIT, TOKEN] {
            assert!(
                !contains(&bytes, secret),
                "{} holds a secret",
                path.display()
            );
        }
    }
}

/// AC: an archive the strict reader or a bundle limit refuses is refused as for a
/// container: a partial bundle without its completion marker, a gap, and the VM is
/// deleted all the same.
#[tokio::test(flavor = "current_thread")]
async fn a_malformed_or_oversized_archive_is_refused_as_for_a_container_and_the_vm_is_still_deleted()
 {
    for case in ["failed copy", "traversal name", "oversized metrics"] {
        let rig = Rig::exporting();
        let scratch = TempDir::new();
        let sink = private_sink(&scratch);
        match case {
            "failed copy" => rig.docker_script("cp_mode", "fail"),
            "traversal name" => rig.docker_script("cp_mode", "bad"),
            _ => rig.docker_script("metrics_text", &"a".repeat(9 * 1024 * 1024)),
        }
        let name = ended_diagnostic_job(&rig, &record(&sink)).await;
        let claim = rig.backend.retained().claim(&name).unwrap();

        let finalized =
            diagnostic::finalize(&rig.backend, &claim, Deadlines::from(Instant::now())).await;

        assert_eq!(finalized.export, ExportOutcome::Failed, "{case}");
        assert!(finalized.removed, "{case}");
        assert_eq!(rig.fake.vm_names(), [golden_name()], "{case}");
        let found = bundles(&sink);
        assert_eq!(found.len(), 1, "{case}: the partial bundle stays");
        assert!(
            !files(&found[0]).contains(&"complete.txt".to_string()),
            "{case}"
        );
        if case == "oversized metrics" {
            let metrics = found[0].join("metrics.txt");
            let size = fs::metadata(metrics).map_or(0, |metadata| metadata.len());
            assert!(size <= 8 * 1024 * 1024, "{case}: {size}");
        }
    }
}

/// AC: a final observation that is missing is a gap with a complete bundle, and a full
/// sink is refused with the container's text, whatever carries the evidence.
#[tokio::test(flavor = "current_thread")]
async fn a_missing_final_observation_and_a_full_sink_are_the_gaps_a_container_has() {
    let rig = Rig::exporting();
    let scratch = TempDir::new();
    let sink = private_sink(&scratch);
    rig.docker_script("cp_mode", "gap");
    let name = ended_diagnostic_job(&rig, &record(&sink)).await;
    finalize_at_job_end(&rig.backend, &*rig.log, &name, Instant::now()).await;
    assert_eq!(
        rig.log_lines("WARN: "),
        [format!(
            "WARN: Diagnostic gap in pool {POOL}, container {name}: final observation unavailable (abrupt exit or observation limit)."
        )]
    );
    let found = bundles(&sink);
    assert_eq!(
        files(&found[0]),
        ["complete.txt", "identity.txt", "metrics.txt"]
    );
    assert_eq!(rig.fake.vm_names(), [golden_name()]);

    let rig = Rig::exporting();
    let scratch = TempDir::new();
    let sink = private_sink(&scratch);
    for index in 1..=10 {
        fs::create_dir(sink.join(format!("{POOL}-bundle-partial-{index}"))).unwrap();
    }
    let name = ended_diagnostic_job(&rig, &record(&sink)).await;
    finalize_at_job_end(&rig.backend, &*rig.log, &name, Instant::now()).await;
    assert_eq!(
        rig.log_lines("WARN: "),
        [format!(
            "WARN: Diagnostic gap in pool {POOL}, container {name}: private sink full (ten bundles); prior evidence retained."
        )]
    );
    assert_eq!(bundles(&sink).len(), 10, "no eleventh bundle");
    assert_eq!(rig.fake.vm_names(), [golden_name()], "the VM goes anyway");
}

/// AC: the stop timeline's gaps. A stop that fails is a gap, and the export and the
/// deletion still run.
#[tokio::test(flavor = "current_thread")]
async fn a_failed_stop_is_a_gap_and_the_export_and_the_deletion_still_run() {
    let rig = Rig::exporting();
    let scratch = TempDir::new();
    let sink = private_sink(&scratch);
    rig.docker_script("stop_fails", "");
    let name = ended_diagnostic_job(&rig, &record(&sink)).await;
    let claims = vec![rig.backend.retained().claim(&name).unwrap()];

    let acted = diagnostic::finalize_claimed(
        &rig.backend,
        &*rig.log,
        claims,
        StopClock::at(Instant::now()),
    )
    .await;

    assert_eq!(acted.len(), 1);
    assert_eq!(acted[0].finalized.export, ExportOutcome::Complete);
    assert!(acted[0].finalized.removed);
    assert_eq!(
        rig.log_lines("WARN: "),
        [format!(
            "WARN: Diagnostic gap in pool {POOL}, container {name}: stop unavailable or over budget; export and removal will still be attempted."
        )]
    );
    assert_eq!(bundles(&sink).len(), 1);
    assert_eq!(rig.fake.vm_names(), [golden_name()]);
}

/// AC: the stop timeline's gaps. A container whose export cannot begin by 18 s is
/// removed, and its VM deleted, with a gap.
#[tokio::test(flavor = "current_thread")]
async fn a_clock_that_leaves_no_time_to_begin_the_export_skips_it_and_still_deletes_the_vm() {
    let rig = Rig::exporting();
    let scratch = TempDir::new();
    let sink = private_sink(&scratch);
    let name = ended_diagnostic_job(&rig, &record(&sink)).await;
    let claim = rig.backend.retained().claim(&name).unwrap();
    let started = Instant::now() - Duration::from_secs(20);

    let finalized = diagnostic::finalize(&rig.backend, &claim, Deadlines::from(started)).await;

    assert_eq!(finalized.export, ExportOutcome::Skipped);
    assert!(finalized.removed);
    assert_eq!(rig.fake.vm_names(), [golden_name()]);
    assert!(bundles(&sink).is_empty());
    let calls = rig.tart_calls();
    assert!(
        calls
            .iter()
            .all(|call| call.get(3).map(String::as_str) != Some("cp")),
        "{calls:?}"
    );
}

/// AC: `diagnostics` finalises a retained VM as it does a container: it exports the
/// evidence, deletes the VM, and counts the pool's bundles.
#[tokio::test(flavor = "current_thread")]
async fn diagnostics_finalises_a_retained_vm_by_exporting_it_and_deleting_it() {
    let rig = Rig::exporting();
    let scratch = TempDir::new();
    let sink = private_sink(&scratch);
    let record = record(&sink);
    let name = ended_diagnostic_job(&rig, &record).await;

    let flushed = diagnostic::flush(&rig.backend, &*rig.log, std::slice::from_ref(&record)).await;

    assert!(flushed.complete, "{flushed:?}");
    assert!(flushed.containers.is_empty(), "{flushed:?}");
    assert_eq!(flushed.pools.len(), 1);
    assert_eq!(
        flushed.pools[0].line(),
        format!("{POOL}: 1 bundles written, 9 of 10 remaining")
    );
    assert!(!rig.backend.retained().contains(&name));
    assert_eq!(rig.fake.vm_names(), [golden_name()]);
    let found = bundles(&sink);
    assert_eq!(
        files(&found[0]),
        ["complete.txt", "identity.txt", "metrics.txt"]
    );
    let calls = rig.tart_calls();
    let copy = position(&calls, |call| call.get(3).map(String::as_str) == Some("cp"));
    let delete = position(&calls, |call| call == ["delete", &job_vm()]);
    assert!(copy < delete, "the export comes before the deletion");

    let nothing = diagnostic::flush(&rig.backend, &*rig.log, std::slice::from_ref(&record)).await;
    assert!(nothing.complete);
    assert_eq!(
        bundles(&sink).len(),
        1,
        "a second `diagnostics` exports nothing"
    );
}

/// the stop sequence leaves a worker that a stop has ended alone while its VM holds
/// evidence, and releases the VM after the finalisation.
#[tokio::test(flavor = "current_thread")]
async fn a_vm_holding_evidence_waits_for_its_finalisation_and_a_finalised_one_does_not() {
    let rig = Rig::exporting();
    let scratch = TempDir::new();
    let sink = private_sink(&scratch);
    assert!(!rig.backend.host_awaits_finalisation(&host()), "no VM yet");
    let name = ended_diagnostic_job(&rig, &record(&sink)).await;
    assert!(rig.backend.host_awaits_finalisation(&host()));

    finalize_at_job_end(&rig.backend, &*rig.log, &name, Instant::now()).await;
    assert!(!rig.backend.host_awaits_finalisation(&host()));
    assert!(rig.backend.prepared_hosts().is_empty());
}

/// D4: a forced stop deletes the diagnostic VM without an export and reports the lost
/// evidence with the container's gap text; the container stays among those kept, which
/// is what makes `start` exit 1.
#[tokio::test(flavor = "current_thread")]
async fn a_forced_stop_deletes_the_diagnostic_vm_without_an_export_and_reports_the_gap() {
    let rig = Rig::exporting();
    let scratch = TempDir::new();
    let sink = private_sink(&scratch);
    let name = ended_diagnostic_job(&rig, &record(&sink)).await;
    assert!(rig.backend.deletes_leftovers_unexported());

    rig.backend.retained().force();
    container_steps(
        &rig.backend,
        &*rig.log,
        StopClock::at(Instant::now()),
        Duration::from_secs(5),
    )
    .await;

    assert_eq!(
        rig.log_lines("WARN: "),
        [
            format!(
                "WARN: Forced stop: the job VM of diagnostic container {name} will be deleted without its export when the hosts are released."
            ),
            format!(
                "WARN: Diagnostic gap in pool {POOL}, container {name}: private export unavailable, incomplete, unsafe or over budget; prior evidence retained."
            ),
        ]
    );
    assert_eq!(
        rig.backend.retained().names(),
        std::slice::from_ref(&name),
        "kept: the exit is 1"
    );
    assert!(bundles(&sink).is_empty(), "nothing was exported");
    let calls = rig.tart_calls();
    assert!(
        calls.iter().all(|call| !matches!(
            call.get(3).map(String::as_str),
            Some("cp" | "kill" | "inspect")
        )),
        "{calls:?}"
    );

    rig.backend
        .release_job_host(&host(), Duration::from_secs(10))
        .await
        .unwrap();
    assert_eq!(
        rig.fake.vm_names(),
        [golden_name()],
        "the release deletes it"
    );
    assert_eq!(
        rig.backend.retained().names(),
        [name],
        "its evidence was never exported, so it stays among those lost"
    );
}

/// N2: an export that succeeded and a removal that failed leave the container
/// retained, and the release that then deletes its VM leaves nothing to report as
/// lost, because the bundle stands in the sink.
#[tokio::test(flavor = "current_thread")]
async fn a_release_that_deletes_the_vm_of_an_exported_container_forgets_it() {
    let rig = Rig::exporting();
    let scratch = TempDir::new();
    let sink = private_sink(&scratch);
    let name = ended_diagnostic_job(&rig, &record(&sink)).await;
    let claim = rig.backend.retained().claim(&name).unwrap();
    rig.fake.script("fail-delete", "");

    let finalized =
        diagnostic::finalize(&rig.backend, &claim, Deadlines::from(Instant::now())).await;

    assert_eq!(finalized.export, ExportOutcome::Complete);
    assert!(!finalized.removed, "the deletion failed");
    assert_eq!(bundles(&sink).len(), 1);
    assert_eq!(rig.backend.retained().names(), std::slice::from_ref(&name));
    drop(claim);

    fs::remove_file(rig.fake.dir.path().join("fail-delete")).unwrap();
    rig.backend
        .release_job_host(&host(), Duration::from_secs(10))
        .await
        .unwrap();

    assert_eq!(rig.fake.vm_names(), [golden_name()]);
    assert!(
        rig.backend.retained().names().is_empty(),
        "the export stands, so nothing was lost"
    );
}

/// the stop request of a finalisation that has no VM left cannot read anything: the
/// export fails with the container's gap and the removal finds nothing to delete.
#[tokio::test(flavor = "current_thread")]
async fn a_container_whose_vm_is_gone_is_a_failed_export_and_a_removal_that_succeeds() {
    let rig = Rig::exporting();
    let scratch = TempDir::new();
    let sink = private_sink(&scratch);
    let name = ended_diagnostic_job(&rig, &record(&sink)).await;
    rig.backend
        .release_job_host(&host(), Duration::from_secs(10))
        .await
        .unwrap();
    assert_eq!(rig.fake.vm_names(), [golden_name()]);

    let stop = StopRequest {
        names: vec![name.clone()],
        grace: Duration::from_secs(1),
        budget: Duration::from_secs(2),
    };
    rig.backend.stop_containers(&stop).await.unwrap();
    finalize_at_job_end(&rig.backend, &*rig.log, &name, Instant::now()).await;

    assert!(!rig.backend.retained().contains(&name));
    assert!(bundles(&sink).is_empty());
    assert_eq!(
        rig.log_lines("WARN: "),
        [format!(
            "WARN: Diagnostic gap in pool {POOL}, container {name}: private export unavailable, incomplete, unsafe or over budget; prior evidence retained."
        )]
    );
}
