//! a diagnostics-enabled axmc Linux pool under `start` as real processes: the fake
//! `tart`, the fake `softnet`, the fake `docker` in the VMs, a loopback GitHub stub, and
//! this CLI run again as the exporter child and as the VM shim. The tests cover the VM
//! transport exporting the diagnostic variant's evidence within the bundle limits,
//! token-free; a killed `start` ending the exporter's `tart exec` clients with the
//! exporter; the stop timeline's gaps behaving as for containers; `diagnostics`
//! finalising a retained VM as it does a container; and a diagnostic VM deleted without
//! an export at a forced stop, at a crash through the shim and at the next `start`, with
//! the lost evidence reported in today's gap wording and exit-code rules (the paragraph
//! "Evidence a VM loses" in the Tart Backend section of docs/operations/cli.md).
#![cfg(unix)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod support;

use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use support::axmc::{MacHost, linux};
use support::docker::{FAKE_DOCKER, invocations};
use support::host::{JIT, Started, TOKEN, registered, send_signal, texts};
use support::stub::{Reply, Stub};
use support::tart::ended_within;

const RUNNING_WITHIN: Duration = Duration::from_secs(30);
const GAP: &str =
    "private export unavailable, incomplete, unsafe or over budget; prior evidence retained.";

fn host() -> MacHost {
    let host = MacHost::new(&[linux("d", 1).diagnostic()]);
    host.docker_script("run_mode", "hold");
    host
}

/// one registration, then a stall, so the slot waits for the stop.
fn one_job() -> Stub {
    Stub::start(vec![registered(), Reply::Stall])
}

