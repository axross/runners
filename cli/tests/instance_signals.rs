//! the signals wired into the instance, driven through the fake signal source, the fake
//! backend, core and source and the fake clock: what stops every runner, what the
//! reasons say, how a resume starts afresh, the gate that holds back the first clone
//! and the keep-awake hold. Each test names the acceptance criterion of plan #74 it
//! covers.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod support;

use std::net::Ipv4Addr;
use std::rc::Rc;

use marshal_cli::instance::lifecycle::{Instance, Monitor};
use marshal_cli::instance::protocol::{Request, Response};
use marshal_cli::macos::fake::FakeSignals;
use marshal_cli::macos::gate::NetworkGate;
use marshal_cli::macos::monitor::{Cadence, SignalMonitor};
use marshal_cli::macos::network::{HardwareAddress, NetworkReading, PrimaryNetwork};
use marshal_cli::runtime::backend::{JobHost, StopRequest};
use marshal_cli::runtime::fake::{Call, HostCall};
use support::instance::{Control, Rig, drive};

const JOB: &str = "r-1-20240305060708009";
const SLEPT: &str = "the Mac slept, so every runner was stopped";

fn network(interface: &str, router: [u8; 4], hardware: Option<[u8; 6]>) -> NetworkReading {
    NetworkReading::Primary(PrimaryNetwork {
        interface: interface.to_string(),
        router: Some(Ipv4Addr::from(router)),
        hardware: hardware.map(HardwareAddress::new),
    })
}

fn wifi() -> NetworkReading {
    network("en0", [192, 0, 2, 1], Some([0x00, 0x00, 0x5e, 0, 0x53, 1]))
}

fn other_wifi() -> NetworkReading {
    network(
        "en0",
        [198, 51, 100, 1],
        Some([0x00, 0x00, 0x5e, 0, 0x53, 2]),
    )
}

fn vpn() -> NetworkReading {
    NetworkReading::Primary(PrimaryNetwork {
        interface: "utun4".to_string(),
        router: None,
        hardware: None,
    })
}

fn changed(from: &str, to: &str) -> String {
    format!("the primary network changed ({from} to {to}), so every runner was stopped")
}

fn prepared_host() -> JobHost {
    JobHost {
        pool: "r".to_string(),
        slot: 1,
    }
}

/// the rig, the signals the monitor reads, and the gate it keeps.
struct Signaled {
    rig: Rig,
    signals: Rc<FakeSignals>,
    gate: NetworkGate,
}

impl Signaled {
    fn new(readings: impl IntoIterator<Item = NetworkReading>) -> Self {
        let signals = Rc::new(FakeSignals::new());
        signals.script_network(readings);
        Self {
            rig: Rig::new(),
            signals,
            gate: NetworkGate::new(),
        }
    }

    fn monitor(
        &self,
    ) -> SignalMonitor<'_, Rc<FakeSignals>, Rc<marshal_cli::runtime::fake::FakeClock>> {
        SignalMonitor::new(
            self.signals.clone(),
            &self.rig.clock,
            &self.rig.log,
            self.gate.clone(),
            Cadence::default(),
        )
    }

    async fn run<F, Fut, T>(&self, script: F) -> (marshal_cli::instance::lifecycle::Ended, T)
    where
        F: FnOnce(Control) -> Fut,
        Fut: std::future::Future<Output = T>,
    {
        let monitor = self.monitor();
        let instance = Instance::new(
            Rig::settings(),
            &self.rig.backend,
            &self.rig.clock,
            &self.rig.core,
            &self.rig.source,
            &self.rig.log,
        )
        .with_monitor(&monitor)
        .with_stop_times(self.rig.times.get());
        let plan = marshal_cli::validate::validate(
            support::instance::CONFIG.as_bytes(),
            marshal_cli::profile::Profile::Axpc,
            None,
        )
        .outcome
        .unwrap();
        drive(&instance, plan, std::time::Duration::from_secs(10), script).await
    }

    fn log_has(&self, needle: &str) -> bool {
        self.rig
            .log
            .lines()
            .iter()
            .any(|line| line.contains(needle))
    }
}

/// gives the instance's tasks turns until `condition` holds.
async fn until(what: &str, condition: impl Fn() -> bool) {
    for _ in 0..100_000 {
        if condition() {
            return;
        }
        tokio::task::yield_now().await;
    }
    panic!("never saw {what}");
}

async fn status(control: &Control) -> Response {
    control.ask(Request::Status).await
}

