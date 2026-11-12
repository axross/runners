//! `start`, `restart`, `status` and `stop` on the axmc profile as real processes
//! against the fake `tart`, the fake `softnet`, the fake `docker` in the VMs and a
//! loopback GitHub stub.
#![cfg(unix)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod support;

use std::time::{Duration, Instant};

use support::axmc::{MacHost, RUNNING_WITHIN, github, linux, macos, running, running_jobs};
use support::host::{Finished, texts};

const OLD_TART: &str = "2.41.0\n";
const OLD_TART_TEXT: &str = "Tart 2.41.0 is older than the tested version 2.41.1.";
const INSTALL_SECTION: &str =
    "the \"Installing Tart and Softnet\" section of the macOS Runner Host document";
const SOFTNET_SECTION: &str =
    "the \"Setting Up Softnet\" section of the macOS Runner Host document";
const SKIP_REASON: &str = "macOS VM pools are not supported yet";

/// a host whose job containers are held until a test ends them.
fn holding_host(pools: &[support::axmc::Pool]) -> MacHost {
    let host = MacHost::new(pools);
    host.docker_script("run_mode", "hold");
    host
}

fn assert_refused_before_any_vm(host: &MacHost, finished: &Finished) {
    assert_eq!(finished.status.code(), Some(1), "{}", finished.stderr);
    let forbidden = ["clone", "set", "run", "exec", "stop", "delete", "list"];
    let calls = host.tart_subcommands();
    assert!(
        calls.iter().all(|word| !forbidden.contains(&word.as_str())),
        "a VM was touched: {calls:?}"
    );
    assert_eq!(host.vm_names().len(), 1, "only the golden VM exists");
}

#[test]
fn a_linux_and_macos_configuration_starts_only_the_linux_pools_with_one_skip_warning_per_macos_pool()
 {
    let host = holding_host(&[macos("macone", 1), linux("linuxone", 1), macos("mactwo", 1)]);
    let stub = github();
    let started = host.start(&stub.address);
    let status = host.wait_for_status("the Linux job", RUNNING_WITHIN, running(1));
    assert!(status.contains("pool: linuxone: running"), "{status}");
    for pool in ["macone", "mactwo"] {
        assert!(
            status.contains(&format!("pool: {pool}: skipped: {SKIP_REASON}")),
            "{status}"
        );
    }

    let finished = host.stop_and_finish(started);
    assert_eq!(finished.status.code(), Some(0), "{}", finished.stderr);
    let log = host.log_text();
    for pool in ["macone", "mactwo"] {
        let line = format!("WARNING: Skipping pool {pool}: {SKIP_REASON}");
        assert_eq!(log.matches(&line).count(), 1, "{log}");
        assert_eq!(finished.stderr.matches(&line).count(), 1);
    }
    assert!(!log.contains("Skipping pool linuxone"), "{log}");
    let clones: Vec<_> = host
        .tart_calls()
        .into_iter()
        .filter(|call| call[0] == "clone")
        .collect();
    assert_eq!(clones.len(), 1, "{clones:?}");
    assert!(clones[0][2].contains("linuxone"), "{clones:?}");
    assert_eq!(host.vm_names().len(), 1, "the job VM is gone");
}

#[test]
fn a_macos_only_configuration_exits_1_and_needs_neither_tart_nor_softnet() {
    let host = MacHost::new(&[macos("macone", 1), macos("mactwo", 1)])
        .without_softnet()
        .with_tart_program("/nonexistent/marshal-test/tart");
    let stub = github();
    let finished = host.start(&stub.address).finish(RUNNING_WITHIN);
    assert_eq!(finished.status.code(), Some(1), "{}", finished.stderr);
    for pool in ["macone", "mactwo"] {
        assert!(
            finished
                .stderr
                .contains(&format!("Skipping pool {pool}: {SKIP_REASON}")),
            "{}",
            finished.stderr
        );
    }
    assert!(
        finished
            .stderr
            .contains("No pool can start, because macOS VM pools are not supported yet"),
        "{}",
        finished.stderr
    );
    assert!(stub.requests().is_empty(), "nothing was registered");
}

#[test]
fn a_missing_tart_is_refused_at_start_naming_the_install_section() {
    let host =
        holding_host(&[linux("linuxone", 1)]).with_tart_program("/nonexistent/marshal-test/tart");
    let stub = github();
    let finished = host.start(&stub.address).finish(RUNNING_WITHIN);
    assert_eq!(finished.status.code(), Some(1), "{}", finished.stderr);
    assert!(
        finished.stderr.contains("Tart could not be started")
            && finished.stderr.contains(INSTALL_SECTION),
        "{}",
        finished.stderr
    );
    assert!(host.tart_calls().is_empty());
    assert!(stub.requests().is_empty());
}

