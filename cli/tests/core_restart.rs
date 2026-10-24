//! The restart policy of the runtime core, on an injected clock.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod support;

use std::time::Duration;

use runners_cli::instance::fake::{CoreStep, FakeCore};
use runners_cli::instance::protocol::{Outcome, Request};
use runners_cli::profile::Profile;
use runners_cli::runtime::fake::Call;
use support::instance::{CONFIG_PATH, Control, Rig, resume};

const MINUTE: Duration = Duration::from_secs(60);

fn seconds(values: &[u64]) -> Vec<Duration> {
    values
        .iter()
        .map(|value| Duration::from_secs(*value))
        .collect()
}

/// Waits until the instance has no more to do on its own: it is stopped, or the
/// core has run `runs` times and is waiting for a stop.
async fn settle(control: &Control, core: &FakeCore, runs: usize) {
    for _ in 0..10_000 {
        if control.state().await == "stopped" || core.runs.get() >= runs {
            return;
        }
        tokio::task::yield_now().await;
    }
    panic!("the instance never settled");
}

#[tokio::test(flavor = "current_thread")]
async fn the_core_restarts_after_10_30_and_60_seconds_and_a_fourth_failure_stops_the_instance() {
    let rig = Rig::new();
    for _ in 0..4 {
        rig.core.push(FakeCore::failure("worker task panicked"));
    }
    let (_, status) = rig
        .drive(|control| async move {
            control.wait_for_state("stopped").await;
            let status = control.ask(Request::Status).await;
            control.stop().await;
            status
        })
        .await;
    assert_eq!(rig.clock.sleeps(), seconds(&[10, 30, 60]));
    assert_eq!(
        rig.core.runs.get(),
        4,
        "a stopped instance does not run the core"
    );
    assert_eq!(status.field("state"), Some("stopped"));
    assert_eq!(
        status.field("reason"),
        Some("the runtime core failed 4 times within 10 minutes")
    );
}

#[tokio::test(flavor = "current_thread")]
async fn each_failure_stops_and_removes_the_containers_the_failed_core_had_started() {
    let rig = Rig::new();
    rig.core.push(runners_cli::instance::fake::CoreStep::Fail {
        message: "boom".to_string(),
        containers: vec!["r-1-20240305060708009".to_string()],
        after: Duration::ZERO,
    });
    let core = &rig.core;
    let (_, ()) = rig
        .drive(|control| async move {
            settle(&control, core, 2).await;
            control.stop().await;
        })
        .await;
    let calls = rig.backend.calls();
    assert!(matches!(calls.first(), Some(Call::StopContainers(request))
        if request.names == ["r-1-20240305060708009"]));
    assert!(matches!(calls.get(1), Some(Call::RemoveContainers(names))
        if names == &["r-1-20240305060708009"]));
    assert_eq!(rig.core.runs.get(), 2);
}

#[tokio::test(flavor = "current_thread")]
async fn a_failure_older_than_ten_minutes_does_not_count() {
    let rig = Rig::new();
    for _ in 0..3 {
        rig.core.push(FakeCore::failure("boom"));
    }
    rig.core.push(CoreStep::Fail {
        message: "boom".to_string(),
        containers: Vec::new(),
        after: 11 * MINUTE,
    });
    let core = &rig.core;
    let (_, state) = rig
        .drive(|control| async move {
            settle(&control, core, 5).await;
            let state = control.state().await;
            control.stop().await;
            state
        })
        .await;
    assert_eq!(
        state, "running",
        "the fourth failure came after the others aged out"
    );
    assert_eq!(rig.clock.sleeps(), seconds(&[10, 30, 60, 10]));
    assert_eq!(rig.core.runs.get(), 5);
}

#[tokio::test(flavor = "current_thread")]
async fn a_fourth_failure_exactly_inside_the_window_still_stops_the_instance() {
    let rig = Rig::new();
    for _ in 0..3 {
        rig.core.push(FakeCore::failure("boom"));
    }
    // the first failure is at 0 s, and the third sleep ends at 100 s: this one fails
    // at 100 s + 499 s, which is 599 s after the first
    rig.core.push(CoreStep::Fail {
        message: "boom".to_string(),
        containers: Vec::new(),
        after: Duration::from_secs(499),
    });
    let (_, state) = rig
        .drive(|control| async move {
            control.wait_for_state("stopped").await;
            let state = control.state().await;
            control.stop().await;
            state
        })
        .await;
    assert_eq!(state, "stopped");
}

#[tokio::test(flavor = "current_thread")]
async fn resuming_clears_the_failure_count() {
    let rig = Rig::new();
    for _ in 0..4 {
        rig.core.push(FakeCore::failure("boom"));
    }
    for _ in 0..3 {
        rig.core.push(FakeCore::failure("boom"));
    }
    let core = &rig.core;
    let (_, state) = rig
        .drive(|control| async move {
            control.wait_for_state("stopped").await;
            let resumed = control.ask(resume(Profile::Axpc, CONFIG_PATH)).await;
            assert_eq!(resumed.outcome, Outcome::Ok);
            settle(&control, core, 8).await;
            let state = control.state().await;
            control.stop().await;
            state
        })
        .await;
    assert_eq!(
        state, "running",
        "three more failures are the first three again"
    );
    assert_eq!(
        rig.clock.sleeps(),
        seconds(&[10, 30, 60, 10, 30, 60]),
        "after the resume the delays start from 10 s"
    );
    assert_eq!(rig.core.runs.get(), 8);
}

#[tokio::test(flavor = "current_thread")]
async fn a_restart_request_clears_the_failure_count_too() {
    let rig = Rig::new();
    for _ in 0..2 {
        rig.core.push(FakeCore::failure("boom"));
    }
    rig.core.push(CoreStep::RunUntilStop);
    for _ in 0..3 {
        rig.core.push(FakeCore::failure("boom"));
    }
    let core = &rig.core;
    let (_, state) = rig
        .drive(|control| async move {
            settle(&control, core, 3).await;
            let restart = control.ask(Request::Restart).await;
            assert_eq!(restart.outcome, Outcome::Ok);
            settle(&control, core, 7).await;
            let state = control.state().await;
            control.stop().await;
            state
        })
        .await;
    assert_eq!(state, "running");
    assert_eq!(rig.clock.sleeps(), seconds(&[10, 30, 10, 30, 60]));
}
