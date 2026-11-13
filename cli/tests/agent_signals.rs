//! the resident agent's signal stops and keep-awake hold through the Windows monitor,
//! on fake seams behind its real channel: a sleep stops every runner, a network change
//! only with `stop_on_network_change` on, the hold follows the state and `keep_awake`,
//! and an update keeps a signal's stop.
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
use support::agent::{ONE_POOL_TOML, Rig, TOKEN};

fn rig(lifecycle: &str) -> Rig {
    let rig = Rig::new("");
    rig.remove("config.json");
    rig.write_toml(&format!("{ONE_POOL_TOML}\n[lifecycle]\n{lifecycle}"));
    rig.store_token("o", "r", TOKEN);
    rig.hold_jobs();
    rig
}

fn network(kind: Kind, index: u32, router: u8) -> NetworkReading {
    NetworkReading::Primary(primary(
        kind,
        index,
        Some(Ipv4Addr::new(192, 168, router, 1)),
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
