//! the resident agent's signal stops and keep-awake hold through the Windows monitor,
//! on fake seams behind its real channel: a sleep stops every runner, a network change
//! only with `stop_on_network_change` on, the hold follows the state and `keep_awake`,
//! and an update keeps a signal's stop. an applied `config_edit` or `config_reload`
//! puts the file's `[lifecycle]` in effect for them.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod support;

use std::net::Ipv4Addr;
use std::time::Duration;

use marshal_cli::agent::Push;
use marshal_cli::agent::machine::{Launch, UPDATE};
use marshal_cli::agent::wire::UpdateEnding;
use marshal_cli::macos::network::NetworkReading;
use marshal_cli::runtime::fake::Call;
use marshal_cli::windows_host::network::{Kind, primary};
use serde_json::json;
use support::agent::{Conn, ONE_POOL_TOML, Rig, TOKEN};

/// the rig's file with `lifecycle` written by hand after the pools.
fn hand_edited(lifecycle: &str) -> String {
    format!("{ONE_POOL_TOML}\n[lifecycle]\n{lifecycle}")
}

fn rig(lifecycle: &str) -> Rig {
    let rig = Rig::new("");
    rig.remove("config.json");
    rig.write_toml(&hand_edited(lifecycle));
    rig.store_token("o", "r", TOKEN);
    rig.hold_jobs();
    rig
}

fn network(kind: Kind, index: u32, router: u8) -> NetworkReading {
    NetworkReading::Primary(primary(
        kind,
        index,
        Some(Ipv4Addr::new(192, 0, 2, router)),
        Some([2, 0, 0, 0, 0, router]),
    ))
}

fn stops(rig: &Rig) -> usize {
    rig.backend
        .calls()
        .into_iter()
        .filter(|call| matches!(call, Call::StopContainers(_)))
        .count()
}

