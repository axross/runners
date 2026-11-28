//! the signals on the axmc profile as real processes: the commands helper's `start`
//! against the fake `tart`, the fake `softnet`, the fake `docker` in the VMs and a loopback GitHub stub,
//! with the scripted signal source (`MARSHAL_TEST_SIGNALS`) driving the sleep, the
//! primary network and the keep-awake hold from outside. The scripted source runs the
//! real cadence at a fiftieth of its time, so one network check is 100 ms. The tests
//! cover what stops the instance, how a resume starts afresh, the gate that holds back
//! the first clone and the keep-awake hold, and the same signals reaching the resident
//! agent on axmc.
#![cfg(unix)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod support;

use std::time::Duration;

use marshal_cli::agent::Push;
use marshal_cli::agent::machine::UPDATE;
use marshal_cli::agent::snapshot::Snapshot;
use marshal_cli::agent::wire::UpdateEnding;
use serde_json::json;

use support::agent::Conn;
use support::axmc::{
    MacHost, OTHER_WIFI_NETWORK, RUNNING_WITHIN, VPN_NETWORK, WIFI_NETWORK, github, linux, running,
    running_jobs,
};
use support::host::{registered, texts};
use support::stub::{Reply, Stub};

const SLEPT: &str = "the Mac slept, so every runner was stopped";

fn holding_host() -> MacHost {
    let host = MacHost::new(&[linux("linuxone", 1)]);
    host.docker_script("run_mode", "hold");
    host
}

fn stopped_because(reason: &'static str) -> impl Fn(&str) -> bool {
    move |status| status.contains("state: stopped") && status.contains(&format!("reason: {reason}"))
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
    // A wake moves a running instance to stopped with its reason and deletes every VM
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

    let finished = host.stop_and_finish(started);
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
    // A primary-network difference on two checks in a row stops the instance with
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

    let finished = host.stop_and_finish(started);
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
    // The same interface and router with the same hardware address is not a change
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
    let finished = host.stop_and_finish(started);
    assert_eq!(finished.status.code(), Some(0), "{}", finished.stderr);
}

#[test]
fn with_no_default_route_at_start_no_vm_is_cloned_and_status_waits_for_a_network() {
    // With no default route at start, no VM is cloned until one has held for two
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

    let finished = host.stop_and_finish(started);
    assert_eq!(finished.status.code(), Some(0), "{}", finished.stderr);
    assert!(
        host.log_text()
            .contains("No IPv4 default route, so no VM is cloned")
    );
}

#[test]
fn a_stop_while_waiting_for_a_network_ends_the_instance_without_a_vm() {
    // The wait for a network holds the core, and a stop still ends it
    let host = holding_host();
    host.set_network("none");
    let stub = github();
    let started = host.start(&stub.address);
    host.wait_for_status("the wait", RUNNING_WITHIN, |status| {
        status.contains("network: waiting for a network")
    });
    let finished = host.stop_and_finish(started);
    assert_eq!(finished.status.code(), Some(0), "{}", finished.stderr);
    assert_eq!(host.count_of("clone"), 0);
    assert_no_job_vm(&host);
}

#[test]
fn the_keep_awake_hold_is_held_while_running_and_released_when_stopped_and_a_failed_hold_warns_and_retries()
 {
    // The hold is held while running or stopping and released when stopped; a
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

    let finished = host.stop_and_finish(started);
    assert_eq!(finished.status.code(), Some(0), "{}", finished.stderr);
    host.wait_for_keep_awake("released");
}

#[test]
fn a_sleep_while_the_instance_is_stopped_is_not_a_stop_at_the_resume() {
    // The sleep baseline is armed again at each resume
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

    let finished = host.stop_and_finish(started);
    assert_eq!(finished.status.code(), Some(0), "{}", finished.stderr);
}