#[test]
fn a_missing_softnet_is_refused_at_start_before_any_vm_boots() {
    let host = holding_host(&[linux("linuxone", 1)]).without_softnet();
    let stub = github();
    let finished = host.start(&stub.address).finish(RUNNING_WITHIN);
    assert_refused_before_any_vm(&host, &finished);
    assert!(
        finished.stderr.contains("Softnet was not found on PATH")
            && finished.stderr.contains(INSTALL_SECTION),
        "{}",
        finished.stderr
    );
    assert!(stub.requests().is_empty());
}

#[test]
fn a_version_refusal_or_a_softnet_setup_refusal_exits_1_before_any_vm_boots() {
    type Prepare = fn(MacHost) -> MacHost;
    let cases: [(&str, Prepare, &[&str]); 5] = [
        (
            "an older Tart",
            |host| {
                host.tart_script("version_output", OLD_TART);
                host
            },
            &[OLD_TART_TEXT, INSTALL_SECTION],
        ),
        (
            "another Softnet minor version",
            |host| {
                host.softnet_script("version_output", "softnet 0.24.0-0f1e2d3\n");
                host
            },
            &[
                "Softnet 0.24.0 is not the tested version 0.25.0",
                INSTALL_SECTION,
            ],
        ),
        (
            "a Softnet that is not owned by root",
            |host| host.with_softnet_file(501, 0o4755),
            &["is owned by user 501, not root", SOFTNET_SECTION],
        ),
        (
            "a Softnet without the SUID bit",
            |host| host.with_softnet_file(0, 0o755),
            &["does not have the SUID bit set", SOFTNET_SECTION],
        ),
        (
            "a Softnet that its group can write",
            |host| host.with_softnet_file(0, 0o4775),
            &["can be written by its group", SOFTNET_SECTION],
        ),
    ];
    for (what, prepare, expected) in cases {
        let host = prepare(holding_host(&[linux("linuxone", 1)]));
        let stub = github();
        let finished = host.start(&stub.address).finish(RUNNING_WITHIN);
        assert_refused_before_any_vm(&host, &finished);
        for text in expected {
            assert!(
                finished.stderr.contains(text),
                "{what}: {}",
                finished.stderr
            );
        }
        assert!(stub.requests().is_empty(), "{what}");
    }
}

#[test]
fn a_softnet_that_does_not_answer_is_refused_at_start() {
    let host = holding_host(&[linux("linuxone", 1)]);
    host.softnet_script("hang-version", "");
    let stub = github();
    let finished = host.start(&stub.address).finish(RUNNING_WITHIN);
    assert_refused_before_any_vm(&host, &finished);
    assert!(
        finished
            .stderr
            .contains("\"softnet --version\" did not answer within 5 s")
            && finished.stderr.contains(SOFTNET_SECTION),
        "{}",
        finished.stderr
    );
}

