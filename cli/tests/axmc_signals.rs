//! the signals on the axmc profile as real processes: `marshal start` against the fake
//! `tart`, the fake `softnet`, the fake `docker` in the VMs and a loopback GitHub stub,
//! with the scripted signal source (`MARSHAL_TEST_SIGNALS`) driving the sleep, the
//! primary network and the keep-awake hold from outside. The scripted source runs the
//! real cadence at a fiftieth of its time, so one network check is 100 ms. Each test
//! names the acceptance criterion of plan #74 it covers.
#![cfg(unix)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod support;

use std::time::Duration;

use support::axmc::{MacHost, OTHER_WIFI_NETWORK, VPN_NETWORK, WIFI_NETWORK, linux};
use support::host::{Finished, Started, registered, texts};
use support::stub::{Reply, Stub};

/// long enough for a fresh instance to boot a VM, probe it and run its container.
const RUNNING_WITHIN: Duration = Duration::from_secs(30);

const SLEPT: &str = "the Mac slept, so every runner was stopped";

fn github() -> Stub {
    Stub::start((0..20).map(|_| registered()).collect::<Vec<Reply>>())
}

fn running_jobs(status: &str) -> Vec<String> {
    status
        .lines()
        .filter(|line| line.starts_with("slot: "))
        .filter_map(|line| line.split_once("running container "))
        .map(|(_, name)| name.trim().to_string())
        .collect()
}

fn running(jobs: usize) -> impl Fn(&str) -> bool {
    move |status| running_jobs(status).len() == jobs
}

fn holding_host() -> MacHost {
    let host = MacHost::new(&[linux("linuxone", 1)]);
    host.docker_script("run_mode", "hold");
    host
}

fn stopped_because(reason: &'static str) -> impl Fn(&str) -> bool {
    move |status| status.contains("state: stopped") && status.contains(&format!("reason: {reason}"))
}

fn stop_and_finish(host: &MacHost, started: Started) -> Finished {
    let stop = host.client(&["stop"]);
    assert_eq!(stop.status.code(), Some(0), "{:?}", texts(&stop));
    started.finish(Duration::from_secs(40))
}

fn assert_no_job_vm(host: &MacHost) {
    assert_eq!(host.vm_names().len(), 1, "only the golden VM is left");
    assert!(host.containers().is_empty(), "{:?}", host.containers());
}

fn changed(from: &str, to: &str) -> String {
    format!("the primary network changed ({from} to {to}), so every runner was stopped")
}

#[test]
fn a_wake_stops_a_running_instance_with_its_reason_deletes_every_vm_and_only_start_resumes_it() {
    // AC: a wake moves a running instance to stopped with its reason and deletes every VM
    let host = holding_host();
    let stub = github();
    let started = host.start(&stub.address);
    host.wait_for_status("the job", RUNNING_WITHIN, running(1));
    assert_eq!(host.vm_names().len(), 2, "the job VM exists");

    host.sleep();
    let status = host.wait_for_status(
        "a stop for the sleep",
        RUNNING_WITHIN,
        stopped_because(SLEPT),
    );
    assert!(running_jobs(&status).is_empty(), "{status}");
    assert_no_job_vm(&host);

    // nothing resumes on its own
    let clones = host.count_of("clone");
    std::thread::sleep(Duration::from_millis(600));
    assert!(host.status().contains("state: stopped"));
    assert_eq!(host.count_of("clone"), clones);

    let again = host.start_again(&stub.address);
    let (_, stderr) = texts(&again);
    assert_eq!(again.status.code(), Some(0), "{stderr}");
    host.wait_for_status("the job again", RUNNING_WITHIN, running(1));

    let finished = stop_and_finish(&host, started);
    assert_eq!(finished.status.code(), Some(0), "{}", finished.stderr);
    let line = format!("The instance is stopped: {SLEPT}.");
    assert!(
        finished.stderr.contains(&line),
        "the terminal: {}",
        finished.stderr
    );
    assert!(host.log_text().contains(&line));
}

#[test]
fn a_primary_network_that_differs_stops_every_runner_naming_both_interfaces_and_a_resume_records_it_afresh()
 {
    // AC: a primary-network difference on two checks in a row stops the instance with
    // its reason; a VPN's interface taking the default route is a change; losing the
    // default route names `none`; a resume records the network afresh
    let host = holding_host();
    let stub = github();
    let started = host.start(&stub.address);
    host.wait_for_status("the job", RUNNING_WITHIN, running(1));

    host.set_network(VPN_NETWORK);
    let reason = changed("en0", "utun4");
    let status = host.wait_for_status("a stop for the VPN", RUNNING_WITHIN, move |status| {
        status.contains("state: stopped") && status.contains(&reason)
    });
    assert!(running_jobs(&status).is_empty(), "{status}");
    assert_no_job_vm(&host);

    // the VPN is the network at the resume
    let resumed = host.start_again(&stub.address);
    assert_eq!(resumed.status.code(), Some(0), "{:?}", texts(&resumed));
    host.wait_for_status("the job on the VPN", RUNNING_WITHIN, running(1));
    std::thread::sleep(Duration::from_millis(600));
    assert!(host.status().contains("state: running"));

    host.set_network("none");
    let reason = changed("utun4", "none");
    host.wait_for_status("a stop for the lost route", RUNNING_WITHIN, move |status| {
        status.contains("state: stopped") && status.contains(&reason)
    });
    assert_no_job_vm(&host);

    let finished = stop_and_finish(&host, started);
    assert_eq!(finished.status.code(), Some(0), "{}", finished.stderr);
    assert!(
        finished.stderr.contains(&format!(
            "The instance is stopped: {}.",
            changed("en0", "utun4")
        )),
        "the terminal: {}",
        finished.stderr
    );
}