#[tokio::test(flavor = "current_thread")]
async fn the_resident_agent_on_axmc_holds_the_mac_awake_and_a_wake_stops_its_runners() {
    // the agent's axmc wiring passes the Mac's signal monitor to the machine: running
    // holds the Mac awake while `keep_awake` is on, a wake stops every runner with the
    // reason `slept`, and the stop has the time to delete the job VM, so the agent ends
    // cleanly
    let host = holding_host();
    let stub = github();
    let agent = host.agent(&stub.address, &["linuxone"]);
    let conn = Conn::open(&agent.roots).await;
    let snapshot = conn.until_runner_runs().await;
    host.wait_for_keep_awake("held");

    // the migrated file has no `[lifecycle]`, so each setting takes the Mac's default,
    // and the agent keeps no sign-in item on the Mac
    let lifecycle = snapshot.lifecycle.unwrap();
    let defaults: Vec<(&str, bool, bool)> = lifecycle
        .settings
        .iter()
        .map(|(key, value)| (key.as_str(), value.on, value.defaulted))
        .collect();
    assert_eq!(
        defaults,
        [
            ("keep_awake", true, true),
            ("resume_after_stop", false, true),
            ("start_at_login", false, true),
            ("start_runners_on_launch", true, true),
            ("stop_on_network_change", true, true),
        ]
    );
    assert_eq!(lifecycle.sign_in, None);

    // the hold follows `keep_awake` in the settings in effect while the runners run,
    // and each write keeps the rest of the file, its comment included
    let before = std::fs::read_to_string(host.state().join("config.toml")).unwrap();
    assert!(
        before.starts_with("# Converted from config.json"),
        "{before}"
    );
    for (on, expected) in [(false, "released"), (true, "held")] {
        conn.call("lifecycle_set", json!({ "key": "keep_awake", "value": on }))
            .await
            .unwrap();
        host.wait_for_keep_awake(expected);
    }
    let after = std::fs::read_to_string(host.state().join("config.toml")).unwrap();
    assert_eq!(
        after.replace("[lifecycle]\nkeep_awake = true\n\n", ""),
        before
    );

    host.sleep();
    let snapshot = conn.until_state("stopped").await;
    let reason = snapshot.agent.reason.unwrap();
    assert_eq!(reason.code, "slept");
    assert_eq!(reason.message, "the Mac slept, so every runner was stopped");
    host.wait_for_keep_awake("released");
    assert_no_job_vm(&host);

    conn.ok("quit").await;
    let (code, stderr) = agent.finish(Duration::from_secs(30)).await;
    assert_eq!(
        code,
        Some(0),
        "the stop deleted the job VM in time: {stderr}"
    );
}

fn resume_of(snapshot: &Snapshot) -> (Option<String>, Option<String>) {
    let lifecycle = snapshot.lifecycle.clone().unwrap();
    (lifecycle.resume, lifecycle.resume_cause)
}

fn waiting_after(cause: &str) -> (Option<String>, Option<String>) {
    (Some("waiting".into()), Some(cause.into()))
}

async fn set(conn: &Conn, key: &str, value: bool) {
    conn.call("lifecycle_set", json!({ "key": key, "value": value }))
        .await
        .unwrap();
}

/// a sleep while the Mac moves to another network, which stops the runners for the
/// sleep, so a resume waits for the network they ran on.
async fn sleep_onto_another_network(host: &MacHost, conn: &Conn) -> Snapshot {
    host.sleep();
    host.set_network(OTHER_WIFI_NETWORK);
    let stopped = conn.until_state("stopped").await;
    assert_eq!(stopped.agent.reason.as_ref().unwrap().code, "slept");
    stopped
}