#[test]
fn a_tart_that_does_not_answer_at_start_makes_the_slots_wait_and_they_boot_once_it_does() {
    let host = holding_host(&[linux("linuxone", 1)]);
    host.tart_script("hang-version", "");
    let stub = github();
    let started = host.start(&stub.address);
    let begun = Instant::now();
    while !started.stderr().contains("Tart is not available") {
        assert!(
            begun.elapsed() < RUNNING_WITHIN,
            "the slot never waited:\n{}",
            started.stderr()
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    let status = host.status();
    assert!(status.contains("state: running"), "{status}");
    assert_eq!(host.count_of("clone"), 0);

    host.remove_tart_script("hang-version");
    host.wait_for_status("the job", RUNNING_WITHIN, running(1));
    let finished = host.stop_and_finish(started);
    assert_eq!(finished.status.code(), Some(0), "{}", finished.stderr);
}

#[test]
fn a_pool_waits_for_tart_in_status_until_tart_lists_the_golden_vm() {
    let host = holding_host(&[linux("linuxone", 1)]);
    host.tart_script("fail-list", "");
    let stub = github();
    let started = host.start(&stub.address);
    let status = host.wait_for_status("the pool waiting", RUNNING_WITHIN, |status| {
        status.contains("pool: linuxone: waiting for Tart")
    });
    assert!(!status.contains("Docker"), "{status}");

    host.remove_tart_script("fail-list");
    let status = host.wait_for_status("the job", RUNNING_WITHIN, running(1));
    assert!(status.contains("pool: linuxone: running"), "{status}");
    let finished = host.stop_and_finish(started);
    assert_eq!(finished.status.code(), Some(0), "{}", finished.stderr);
}

#[test]
fn a_newer_patch_of_tart_and_softnet_runs_with_the_warnings_in_the_log_and_in_status() {
    let host = holding_host(&[linux("linuxone", 1)]);
    host.tart_script("version_output", "2.41.2\n");
    host.softnet_script("version_output", "softnet 0.25.1-0f1e2d3\n");
    let stub = github();
    let started = host.start(&stub.address);
    let status = host.wait_for_status("the job", RUNNING_WITHIN, running(1));
    let tart = "Tart 2.41.2 is a newer patch of the tested version 2.41.1; it runs, but only 2.41.1 was tested.";
    let softnet = "Softnet 0.25.1 is a newer patch of the tested version 0.25.0; it runs, but only 0.25.0 was tested.";
    for warning in [tart, softnet] {
        assert!(status.contains(&format!("warning: {warning}")), "{status}");
    }
    let finished = host.stop_and_finish(started);
    assert_eq!(finished.status.code(), Some(0), "{}", finished.stderr);
    let log = host.log_text();
    for warning in [tart, softnet] {
        assert_eq!(
            log.matches(&format!("WARNING: {warning}")).count(),
            1,
            "{log}"
        );
    }
}

#[test]
fn a_refused_resume_leaves_the_instance_stopped_with_the_refusal_as_its_reason() {
    let host = holding_host(&[linux("linuxone", 1)]);
    let stub = github();
    let started = host.start(&stub.address);
    host.wait_for_status("the job", RUNNING_WITHIN, running(1));
    host.tart_script("version_output", OLD_TART);

    let restart = host.client(&["restart"]);
    let (_, stderr) = texts(&restart);
    assert_eq!(restart.status.code(), Some(1), "{stderr}");
    assert!(
        stderr.contains("the instance stays stopped: Tart 2.41.0 is older"),
        "{stderr}"
    );
    let status = host.status();
    assert!(status.contains("state: stopped"), "{status}");
    assert!(
        status.contains(&format!(
            "reason: {OLD_TART_TEXT} Install Tart 2.41.1 as {INSTALL_SECTION} describes."
        )),
        "{status}"
    );
    assert!(running_jobs(&status).is_empty(), "{status}");
    assert_eq!(host.vm_names().len(), 1, "every job VM was deleted");
    let clones = host.count_of("clone");

    let again = host.start_again(&stub.address);
    let (_, stderr) = texts(&again);
    assert_eq!(again.status.code(), Some(1), "{stderr}");
    assert!(
        stderr.contains("the instance did not resume: the instance stays stopped: Tart 2.41.0"),
        "{stderr}"
    );
    assert_eq!(host.count_of("clone"), clones, "nothing booted");

    host.tart_script("version_output", "2.41.1\n");
    let resumed = host.start_again(&stub.address);
    let (_, stderr) = texts(&resumed);
    assert_eq!(resumed.status.code(), Some(0), "{stderr}");
    assert!(stderr.contains("The stopped instance resumed"), "{stderr}");
    host.wait_for_status("the job", RUNNING_WITHIN, running(1));
    let finished = host.stop_and_finish(started);
    assert_eq!(finished.status.code(), Some(0), "{}", finished.stderr);
}

#[test]
fn a_refusal_after_start_holds_the_slot_while_another_slots_job_finishes_and_it_boots_again() {
    let host = holding_host(&[linux("linuxone", 2)]);
    let stub = github();
    let started = host.start(&stub.address);
    let status = host.wait_for_status("both jobs", RUNNING_WITHIN, running(2));
    let jobs = running_jobs(&status);
    let first = jobs
        .iter()
        .find(|name| name.starts_with("linuxone-1-"))
        .unwrap();
    let second = jobs
        .iter()
        .find(|name| name.starts_with("linuxone-2-"))
        .unwrap();
    assert_eq!(host.count_of("clone"), 2);
    // `status` shows a job once its client starts; ending it before the fake engine
    // holds the container would end nothing
    host.wait_for_containers(&[first, second], RUNNING_WITHIN);

    host.tart_script("version_output", OLD_TART);
    host.end_job(first);
    let status = host.wait_for_status("slot 1 held", RUNNING_WITHIN, |status| {
        status.contains("slot: linuxone slot 1: held: Tart 2.41.0 is older")
    });
    assert!(
        status.contains("slot: linuxone slot 2: running container linuxone-2-"),
        "the other slot's job goes on:\n{status}"
    );
    assert!(host.containers().contains(second));
    assert!(
        host.log_text()
            .contains("linuxone slot 1: held, nothing is started (Tart 2.41.0 is older"),
        "{}",
        host.log_text()
    );

    host.end_job(second);
    host.wait_for_status("both slots held", RUNNING_WITHIN, |status| {
        status.contains("slot: linuxone slot 1: held:")
            && status.contains("slot: linuxone slot 2: held:")
    });
    assert_eq!(host.count_of("clone"), 2, "a held slot boots nothing");
    assert_eq!(host.vm_names().len(), 1, "both job VMs were deleted");

    host.tart_script("version_output", "2.41.1\n");
    host.wait_for_status("both jobs again", Duration::from_secs(60), running(2));
    assert_eq!(host.count_of("clone"), 4);
    let finished = host.stop_and_finish(started);
    assert_eq!(finished.status.code(), Some(0), "{}", finished.stderr);
    assert!(
        host.log_text()
            .contains("WARNING: linuxone slot 1: held, nothing is started"),
        "a hold is a warning"
    );
}

/// the slot's wait for its VM's Docker Engine is not raced against the stop, so it
/// must end when the stop releases the VM, not at the 28 s exit deadline.
#[test]
fn a_stop_while_a_slot_waits_for_its_vms_docker_engine_ends_far_before_the_exit_deadline() {
    let host = holding_host(&[linux("linuxone", 1)]);
    host.docker_script("info_failures", "100000");
    let stub = github();
    let started = host.start(&stub.address);
    let waiting = Instant::now();
    while !support::docker::invocations(&host.docker_dir())
        .iter()
        .any(|call| call.arguments == ["info"])
    {
        assert!(
            waiting.elapsed() < RUNNING_WITHIN,
            "the slot never asked the VM's engine: {}",
            host.log_text()
        );
        std::thread::sleep(Duration::from_millis(20));
    }

    let requested = Instant::now();
    let stop = host.client(&["stop"]);
    assert_eq!(stop.status.code(), Some(0), "{:?}", texts(&stop));
    let finished = started.finish(Duration::from_secs(40));

    let took = requested.elapsed();
    assert!(
        took < Duration::from_secs(15),
        "the stop took {took:?}, as long as the wait for the engine"
    );
    assert_eq!(finished.status.code(), Some(0), "{}", finished.stderr);
    let log = host.log_text();
    assert!(
        !log.contains("did not finish within") && !finished.stderr.contains("did not finish"),
        "the stop ran into its exit deadline:\n{log}"
    );
    assert_eq!(host.vm_names().len(), 1, "only the golden VM is left");
}

/// the graceful stop is tried first and, when the VM ignores it, a forced stop
/// follows and the VM is deleted, all within the stop sequence's 30 s.
#[test]
fn a_vm_that_ignores_the_graceful_stop_is_stopped_forcibly_and_deleted_within_30_seconds() {
    let host = holding_host(&[linux("linuxone", 1)]);
    let stub = github();
    let started = host.start(&stub.address);
    let status = host.wait_for_status("the job", RUNNING_WITHIN, running(1));
    let job = running_jobs(&status).remove(0);
    host.wait_for_containers(&[&job], RUNNING_WITHIN);
    host.tart_script("stop_ignored", "");
    let vm = host
        .vm_names()
        .into_iter()
        .find(|name| name.starts_with("marshal-job-linuxone-1-"))
        .expect("the job VM exists");

    let requested = Instant::now();
    let stop = host.client(&["stop"]);
    assert_eq!(stop.status.code(), Some(0), "{:?}", texts(&stop));
    let finished = started.finish(Duration::from_secs(30));

    assert!(
        requested.elapsed() < Duration::from_secs(30),
        "the instance ended in {:?}",
        requested.elapsed()
    );
    assert_eq!(finished.status.code(), Some(0), "{}", finished.stderr);
    assert_eq!(host.vm_names().len(), 1, "the job VM {vm} was deleted");
    let stops: Vec<Vec<String>> = host
        .tart_calls()
        .into_iter()
        .filter(|call| call.first().map(String::as_str) == Some("stop") && call.contains(&vm))
        .collect();
    assert!(
        stops.iter().any(|call| call.ends_with(&["0".to_string()])),
        "the VM was stopped forcibly: {stops:?}"
    );
    assert!(
        stops.iter().any(|call| !call.ends_with(&["0".to_string()])),
        "a graceful stop was tried first: {stops:?}"
    );
}