fn wait_until(what: &str, within: Duration, mut condition: impl FnMut() -> bool) {
    let started = Instant::now();
    while !condition() {
        assert!(started.elapsed() < within, "never: {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// the job container the status shows running.
fn running_container(host: &MacHost) -> String {
    let status = host.wait_for_status("a running container", RUNNING_WITHIN, |status| {
        status.contains("running container ")
    });
    let name = status
        .lines()
        .find_map(|line| line.split_once("running container "))
        .map(|(_, name)| name.trim().to_string())
        .unwrap();
    // the status names the container before its `docker run` has created it
    wait_until("the fake engine runs it", RUNNING_WITHIN, || {
        host.containers().contains(&name)
    });
    name
}

/// ends a diagnostic job the way a runner's end does: the container exits and stays.
fn end_job(host: &MacHost, name: &str) {
    let status = Command::new(FAKE_DOCKER)
        .args(["kill", name])
        .env("FAKE_DOCKER_DIR", host.docker_dir())
        .status()
        .unwrap();
    assert!(status.success());
}

fn only_golden_vm_remains(host: &MacHost) -> bool {
    host.vm_names().len() == 1
}

fn complete(bundle: &Path) -> bool {
    bundle.join("complete.txt").exists()
}

fn all_text(host: &MacHost) -> String {
    let mut text = host.log_text();
    for bundle in host.bundles() {
        for entry in fs::read_dir(bundle).unwrap() {
            text.push_str(&fs::read_to_string(entry.unwrap().path()).unwrap_or_default());
        }
    }
    text
}

fn stop(host: &MacHost, started: Started) -> support::host::Finished {
    let stop = host.client(&["stop"]);
    assert_eq!(stop.status.code(), Some(0), "{:?}", texts(&stop));
    started.finish(Duration::from_secs(40))
}

/// At job end the exporter child reads the evidence from the VM through the guest
/// agent into the private directory, token-free, and the VM is deleted after it.
#[test]
fn a_finished_job_exports_its_evidence_from_the_vm_and_then_deletes_the_vm() {
    let host = host();
    let stub = one_job();
    let started = host.start(&stub.address);
    let name = running_container(&host);

    end_job(&host, &name);
    wait_until("the bundle is complete", RUNNING_WITHIN, || {
        host.bundles()
            .first()
            .is_some_and(|bundle| complete(bundle))
    });
    wait_until("the VM is deleted", RUNNING_WITHIN, || {
        only_golden_vm_remains(&host)
    });

    let bundle = &host.bundles()[0];
    assert_eq!(
        support::files(bundle),
        ["complete.txt", "identity.txt", "metrics.txt"]
    );
    let calls = host.tart_calls();
    let copy = calls
        .iter()
        .position(|call| {
            call.get(2).map(String::as_str) == Some("docker")
                && call.get(3).map(String::as_str) == Some("cp")
        })
        .expect("the evidence was copied through the guest agent");
    let delete = calls
        .iter()
        .rposition(|call| call.first().map(String::as_str) == Some("delete"))
        .unwrap();
    assert!(copy < delete, "the export comes first: {calls:?}");

    let finished = stop(&host, started);
    assert_eq!(finished.status.code(), Some(0), "{}", finished.stderr);
    let everything = format!("{}{}{}", all_text(&host), finished.stdout, finished.stderr);
    for secret in [JIT, TOKEN] {
        assert!(!everything.contains(secret), "{secret} leaked");
    }
    assert!(!everything.contains("Diagnostic gap"), "{everything}");
}

/// A graceful stop exports a running diagnostic job's evidence
/// before its VM goes, although the slot's worker ends as soon as the container does.
#[test]
fn a_graceful_stop_exports_the_running_jobs_evidence_before_its_vm_is_deleted() {
    let host = host();
    let stub = one_job();
    let started = host.start(&stub.address);
    let _ = running_container(&host);

    let finished = stop(&host, started);

    assert_eq!(finished.status.code(), Some(0), "{}", finished.stderr);
    let bundles = host.bundles();
    assert_eq!(bundles.len(), 1, "{}", host.log_text());
    assert!(complete(&bundles[0]), "{}", host.log_text());
    assert!(only_golden_vm_remains(&host));
    let calls = host.tart_calls();
    let copy = calls
        .iter()
        .position(|call| call.get(3).map(String::as_str) == Some("cp"))
        .unwrap();
    let delete = calls
        .iter()
        .position(|call| call.first().map(String::as_str) == Some("delete"))
        .unwrap();
    assert!(copy < delete, "{calls:?}");
}

/// A second SIGINT deletes the diagnostic VM without an export, reports the lost
/// evidence with the container's gap text, and `start` exits 1 naming the container.
#[test]
fn a_forced_stop_deletes_the_diagnostic_vm_without_an_export_and_exits_1() {
    let host = host();
    host.docker_script("stop_hangs", "");
    let stub = one_job();
    let started = host.start(&stub.address);
    let name = running_container(&host);

    send_signal(started.pid(), "INT");
    wait_until("the stop runs docker stop", Duration::from_secs(10), || {
        invocations(&host.docker_dir())
            .iter()
            .any(|call| call.arguments.first().map(String::as_str) == Some("stop"))
    });
    send_signal(started.pid(), "INT");
    let finished = started.finish(Duration::from_secs(40));

    assert_eq!(finished.status.code(), Some(1), "{}", finished.stderr);
    let last = finished.stderr.lines().last().unwrap_or_default();
    assert!(
        last.contains(&name) && last.contains("evidence is lost"),
        "{last}"
    );
    assert!(
        finished.stderr.contains(&format!(
            "Diagnostic gap in pool d, container {name}: {GAP}"
        )),
        "{}",
        finished.stderr
    );
    assert!(host.bundles().is_empty(), "nothing was exported");
    assert!(only_golden_vm_remains(&host), "the VM is deleted");
    assert!(
        host.tart_calls()
            .iter()
            .all(|call| call.get(3).map(String::as_str) != Some("cp"))
    );
}

/// The next `start` deletes a leftover diagnostic VM of a configured pool without
/// an export, and reports the lost evidence with the container's gap text.
#[test]
fn the_next_start_deletes_a_leftover_diagnostic_vm_without_an_export_and_reports_the_gap() {
    let host = host();
    let leftover = "marshal-job-d-1-20240305060708009";
    let golden = host.vm_names()[0].clone();
    host.tart_script(
        "vms",
        &format!("{golden}\tstopped\tlocal\t\t\t\n{leftover}\tstopped\tlocal\t\t\t"),
    );
    let stub = Stub::start(vec![Reply::Stall]);
    let started = host.start(&stub.address);

    wait_until("the leftover is reported", RUNNING_WITHIN, || {
        host.log_text().contains(&format!(
            "Diagnostic gap in pool d, job VM {leftover}: {GAP}"
        ))
    });
    assert!(host.log_text().contains(&format!(
        "Deleting job VM '{leftover}' left over from an earlier run."
    )));
    wait_until("the leftover is deleted", RUNNING_WITHIN, || {
        !host.vm_names().contains(&leftover.to_string())
    });
    let finished = stop(&host, started);
    assert_eq!(finished.status.code(), Some(0), "{}", finished.stderr);
    assert!(host.bundles().is_empty(), "nothing was exported");
    assert!(
        host.tart_calls()
            .iter()
            .all(|call| call.get(3).map(String::as_str) != Some("cp")),
        "nothing was read from the leftover"
    );
}

/// A killed `start` closes the VM shim's pipe; the shim deletes the VM and reports
/// the lost evidence with the container's gap text on the standard error it shares.
#[test]
fn a_killed_start_has_its_shim_delete_the_diagnostic_vm_and_report_the_gap() {
    let host = host();
    let stub = one_job();
    let mut started = host.start(&stub.address);
    let _ = running_container(&host);
    assert!(!only_golden_vm_remains(&host));

    started.kill();
    assert!(
        started.wait_for_stderr_to_close(Duration::from_secs(30)),
        "a shim is still running"
    );

    assert!(only_golden_vm_remains(&host), "the shim deleted the VM");
    let finished = started.finish(Duration::from_secs(10));
    let vm_line = finished
        .stderr
        .lines()
        .find(|line| line.contains("Diagnostic gap in pool d, job VM marshal-job-d-1-"))
        .unwrap_or_else(|| panic!("no gap reported: {}", finished.stderr));
    assert!(
        vm_line.starts_with("WARNING: ") && vm_line.ends_with(GAP),
        "{vm_line}"
    );
    assert!(host.bundles().is_empty());
}

/// A `start` killed while its exporter copies evidence out of the VM closes the
/// exporter's liveness pipe, so the exporter ends, and the `tart exec` client of its
/// `docker cp` ends with it.
#[test]
fn a_killed_start_ends_its_exporters_tart_exec_clients_with_the_exporter() {
    let host = host();
    host.docker_script("cp_mode", "hang");
    let stub = one_job();
    let mut started = host.start(&stub.address);
    let name = running_container(&host);
    end_job(&host, &name);
    let copy_call = |host: &MacHost| {
        host.tart_calls().iter().position(|call| {
            call.first().map(String::as_str) == Some("exec")
                && call.get(2).map(String::as_str) == Some("docker")
                && call.get(3).map(String::as_str) == Some("cp")
        })
    };
    wait_until("the copy runs in the VM", RUNNING_WITHIN, || {
        copy_call(&host).is_some()
    });
    let copy = copy_call(&host).unwrap();

    started.kill();
    assert!(
        started.wait_for_stderr_to_close(Duration::from_secs(30)),
        "the exporter outlived its instance"
    );

    assert!(
        ended_within(&host.tart_dir(), copy, Duration::from_secs(2)),
        "the tart exec client of the copy outlived the exporter"
    );
}

/// `diagnostics` finalises a retained VM as it does a container. A VM whose
/// deletion failed stays retained, and `diagnostics` reports it, then deletes it once
/// Tart can.
#[test]
fn diagnostics_finalises_a_retained_vm_whose_deletion_failed() {
    let host = host();
    host.tart_script("fail-delete", "Error: the VM cannot be deleted");
    let stub = one_job();
    let started = host.start(&stub.address);
    let name = running_container(&host);

    end_job(&host, &name);
    wait_until("the bundle is complete", RUNNING_WITHIN, || {
        host.bundles()
            .first()
            .is_some_and(|bundle| complete(bundle))
    });
    wait_until("the retained VM is reported", RUNNING_WITHIN, || {
        host.log_text().contains("it stays retained")
    });
    assert!(!only_golden_vm_remains(&host), "the VM is still there");

    let refused = host.client(&["diagnostics"]);
    let (stdout, _) = texts(&refused);
    assert_eq!(refused.status.code(), Some(1), "{stdout}");
    assert!(
        stdout.contains(&format!("{name}: removal failed; it stays retained")),
        "{stdout}"
    );

    host.remove_tart_script("fail-delete");
    let finalised = host.client(&["diagnostics"]);
    let (stdout, stderr) = texts(&finalised);
    assert_eq!(finalised.status.code(), Some(0), "{stdout}{stderr}");
    assert!(
        stdout.contains("pool d: 0 bundles written, 9 of 10 remaining"),
        "it is exported already, so nothing more is written: {stdout}"
    );
    assert!(only_golden_vm_remains(&host), "the VM is deleted");
    assert_eq!(host.bundles().len(), 1, "it was exported once");

    let finished = stop(&host, started);
    assert_eq!(finished.status.code(), Some(0), "{}", finished.stderr);
}