/// waits until the instance runs its core and the monitor has taken its first reading,
/// so a signal the script raises afterwards is seen as one.
async fn armed(setup: &Signaled) {
    until("the first network reading", || {
        setup.signals.network_reads() >= 1
    })
    .await;
}

/// gives the monitor `reads` network reads, so a script can tell that a difference it
/// scripted was seen and judged.
async fn after_reads(signals: &FakeSignals, reads: u32) {
    until("the network reads", || signals.network_reads() >= reads).await;
    for _ in 0..50 {
        tokio::task::yield_now().await;
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_wake_stops_a_running_instance_with_its_reason_and_deletes_every_vm() {
    // AC: a wake moves a running instance to stopped with its reason and deletes every VM
    let setup = Signaled::new([wifi()]);
    let s = &setup;
    {
        let mut state = setup.rig.backend.state.borrow_mut();
        state.running = vec![JOB.to_string()];
        state.hosts = vec![prepared_host()];
    }
    let (ended, status) = setup
        .run(|control| async move {
            armed(s).await;
            s.signals.sleep();
            control.wait_for_state("stopped").await;
            let stopped = status(&control).await;
            control.stop().await;
            stopped
        })
        .await;
    assert_eq!(status.field("state"), Some("stopped"));
    assert_eq!(status.field("reason"), Some(SLEPT));
    assert!(ended.containers_removed);
    let calls = setup.rig.backend.calls();
    assert!(
        calls.iter().any(|call| matches!(
            call,
            Call::StopContainers(StopRequest { names, .. }) if names == &[JOB.to_string()]
        )),
        "{calls:?}"
    );
    assert!(calls.contains(&Call::RemoveContainers(vec![JOB.to_string()])));
    let host_calls = setup.rig.backend.state.borrow().host_calls.clone();
    assert!(
        host_calls
            .iter()
            .any(|call| matches!(call, HostCall::Release(host, _) if *host == prepared_host())),
        "{host_calls:?}"
    );
    assert!(setup.rig.backend.state.borrow().hosts.is_empty());
    assert!(setup.log_has(&format!("The instance is stopped: {SLEPT}.")));
}

#[tokio::test(flavor = "current_thread")]
async fn a_network_that_differs_on_two_checks_in_a_row_stops_every_runner_with_its_reason() {
    // AC: a primary-network difference on two checks in a row stops the instance with
    // its reason
    let setup = Signaled::new([wifi(), vpn(), vpn()]);
    let (_, status) = setup
        .run(|control| async move {
            control.wait_for_state("stopped").await;
            let stopped = status(&control).await;
            control.stop().await;
            stopped
        })
        .await;
    assert_eq!(
        status.field("reason"),
        Some(changed("en0", "utun4").as_str())
    );
    assert!(setup.log_has(&format!(
        "The instance is stopped: {}.",
        changed("en0", "utun4")
    )));
}

#[tokio::test(flavor = "current_thread")]
async fn a_difference_on_one_check_does_not_stop_the_instance() {
    // AC: a difference on one check does not stop the instance
    let setup = Signaled::new([wifi(), other_wifi(), wifi()]);
    let s = &setup;
    let (_, state) = setup
        .run(|control| async move {
            after_reads(&s.signals, 6).await;
            let state = control.state().await;
            control.stop().await;
            state
        })
        .await;
    assert_eq!(state, "running");
    assert!(setup.log_has("differs from its record"));
}

#[tokio::test(flavor = "current_thread")]
async fn the_same_interface_and_router_with_the_same_hardware_address_is_not_a_change() {
    // AC: the same interface and router with the same hardware address is not a change
    let setup = Signaled::new([wifi(), wifi(), wifi()]);
    let s = &setup;
    let (_, state) = setup
        .run(|control| async move {
            after_reads(&s.signals, 8).await;
            let state = control.state().await;
            control.stop().await;
            state
        })
        .await;
    assert_eq!(state, "running");
}

#[tokio::test(flavor = "current_thread")]
async fn a_missing_hardware_address_is_not_compared() {
    // AC: a missing hardware address is not compared
    let without = network("en0", [192, 0, 2, 1], None);
    let setup = Signaled::new([wifi(), without.clone(), without, wifi()]);
    let s = &setup;
    let (_, state) = setup
        .run(|control| async move {
            after_reads(&s.signals, 8).await;
            let state = control.state().await;
            control.stop().await;
            state
        })
        .await;
    assert_eq!(state, "running");
}

#[tokio::test(flavor = "current_thread")]
async fn a_vpn_taking_the_default_route_is_a_change() {
    // AC: a VPN's interface taking the default route is a change
    let setup = Signaled::new([wifi(), vpn(), vpn()]);
    let (_, reason) = setup
        .run(|control| async move {
            control.wait_for_state("stopped").await;
            let reason = status(&control).await.field("reason").map(str::to_string);
            control.stop().await;
            reason
        })
        .await;
    assert_eq!(reason, Some(changed("en0", "utun4")));
}

#[tokio::test(flavor = "current_thread")]
async fn a_routing_table_that_cannot_be_read_counts_as_no_default_route_and_names_none() {
    // AC: a routing table that cannot be read counts as no default route (a reading
    // that is `NoDefaultRoute`), and D14 names the new interface `none`
    let setup = Signaled::new([wifi(), NetworkReading::NoDefaultRoute]);
    let (_, reason) = setup
        .run(|control| async move {
            control.wait_for_state("stopped").await;
            let reason = status(&control).await.field("reason").map(str::to_string);
            control.stop().await;
            reason
        })
        .await;
    assert_eq!(reason, Some(changed("en0", "none")));
}

#[tokio::test(flavor = "current_thread")]
async fn a_stopped_instance_does_not_resume_on_its_own_and_only_a_restart_resumes_it() {
    // AC: only `start` or `restart` resumes
    let setup = Signaled::new([wifi()]);
    let s = &setup;
    let (_, (idle, resumed)) = setup
        .run(|control| async move {
            armed(s).await;
            s.signals.sleep();
            control.wait_for_state("stopped").await;
            for _ in 0..500 {
                tokio::task::yield_now().await;
            }
            let idle = (control.state().await, s.rig.core.runs.get());
            let resumed = control.ask(Request::Restart).await;
            control.wait_for_state("running").await;
            control.stop().await;
            (idle, resumed)
        })
        .await;
    assert_eq!(idle, ("stopped".to_string(), 1));
    assert_eq!(resumed.field("state"), Some("running"));
    assert_eq!(setup.rig.core.runs.get(), 2);
}

#[tokio::test(flavor = "current_thread")]
async fn a_resume_arms_the_sleep_baseline_again_so_a_sleep_while_stopped_is_not_a_stop() {
    // carry-over: re-arm the sleep baseline at each resume
    let setup = Signaled::new([wifi()]);
    let s = &setup;
    let (_, state) = setup
        .run(|control| async move {
            armed(s).await;
            s.signals.sleep();
            control.wait_for_state("stopped").await;
            assert_eq!(s.signals.rearm_calls(), 1);
            // the Mac sleeps while nothing samples the clocks
            s.signals.sleep();
            control.ask(Request::Restart).await;
            control.wait_for_state("running").await;
            let reads = s.signals.network_reads();
            after_reads(&s.signals, reads + 6).await;
            let state = control.state().await;
            control.stop().await;
            state
        })
        .await;
    assert_eq!(state, "running");
    assert_eq!(setup.signals.rearm_calls(), 2);
}

#[tokio::test(flavor = "current_thread")]
async fn a_resume_records_the_primary_network_afresh() {
    // carry-over: a new network watch at each resume; the record of the network that
    // changed is not kept
    let setup = Signaled::new([wifi(), vpn(), vpn()]);
    let s = &setup;
    let (_, state) = setup
        .run(|control| async move {
            control.wait_for_state("stopped").await;
            s.signals.set_network(vpn());
            control.ask(Request::Restart).await;
            control.wait_for_state("running").await;
            let reads = s.signals.network_reads();
            after_reads(&s.signals, reads + 8).await;
            let state = control.state().await;
            control.stop().await;
            state
        })
        .await;
    assert_eq!(state, "running");
}

#[tokio::test(flavor = "current_thread")]
async fn with_no_default_route_the_gate_stays_closed_until_a_network_has_held_for_two_checks() {
    // AC: with no default route at start, no VM is cloned until one has held for two
    // checks, and `status` shows that the instance is waiting for a network
    let setup = Signaled::new([
        NetworkReading::NoDefaultRoute,
        NetworkReading::NoDefaultRoute,
        wifi(),
        wifi(),
    ]);
    let s = &setup;
    let (_, (waiting, closed_at_three, open_at, after)) = setup
        .run(|control| async move {
            armed(s).await;
            let waiting = status(&control).await.field("network").map(str::to_string);
            until("the third reading", || s.signals.network_reads() >= 3).await;
            // the third reading is the first of the network: one check, not two
            let closed_at_three = !s.gate.is_open() && s.signals.network_reads() == 3;
            s.gate.wait_until_open().await;
            let open_at = s.signals.network_reads();
            let after = status(&control).await.field("network").map(str::to_string);
            control.stop().await;
            (waiting, closed_at_three, open_at, after)
        })
        .await;
    assert_eq!(
        waiting.as_deref(),
        Some("waiting for a network (no IPv4 default route)")
    );
    assert!(closed_at_three);
    assert_eq!(
        open_at, 4,
        "the gate opened at the second check of the network"
    );
    assert_eq!(after, None);
    assert!(setup.log_has("No IPv4 default route, so no VM is cloned"));
}

#[tokio::test(flavor = "current_thread")]
async fn a_network_at_start_opens_the_gate_at_once() {
    // AC: the gate is closed only without a default route at start
    let setup = Signaled::new([wifi()]);
    let monitor = setup.monitor();
    monitor.begin();
    assert!(
        !setup.gate.is_open(),
        "closed while the first reading is taken"
    );
    let watching = monitor.watch();
    tokio::select! {
        _ = watching => panic!("a steady network stops nothing"),
        () = setup.gate.wait_until_open() => {}
    }
    assert_eq!(setup.signals.network_reads(), 1);
    assert!(monitor.report().is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn the_keep_awake_hold_is_held_while_running_and_stopping_and_released_when_stopped() {
    // AC: the keep-awake hold is held while running or stopping and released when
    // stopped
    let setup = Signaled::new([wifi()]);
    let s = &setup;
    let gate = Rc::new(tokio::sync::Notify::new());
    *setup.rig.core.end_gate.borrow_mut() = Some(gate.clone());
    let (ended, held) = setup
        .run(|control| async move {
            control.wait_for_state("running").await;
            let running = s.signals.is_held();
            let stopping = control.send(Request::Stop);
            control.wait_for_state("stopping").await;
            let while_stopping = s.signals.is_held();
            gate.notify_one();
            stopping.await.unwrap();
            (running, while_stopping)
        })
        .await;
    assert_eq!(held, (true, true));
    assert!(!setup.signals.is_held(), "released when the instance ends");
    assert!(ended.containers_removed);
}

#[tokio::test(flavor = "current_thread")]
async fn the_hold_is_released_on_entering_stopped_and_taken_again_by_a_resume() {
    // AC: released when stopped; D7: a resume holds it again
    let setup = Signaled::new([wifi()]);
    let s = &setup;
    let (_, (stopped, resumed)) = setup
        .run(|control| async move {
            armed(s).await;
            s.signals.sleep();
            control.wait_for_state("stopped").await;
            let stopped = s.signals.is_held();
            let released = s.signals.release_calls();
            assert!(released >= 1);
            control.ask(Request::Restart).await;
            control.wait_for_state("running").await;
            let resumed = s.signals.is_held();
            control.stop().await;
            (stopped, resumed)
        })
        .await;
    assert!(!stopped);
    assert!(resumed);
}

#[tokio::test(flavor = "current_thread")]
async fn a_hold_that_cannot_be_taken_warns_in_the_log_and_status_and_is_retried() {
    // AC: a failed hold warns and is retried
    let setup = Signaled::new([wifi()]);
    let s = &setup;
    setup
        .signals
        .fail_keep_awake(Some("keep-awake is not held (scripted)"));
    let (_, (warned, recovered)) = setup
        .run(|control| async move {
            control.wait_for_state("running").await;
            let warned = status(&control)
                .await
                .fields()
                .iter()
                .any(|(name, value)| name == "warning" && value.contains("not held"));
            s.signals.fail_keep_awake(None);
            let mut recovered = false;
            for _ in 0..1000 {
                tokio::task::yield_now().await;
                recovered = status(&control).await.field("warning").is_none();
                if recovered {
                    break;
                }
            }
            control.stop().await;
            (warned, recovered)
        })
        .await;
    assert!(warned, "status shows the warning");
    assert!(recovered, "status drops it once the hold is held");
    assert!(setup.log_has("keep-awake is not held (scripted)"));
    assert!(setup.log_has("Keep-awake is held again."));
    assert!(setup.signals.hold_calls() >= 1);
}

#[tokio::test(flavor = "current_thread")]
async fn an_instance_without_a_monitor_asks_for_no_signals() {
    // axpc is unchanged: it builds its instance without a monitor
    let rig = Rig::new();
    let (_, state) = rig
        .drive(|control| async move {
            let state = control.state().await;
            control.stop().await;
            state
        })
        .await;
    assert_eq!(state, "running");
}