#[tokio::test(flavor = "current_thread")]
async fn a_sleep_stops_every_runner_and_ends_the_hold() {
    let rig = rig("");
    let rig = &rig;
    rig.drive(Launch::Running, |conn| async move {
        conn.until_runner_runs().await;
        rig.mark_running();
        assert!(
            rig.signals.is_held(),
            "keep-awake is not held while running"
        );

        rig.signals.sleep();
        let snapshot = conn.until_state("stopped").await;
        let reason = snapshot.agent.reason.unwrap();
        assert_eq!(reason.code, "slept");
        assert!(
            reason
                .message
                .contains("slept, so every runner was stopped")
        );
        assert!(stops(rig) > 0, "the runners were not stopped");
        assert!(!rig.signals.is_held(), "keep-awake is held while stopped");
        conn.ok("quit").await;
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_network_change_on_two_checks_stops_every_runner_and_names_both_interfaces() {
    let rig = rig("");
    rig.signals.set_network(network(Kind::WiFi, 12, 1));
    let rig = &rig;
    rig.drive(Launch::Running, |conn| async move {
        conn.until_runner_runs().await;
        rig.mark_running();
        rig.signals.set_network(network(Kind::Ethernet, 3, 2));
        let snapshot = conn.until_state("stopped").await;
        let reason = snapshot.agent.reason.unwrap();
        assert_eq!(reason.code, "network_changed");
        assert!(
            reason.message.contains("(Wi-Fi 12 to Ethernet 3)"),
            "{}",
            reason.message
        );
        conn.ok("quit").await;
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn with_the_network_stop_off_a_change_leaves_the_runners_running() {
    let rig = rig("stop_on_network_change = false\n");
    rig.signals.set_network(network(Kind::WiFi, 12, 1));
    let rig = &rig;
    rig.drive(Launch::Running, |conn| async move {
        conn.until_runner_runs().await;
        rig.mark_running();
        rig.signals.set_network(network(Kind::Ethernet, 3, 2));
        // ten network checks pass at the rig's cadence
        tokio::time::sleep(Duration::from_millis(600)).await;
        assert_eq!(conn.status().await.agent.state, "running");
        // with resume off too, the network is not read at all
        assert_eq!(rig.signals.network_reads(), 0);
        conn.ok("quit").await;
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn with_keep_awake_off_the_request_is_never_taken() {
    let rig = rig("keep_awake = false\n");
    let rig = &rig;
    rig.drive(Launch::Running, |conn| async move {
        conn.until_runner_runs().await;
        rig.mark_running();
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(rig.signals.hold_calls(), 0);
        conn.ok("quit").await;
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_sleep_during_a_drain_ends_the_agent_for_the_update_with_the_sleep_kept() {
    let rig = rig("");
    rig.backend.state.borrow_mut().hold_runs_by_name = true;
    let rig = &rig;
    let (ended, ()) = rig
        .drive(Launch::Running, |mut conn| async move {
            conn.until_runner_runs().await;
            rig.mark_running();
            conn.call("drain_for_update", json!({ "version": "1.4.0" }))
                .await
                .unwrap();
            rig.signals.sleep();
            loop {
                if let Push::Ending { reason, update, .. } = conn.push().await {
                    assert_eq!(reason, UPDATE);
                    assert_eq!(update, Some(UpdateEnding::with_runners("1.4.0", "slept")));
                    break;
                }
            }
        })
        .await;
    let update = ended.update.unwrap();
    assert_eq!(update.runners, "slept");
    assert!(!update.stopped_by_you);
}

fn resume_of(
    snapshot: &marshal_cli::agent::snapshot::Snapshot,
) -> (Option<String>, Option<String>) {
    let lifecycle = snapshot.lifecycle.clone().unwrap();
    (lifecycle.resume, lifecycle.resume_cause)
}

#[tokio::test(flavor = "current_thread")]
async fn with_resume_on_the_runners_start_again_only_once_the_network_they_stopped_on_holds() {
    let rig = rig("resume_after_stop = true\n");
    rig.signals.set_network(network(Kind::WiFi, 12, 1));
    let rig = &rig;
    rig.drive(Launch::Running, |conn| async move {
        conn.until_runner_runs().await;
        rig.mark_running();
        // the sleep is judged at the next sample, before any network check
        rig.signals.sleep();
        rig.signals.set_network(network(Kind::Ethernet, 3, 2));
        let stopped = conn.until_state("stopped").await;
        assert_eq!(stopped.agent.reason.as_ref().unwrap().code, "slept");
        assert_eq!(
            resume_of(&stopped),
            (Some("waiting".into()), Some("slept".into()))
        );
        // ten checks on another network leave the runners stopped
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert_eq!(conn.status().await.agent.state, "stopped");
        let reads = rig.signals.network_reads();
        assert!(reads > 0, "the network is not read while a resume waits");

        rig.signals.set_network(network(Kind::WiFi, 12, 1));
        let running = conn.until_state("running").await;
        assert_eq!(resume_of(&running), (None, None));
        conn.ok("quit").await;
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn stop_all_cancels_a_waiting_resume_and_leaves_the_runners_stopped_by_you() {
    let rig = rig("resume_after_stop = true\n");
    rig.signals.set_network(network(Kind::WiFi, 12, 1));
    let rig = &rig;
    rig.drive(Launch::Running, |conn| async move {
        conn.until_runner_runs().await;
        rig.mark_running();
        rig.signals.sleep();
        rig.signals.set_network(network(Kind::Ethernet, 3, 2));
        conn.until_state("stopped").await;
        conn.ok("stop_all").await;
        let snapshot = conn.status().await;
        assert_eq!(snapshot.agent.reason.as_ref().unwrap().code, "by_you");
        assert_eq!(resume_of(&snapshot), (None, None));

        let reads = rig.signals.network_reads();
        rig.signals.set_network(network(Kind::WiFi, 12, 1));
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert_eq!(conn.status().await.agent.state, "stopped");
        assert_eq!(
            rig.signals.network_reads(),
            reads,
            "nothing is read any more"
        );
        conn.ok("quit").await;
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_network_change_arms_the_resume_and_build_cancels_it() {
    let rig = rig("resume_after_stop = true\n");
    rig.signals.set_network(network(Kind::WiFi, 12, 1));
    let rig = &rig;
    rig.drive(Launch::Running, |conn| async move {
        conn.until_runner_runs().await;
        rig.mark_running();
        rig.signals.set_network(network(Kind::Ethernet, 3, 2));
        let stopped = conn.until_state("stopped").await;
        assert_eq!(
            resume_of(&stopped),
            (Some("waiting".into()), Some("network_changed".into()))
        );
        assert_eq!(conn.ok("build").await["state"], "building");
        conn.until("the build's end", |snapshot| {
            snapshot.agent.state == "stopped" && snapshot.agent.build.is_some()
        })
        .await;
        rig.signals.set_network(network(Kind::WiFi, 12, 1));
        tokio::time::sleep(Duration::from_millis(300)).await;
        let snapshot = conn.status().await;
        assert_eq!(snapshot.agent.state, "stopped");
        assert_eq!(resume_of(&snapshot), (None, None));
        conn.ok("quit").await;
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_users_start_all_cancels_a_waiting_resume() {
    let rig = rig("resume_after_stop = true\n");
    rig.signals.set_network(network(Kind::WiFi, 12, 1));
    let rig = &rig;
    rig.drive(Launch::Running, |conn| async move {
        conn.until_runner_runs().await;
        rig.mark_running();
        rig.signals.sleep();
        rig.signals.set_network(network(Kind::Ethernet, 3, 2));
        conn.until_state("stopped").await;
        conn.ok("start_all").await;
        let running = conn.until_state("running").await;
        assert_eq!(resume_of(&running), (None, None));
        conn.ok("quit").await;
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn turning_resume_off_cancels_a_waiting_resume() {
    let rig = rig("resume_after_stop = true\n");
    rig.signals.set_network(network(Kind::WiFi, 12, 1));
    let rig = &rig;
    rig.drive(Launch::Running, |conn| async move {
        conn.until_runner_runs().await;
        rig.mark_running();
        rig.signals.sleep();
        rig.signals.set_network(network(Kind::Ethernet, 3, 2));
        conn.until_state("stopped").await;
        conn.call(
            "lifecycle_set",
            json!({ "key": "resume_after_stop", "value": false }),
        )
        .await
        .unwrap();
        rig.signals.set_network(network(Kind::WiFi, 12, 1));
        tokio::time::sleep(Duration::from_millis(300)).await;
        let snapshot = conn.status().await;
        assert_eq!(snapshot.agent.state, "stopped");
        assert_eq!(snapshot.agent.reason.as_ref().unwrap().code, "slept");
        assert_eq!(resume_of(&snapshot), (None, None));
        conn.ok("quit").await;
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn an_applied_reload_that_turns_the_network_stop_off_leaves_a_network_change_alone() {
    let rig = rig("resume_after_stop = false\n");
    rig.signals.set_network(network(Kind::WiFi, 12, 1));
    let rig = &rig;
    rig.drive(Launch::Running, |conn| async move {
        conn.until_runner_runs().await;
        rig.mark_running();
        rig.write_toml(&hand_edited("stop_on_network_change = false\n"));
        conn.reload_config(false).await.unwrap();
        rig.signals.set_network(network(Kind::Ethernet, 3, 2));
        tokio::time::sleep(Duration::from_millis(600)).await;
        assert_eq!(conn.status().await.agent.state, "running");
        conn.ok("quit").await;
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn an_applied_reload_that_turns_keep_awake_off_ends_the_hold_and_one_that_turns_it_on_takes_it()
 {
    let rig = rig("keep_awake = false\n");
    let rig = &rig;
    rig.drive(Launch::Running, |conn| async move {
        conn.until_runner_runs().await;
        rig.mark_running();
        assert!(!rig.signals.is_held());
        rig.write_toml(&hand_edited("keep_awake = true\n"));
        conn.reload_config(false).await.unwrap();
        conn.eventually("the hold taken", || rig.signals.is_held())
            .await;
        rig.write_toml(&hand_edited("keep_awake = false\n"));
        conn.reload_config(false).await.unwrap();
        conn.eventually("the hold ended", || !rig.signals.is_held())
            .await;
        conn.ok("quit").await;
    })
    .await;
}

/// sleeps the runners of a rig that began on the Wi-Fi network, so a resume waits for it.
async fn waiting_to_resume(conn: &Conn, rig: &Rig) {
    conn.until_runner_runs().await;
    rig.mark_running();
    rig.signals.sleep();
    rig.signals.set_network(network(Kind::Ethernet, 3, 2));
    let stopped = conn.until_state("stopped").await;
    assert_eq!(
        resume_of(&stopped),
        (Some("waiting".into()), Some("slept".into()))
    );
}

#[tokio::test(flavor = "current_thread")]
async fn an_applied_reload_that_turns_resume_off_cancels_a_waiting_resume() {
    let rig = rig("resume_after_stop = true\n");
    rig.signals.set_network(network(Kind::WiFi, 12, 1));
    let rig = &rig;
    rig.drive(Launch::Running, |conn| async move {
        waiting_to_resume(&conn, rig).await;
        rig.write_toml(&hand_edited("resume_after_stop = false\n"));
        conn.reload_config(false).await.unwrap();
        resume_stays_cancelled(&conn, rig).await;
        conn.ok("quit").await;
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn an_applied_edit_that_turns_resume_off_cancels_a_waiting_resume() {
    let rig = rig("resume_after_stop = true\n");
    rig.signals.set_network(network(Kind::WiFi, 12, 1));
    let rig = &rig;
    rig.drive(Launch::Running, |conn| async move {
        waiting_to_resume(&conn, rig).await;
        rig.write_toml(&hand_edited("resume_after_stop = false\n"));
        conn.edit_first_pool_runners(false).await.unwrap();
        resume_stays_cancelled(&conn, rig).await;
        conn.ok("quit").await;
    })
    .await;
}

/// the resume is gone from the snapshot, and the network they stopped on coming back
/// starts nothing.
async fn resume_stays_cancelled(conn: &Conn, rig: &Rig) {
    let snapshot = conn.status().await;
    assert_eq!(snapshot.agent.reason.as_ref().unwrap().code, "slept");
    assert_eq!(resume_of(&snapshot), (None, None));
    rig.signals.set_network(network(Kind::WiFi, 12, 1));
    tokio::time::sleep(Duration::from_millis(300)).await;
    let snapshot = conn.status().await;
    assert_eq!(snapshot.agent.state, "stopped");
    assert_eq!(resume_of(&snapshot), (None, None));
}

#[tokio::test(flavor = "current_thread")]
async fn a_check_and_a_refused_reload_leave_a_waiting_resume_alone() {
    let rig = rig("resume_after_stop = true\n");
    rig.signals.set_network(network(Kind::WiFi, 12, 1));
    let rig = &rig;
    rig.drive(Launch::Running, |conn| async move {
        waiting_to_resume(&conn, rig).await;
        rig.write_toml(&hand_edited("resume_after_stop = false\n"));
        conn.reload_config(true).await.unwrap();
        let waiting = (Some("waiting".to_string()), Some("slept".to_string()));
        assert_eq!(resume_of(&conn.status().await), waiting);

        rig.write_toml(&hand_edited("resume_after_stop = 3\n"));
        let code = conn
            .code(
                "config_reload",
                json!({"revision": conn.config_revision().await}),
            )
            .await;
        assert!(code.starts_with("config_"), "{code}");
        assert_eq!(resume_of(&conn.status().await), waiting);
        conn.ok("quit").await;
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_network_without_a_gateway_hardware_address_arms_no_resume() {
    let rig = rig("resume_after_stop = true\n");
    rig.signals.set_network(NetworkReading::Primary(primary(
        Kind::Vpn,
        7,
        Some(Ipv4Addr::new(198, 51, 100, 1)),
        None,
    )));
    let rig = &rig;
    rig.drive(Launch::Running, |conn| async move {
        conn.until_runner_runs().await;
        rig.mark_running();
        rig.signals.sleep();
        let stopped = conn.until_state("stopped").await;
        assert_eq!(
            resume_of(&stopped),
            (Some("no_gateway".into()), Some("slept".into()))
        );
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert_eq!(conn.status().await.agent.state, "stopped");
        conn.ok("quit").await;
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn the_sixth_signal_stop_within_an_hour_of_five_resumes_stops_with_resumed_too_often() {
    let rig = rig("resume_after_stop = true\n");
    rig.signals.set_network(network(Kind::WiFi, 12, 1));
    let rig = &rig;
    rig.drive(Launch::Running, |conn| async move {
        for _ in 0..5 {
            conn.until_state("running").await;
            conn.until_runner_runs().await;
            rig.mark_running();
            rig.signals.sleep();
            conn.until("the sleep's stop", |snapshot| {
                snapshot.agent.state != "running"
            })
            .await;
        }
        conn.until_state("running").await;
        conn.until_runner_runs().await;
        rig.mark_running();
        rig.signals.sleep();
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
        assert!(reason.message.contains("slept"), "{}", reason.message);
        assert!(
            reason.message.contains("5 times within an hour"),
            "{}",
            reason.message
        );
        assert_eq!(resume_of(&stopped), (None, None));
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert_eq!(conn.status().await.agent.state, "stopped");
        conn.ok("quit").await;
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_drain_while_stopped_by_a_sleep_keeps_the_sleep_for_the_update() {
    let rig = rig("");
    let rig = &rig;
    let (ended, ()) = rig
        .drive(Launch::Running, |conn| async move {
            conn.until_runner_runs().await;
            rig.mark_running();
            rig.signals.sleep();
            conn.until_state("stopped").await;
            conn.call("drain_for_update", json!({ "version": "1.4.0" }))
                .await
                .unwrap();
        })
        .await;
    assert_eq!(ended.reason, UPDATE);
    assert_eq!(
        ended.update,
        Some(UpdateEnding::with_runners("1.4.0", "slept"))
    );
}