#[test]
fn another_router_on_the_same_interface_is_a_change_and_the_same_network_is_not() {
    // AC: the same interface and router with the same hardware address is not a change
    let host = holding_host();
    let stub = github();
    let started = host.start(&stub.address);
    host.wait_for_status("the job", RUNNING_WITHIN, running(1));

    // the same network again, and with its hardware address unreadable
    host.set_network(WIFI_NETWORK);
    host.set_network("interface=en0\nrouter=192.0.2.1\n");
    std::thread::sleep(Duration::from_millis(800));
    assert!(host.status().contains("state: running"));

    host.set_network(OTHER_WIFI_NETWORK);
    let reason = changed("en0", "en0");
    host.wait_for_status(
        "a stop for the other router",
        RUNNING_WITHIN,
        move |status| status.contains("state: stopped") && status.contains(&reason),
    );
    let finished = stop_and_finish(&host, started);
    assert_eq!(finished.status.code(), Some(0), "{}", finished.stderr);
}

#[test]
fn with_no_default_route_at_start_no_vm_is_cloned_and_status_waits_for_a_network() {
    // AC: with no default route at start, no VM is cloned until one has held for two
    // checks, and `status` shows that the instance is waiting for a network
    let host = holding_host();
    host.set_network("none");
    let stub = github();
    let started = host.start(&stub.address);
    let status = host.wait_for_status("the wait", RUNNING_WITHIN, |status| {
        status.contains("network: waiting for a network")
    });
    assert!(status.contains("state: running"), "{status}");
    std::thread::sleep(Duration::from_millis(800));
    assert_eq!(host.count_of("clone"), 0, "{:?}", host.tart_subcommands());
    assert!(host.status().contains("network: waiting for a network"));

    host.set_network(WIFI_NETWORK);
    let status = host.wait_for_status("the job", RUNNING_WITHIN, running(1));
    assert!(!status.contains("network:"), "{status}");
    assert_eq!(host.count_of("clone"), 1);

    let finished = stop_and_finish(&host, started);
    assert_eq!(finished.status.code(), Some(0), "{}", finished.stderr);
    assert!(
        host.log_text()
            .contains("No IPv4 default route, so no VM is cloned")
    );
}

#[test]
fn a_stop_while_waiting_for_a_network_ends_the_instance_without_a_vm() {
    // AC: the wait for a network holds the core, and a stop still ends it
    let host = holding_host();
    host.set_network("none");
    let stub = github();
    let started = host.start(&stub.address);
    host.wait_for_status("the wait", RUNNING_WITHIN, |status| {
        status.contains("network: waiting for a network")
    });
    let finished = stop_and_finish(&host, started);
    assert_eq!(finished.status.code(), Some(0), "{}", finished.stderr);
    assert_eq!(host.count_of("clone"), 0);
    assert_no_job_vm(&host);
}

#[test]
fn the_keep_awake_hold_is_held_while_running_and_released_when_stopped_and_a_failed_hold_warns_and_retries()
 {
    // AC: the hold is held while running or stopping and released when stopped; a
    // failed hold warns and is retried
    let host = holding_host();
    host.fail_keep_awake(Some("keep-awake is not held (scripted)"));
    let stub = github();
    let started = host.start(&stub.address);
    let status = host.wait_for_status("the warning", RUNNING_WITHIN, |status| {
        status.contains("warning: keep-awake is not held (scripted)")
    });
    assert!(status.contains("state: running"), "{status}");
    host.wait_for_keep_awake("warning");

    host.fail_keep_awake(None);
    host.wait_for_status("the hold", RUNNING_WITHIN, |status| {
        !status.contains("keep-awake is not held") && running_jobs(status).len() == 1
    });
    host.wait_for_keep_awake("held");
    let log = host.log_text();
    assert!(log.contains("keep-awake is not held (scripted)"), "{log}");
    assert!(log.contains("Keep-awake is held again."), "{log}");

    host.sleep();
    host.wait_for_status("the stop", RUNNING_WITHIN, stopped_because(SLEPT));
    host.wait_for_keep_awake("released");

    let resumed = host.start_again(&stub.address);
    assert_eq!(resumed.status.code(), Some(0), "{:?}", texts(&resumed));
    host.wait_for_status("the job", RUNNING_WITHIN, running(1));
    host.wait_for_keep_awake("held");

    let finished = stop_and_finish(&host, started);
    assert_eq!(finished.status.code(), Some(0), "{}", finished.stderr);
    host.wait_for_keep_awake("released");
}

#[test]
fn a_sleep_while_the_instance_is_stopped_is_not_a_stop_at_the_resume() {
    // carry-over: the sleep baseline is armed again at each resume
    let host = holding_host();
    let stub = github();
    let started = host.start(&stub.address);
    host.wait_for_status("the job", RUNNING_WITHIN, running(1));
    host.sleep();
    host.wait_for_status("the stop", RUNNING_WITHIN, stopped_because(SLEPT));

    // the Mac sleeps again while nothing samples the clocks
    host.sleep();
    let resumed = host.start_again(&stub.address);
    assert_eq!(resumed.status.code(), Some(0), "{:?}", texts(&resumed));
    host.wait_for_status("the job", RUNNING_WITHIN, running(1));
    std::thread::sleep(Duration::from_millis(800));
    assert!(host.status().contains("state: running"));
    assert!(!host.signals_dir().join("sleep").exists());

    let finished = stop_and_finish(&host, started);
    assert_eq!(finished.status.code(), Some(0), "{}", finished.stderr);
}