#[tokio::test(flavor = "current_thread")]
async fn on_axmc_a_network_change_stops_whatever_the_setting_says_and_resume_waits_for_the_network_the_runners_ran_on()
 {
    // the network read feeds the resume: a stop for another network waits until the Mac
    // is back on the first one, and a sleep on the same network resumes after two
    // matching readings; `stop_on_network_change` off changes nothing on the Mac
    let host = holding_host();
    let stub = github();
    let agent = host.agent(&stub.address, &["linuxone"]);
    let conn = Conn::open(&agent.roots).await;
    conn.until_runner_runs().await;
    set(&conn, "stop_on_network_change", false).await;
    set(&conn, "resume_after_stop", true).await;

    host.set_network(OTHER_WIFI_NETWORK);
    let stopped = conn.until_state("stopped").await;
    let reason = stopped.agent.reason.clone().unwrap();
    assert_eq!(reason.code, "network_changed");
    assert_eq!(reason.message, changed("en0", "en0"));
    assert_eq!(resume_of(&stopped), waiting_after("network_changed"));
    assert_no_job_vm(&host);
    // ten checks on the other network leave the runners stopped
    std::thread::sleep(Duration::from_secs(1));
    assert_eq!(conn.status().await.agent.state, "stopped");

    host.set_network(WIFI_NETWORK);
    let running = conn.until_state("running").await;
    assert_eq!(resume_of(&running), (None, None));
    conn.until_runner_runs().await;

    host.sleep();
    let stopped = conn.until_state("stopped").await;
    assert_eq!(resume_of(&stopped), waiting_after("slept"));
    conn.until_state("running").await;
    conn.until_runner_runs().await;

    conn.ok("quit").await;
    let (code, stderr) = agent.finish(Duration::from_secs(30)).await;
    assert_eq!(code, Some(0), "{stderr}");
}

#[tokio::test(flavor = "current_thread")]
async fn on_axmc_stop_all_and_turning_resume_off_cancel_a_waiting_resume() {
    let host = holding_host();
    let stub = github();
    let agent = host.agent(&stub.address, &["linuxone"]);
    let conn = Conn::open(&agent.roots).await;
    conn.until_runner_runs().await;
    set(&conn, "resume_after_stop", true).await;

    let stopped = sleep_onto_another_network(&host, &conn).await;
    assert_eq!(resume_of(&stopped), waiting_after("slept"));
    conn.ok("stop_all").await;
    let snapshot = conn.status().await;
    assert_eq!(snapshot.agent.reason.as_ref().unwrap().code, "by_you");
    assert_eq!(resume_of(&snapshot), (None, None));
    host.set_network(WIFI_NETWORK);
    std::thread::sleep(Duration::from_secs(1));
    assert_eq!(conn.status().await.agent.state, "stopped");

    conn.ok("start_all").await;
    conn.until_runner_runs().await;
    let stopped = sleep_onto_another_network(&host, &conn).await;
    assert_eq!(resume_of(&stopped), waiting_after("slept"));
    set(&conn, "resume_after_stop", false).await;
    host.set_network(WIFI_NETWORK);
    std::thread::sleep(Duration::from_secs(1));
    let snapshot = conn.status().await;
    assert_eq!(snapshot.agent.state, "stopped");
    assert_eq!(snapshot.agent.reason.as_ref().unwrap().code, "slept");
    assert_eq!(resume_of(&snapshot), (None, None));

    conn.ok("quit").await;
    let (code, stderr) = agent.finish(Duration::from_secs(30)).await;
    assert_eq!(code, Some(0), "{stderr}");
}

#[tokio::test(flavor = "current_thread")]
async fn on_axmc_a_network_without_the_routers_hardware_address_arms_no_resume() {
    // a point-to-point VPN that holds the default route has no router to read
    let host = holding_host();
    host.set_network(VPN_NETWORK);
    let stub = github();
    let agent = host.agent(&stub.address, &["linuxone"]);
    let conn = Conn::open(&agent.roots).await;
    conn.until_runner_runs().await;
    set(&conn, "resume_after_stop", true).await;

    host.sleep();
    let stopped = conn.until_state("stopped").await;
    assert_eq!(
        resume_of(&stopped),
        (Some("no_gateway".into()), Some("slept".into()))
    );
    std::thread::sleep(Duration::from_secs(1));
    assert_eq!(conn.status().await.agent.state, "stopped");

    conn.ok("quit").await;
    let (code, stderr) = agent.finish(Duration::from_secs(30)).await;
    assert_eq!(code, Some(0), "{stderr}");
}

