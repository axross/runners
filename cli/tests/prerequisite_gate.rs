//! where the instance asks the core to check the host prerequisites: before its first
//! run, and at every restart and resume, on the fake seams.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod support;

use marshal_cli::instance::fake::FakeCore;
use marshal_cli::instance::lifecycle::Ending;
use marshal_cli::instance::protocol::Outcome;
use marshal_cli::instance::protocol::Request;
use marshal_cli::profile::Profile;
use support::instance::{CONFIG_PATH, Rig};

const REFUSAL: &str = "Softnet was not found on PATH. Install Softnet 0.25.0 as the \"Installing \
                       Tart and Softnet\" section of the macOS Runner Host document describes.";

#[tokio::test(flavor = "current_thread")]
async fn a_refusal_before_the_first_run_ends_the_instance_without_running_the_core() {
    let rig = Rig::new();
    rig.core
        .prerequisite_checks
        .borrow_mut()
        .push_back(Err(REFUSAL.to_string()));
    let (ended, ()) = rig.drive(|_control| async {}).await;
    assert_eq!(ended.ending, Ending::PrerequisiteRefused);
    assert_eq!(rig.core.runs.get(), 0, "nothing was started");
    assert_eq!(rig.core.prerequisites_checked.get(), 1);
    assert!(
        rig.log
            .lines()
            .contains(&format!("WARN: The instance ends: {REFUSAL}")),
        "{:?}",
        rig.log.lines()
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_passing_check_before_the_first_run_runs_the_core_and_is_not_repeated_by_it() {
    let rig = Rig::new();
    let (ended, state) = rig
        .drive(|control| async move {
            let state = control.state().await;
            control.stop().await;
            state
        })
        .await;
    assert_eq!(state, "running");
    assert_eq!(ended.ending, Ending::Ordinary);
    assert_eq!(rig.core.runs.get(), 1);
    assert_eq!(rig.core.prerequisites_checked.get(), 1);
}

#[tokio::test(flavor = "current_thread")]
async fn a_refusal_at_a_restart_leaves_the_instance_stopped_with_the_refusal_as_its_reason() {
    let rig = Rig::new();
    rig.core
        .prerequisite_checks
        .borrow_mut()
        .extend([Ok(()), Err(REFUSAL.to_string())]);
    let (_, (restart, status)) = rig
        .drive(|control| async move {
            let restart = control.ask(Request::Restart).await;
            let status = control.ask(Request::Status).await;
            control.stop().await;
            (restart, status)
        })
        .await;
    assert_eq!(restart.outcome, Outcome::Refused);
    assert_eq!(
        restart.message,
        format!("the instance stays stopped: {REFUSAL}")
    );
    assert_eq!(status.field("state"), Some("stopped"));
    assert_eq!(status.field("reason"), Some(REFUSAL));
    assert_eq!(rig.core.runs.get(), 1, "the core is not run again");
    assert_eq!(
        rig.core.image_checked.get(),
        0,
        "the image is not checked for a host that cannot boot"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_refusal_at_a_resume_keeps_the_instance_stopped_and_a_later_pass_resumes_it() {
    let rig = Rig::new();
    for _ in 0..4 {
        rig.core.push(FakeCore::failure("worker task panicked"));
    }
    rig.core
        .prerequisite_checks
        .borrow_mut()
        .extend([Ok(()), Err(REFUSAL.to_string()), Ok(())]);
    let (_, (refused, status, resumed)) = rig
        .drive(|control| async move {
            control.wait_for_state("stopped").await;
            let request = || support::instance::resume(Profile::Axpc, CONFIG_PATH);
            let refused = control.ask(request()).await;
            let status = control.ask(Request::Status).await;
            let resumed = control.ask(request()).await;
            control.stop().await;
            (refused, status, resumed)
        })
        .await;
    assert_eq!(refused.outcome, Outcome::Refused);
    assert_eq!(status.field("state"), Some("stopped"));
    assert_eq!(status.field("reason"), Some(REFUSAL));
    assert_eq!(resumed.outcome, Outcome::Ok);
    assert_eq!(rig.core.prerequisites_checked.get(), 3);
}