#[tokio::test(flavor = "current_thread")]
async fn on_axmc_a_hardware_address_first_read_after_the_runners_started_still_arms_a_resume() {
    // the router's address could not be read at the first reading and was read at a
    // later one of the same network
    let host = holding_host();
    host.set_network("interface=en0\nrouter=192.0.2.1\n");
    let stub = github();
    let agent = host.agent(&stub.address, &["linuxone"]);
    let conn = Conn::open(&agent.roots).await;
    conn.until_runner_runs().await;
    host.set_network(WIFI_NETWORK);
    // ten checks on the same network, now with its hardware address
    std::thread::sleep(Duration::from_secs(1));
    set(&conn, "resume_after_stop", true).await;

    host.sleep();
    let stopped = conn.until_state("stopped").await;
    assert_eq!(resume_of(&stopped), waiting_after("slept"));
    conn.until_state("running").await;
    conn.until_runner_runs().await;

    conn.ok("quit").await;
    let (code, stderr) = agent.finish(Duration::from_secs(30)).await;
    assert_eq!(code, Some(0), "{stderr}");
}

#[tokio::test(flavor = "current_thread")]
async fn on_axmc_the_sixth_signal_stop_within_an_hour_stops_with_resumed_too_often() {
    let host = holding_host();
    let stub = Stub::start((0..40).map(|_| registered()).collect::<Vec<Reply>>());
    let agent = host.agent(&stub.address, &["linuxone"]);
    let conn = Conn::open(&agent.roots).await;
    conn.until_runner_runs().await;
    set(&conn, "resume_after_stop", true).await;
    for _ in 0..5 {
        conn.until_runner_runs().await;
        host.sleep();
        conn.until("the sleep's stop", |snapshot| {
            snapshot.agent.state != "running"
        })
        .await;
        conn.until_state("running").await;
    }
    conn.until_runner_runs().await;
    host.sleep();
    let stopped = conn
        .until("resumed too often", |snapshot| {
            snapshot
                .agent
                .reason
                .as_ref()
                .is_some_and(|reason| reason.code == "resumed_too_often")
        })
        .await;
    let reason = stopped.agent.reason.clone().unwrap();
    assert!(reason.message.starts_with(SLEPT), "{}", reason.message);
    assert_eq!(resume_of(&stopped), (None, None));
    std::thread::sleep(Duration::from_secs(1));
    assert_eq!(conn.status().await.agent.state, "stopped");

    conn.ok("quit").await;
    let (code, stderr) = agent.finish(Duration::from_secs(30)).await;
    assert_eq!(code, Some(0), "{stderr}");
}

#[tokio::test(flavor = "current_thread")]
async fn on_axmc_a_sleep_during_a_drain_ends_the_agent_for_the_update_and_arms_no_resume() {
    signal_during_a_drain(MacHost::sleep, "slept").await;
}

#[tokio::test(flavor = "current_thread")]
async fn on_axmc_a_network_change_during_a_drain_ends_the_agent_for_the_update_and_arms_no_resume()
{
    signal_during_a_drain(
        |host| host.set_network(OTHER_WIFI_NETWORK),
        "network_changed",
    )
    .await;
}

/// the Tart backend reads no job activity here, so the drain waits for the runner, and
/// the signal stop ends the agent for the update with its reason `code` kept, so the new
/// app sends no `start_all`; the agent ends, so no resume is left armed.
async fn signal_during_a_drain(signal: impl FnOnce(&MacHost), code: &str) {
    let host = holding_host();
    let stub = github();
    let agent = host.agent(&stub.address, &["linuxone"]);
    let mut conn = Conn::open(&agent.roots).await;
    conn.until_runner_runs().await;
    set(&conn, "resume_after_stop", true).await;
    conn.call("drain_for_update", json!({ "version": "1.4.0" }))
        .await
        .unwrap();
    signal(&host);
    loop {
        if let Push::Ending { reason, update, .. } = conn.push().await {
            assert_eq!(reason, UPDATE);
            assert_eq!(update, Some(UpdateEnding::with_runners("1.4.0", code)));
            break;
        }
    }
    let (exit, stderr) = agent.finish(Duration::from_secs(30)).await;
    assert_eq!(exit, Some(0), "{stderr}");
    assert_no_job_vm(&host);
}
