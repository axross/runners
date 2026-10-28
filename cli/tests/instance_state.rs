//! the instance's state model, driven through the fake backend, core and source.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod support;

use std::rc::Rc;
use std::time::Duration;

use marshal_cli::instance::fake::CoreStep;
use marshal_cli::instance::lifecycle::{Ending, StopTimes};
use marshal_cli::instance::protocol::{Outcome, Request};
use marshal_cli::profile::Profile;
use marshal_cli::runtime::backend::StopRequest;
use marshal_cli::runtime::fake::Call;
use support::instance::{CONFIG_PATH, Rig, resume};
use tokio::sync::Notify;

const JOB: &str = "r-1-20240305060708009";

fn invalid() -> Result<marshal_cli::validate::Plan, Vec<String>> {
    Err(vec![
        "repositories[0].slots: must be an integer from 1 to 64".to_string(),
        "imageName: required field is missing".to_string(),
    ])
}

#[tokio::test(flavor = "current_thread")]
async fn a_new_instance_is_running_and_status_reports_its_configuration() {
    let rig = Rig::new();
    let (ended, status) = rig
        .drive(|control| async move {
            let status = control.ask(Request::Status).await;
            control.stop().await;
            status
        })
        .await;
    assert_eq!(status.outcome, Outcome::Ok);
    assert_eq!(status.field("state"), Some("running"));
    assert_eq!(status.field("config"), Some(CONFIG_PATH));
    assert_eq!(status.field("profile"), Some("axpc"));
    assert_eq!(status.field("reason"), None);
    assert!(ended.containers_removed);
    assert_eq!(rig.core.runs.get(), 1);
}

#[tokio::test(flavor = "current_thread")]
async fn stop_ends_a_running_instance_after_its_containers_are_stopped_and_removed() {
    let rig = Rig::new();
    rig.backend.state.borrow_mut().running = vec![JOB.to_string()];
    let (ended, reply) = rig
        .drive(|control| async move { control.stop().await })
        .await;
    assert_eq!(reply.outcome, Outcome::Ok);
    assert!(ended.containers_removed);
    assert_eq!(
        rig.backend.calls(),
        [
            Call::StopContainers(StopRequest {
                names: vec![JOB.to_string()],
                grace: Duration::from_secs(15),
                budget: Duration::from_secs(20),
            }),
            Call::RemoveContainers(vec![JOB.to_string()]),
        ]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn stop_ends_a_stopped_instance_and_touches_no_container() {
    let rig = Rig::new();
    rig.source.results.borrow_mut().push_back(invalid());
    let (ended, ()) = rig
        .drive(|control| async move {
            control.ask(Request::Restart).await;
            assert_eq!(control.state().await, "stopped");
            assert_eq!(control.stop().await.outcome, Outcome::Ok);
        })
        .await;
    assert!(ended.containers_removed);
    assert!(rig.backend.calls().is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn stop_while_stopping_lets_the_stop_finish_and_does_not_resume() {
    let rig = Rig::new();
    let gate = Rc::new(Notify::new());
    *rig.core.end_gate.borrow_mut() = Some(gate.clone());
    rig.backend.state.borrow_mut().running = vec![JOB.to_string()];
    let (ended, restart) = rig
        .drive(|control| async move {
            let restart = control.send(Request::Restart);
            control.wait_for_state("stopping").await;
            assert_eq!(control.stop().await.outcome, Outcome::Ok);
            gate.notify_one();
            restart.await.unwrap()
        })
        .await;
    assert_eq!(
        restart.outcome,
        Outcome::Refused,
        "the restart did not go on"
    );
    assert!(ended.containers_removed);
    assert_eq!(rig.source.loads.get(), 0);
    assert_eq!(rig.core.runs.get(), 1);
}

#[tokio::test(flavor = "current_thread")]
async fn stop_while_resuming_ends_the_instance_without_waiting_for_the_configuration() {
    let rig = Rig::new();
    let gate = Rc::new(Notify::new());
    *rig.source.gate.borrow_mut() = Some(gate.clone());
    rig.source.results.borrow_mut().push_back(invalid());
    let (ended, ()) = rig
        .drive(|control| async move {
            gate.notify_one();
            control.ask(Request::Restart).await;
            assert_eq!(control.state().await, "stopped");
            // the gate is closed again, and nothing opens it
            let restart = control.send(Request::Restart);
            control.wait_for_state("resuming").await;
            assert_eq!(control.stop().await.outcome, Outcome::Ok);
            assert_eq!(restart.await.unwrap().outcome, Outcome::Refused);
        })
        .await;
    assert!(ended.containers_removed);
    assert_eq!(rig.source.loads.get(), 2);
}

#[tokio::test(flavor = "current_thread")]
async fn restart_and_resume_are_refused_while_stopping_and_nothing_is_read() {
    let rig = Rig::new();
    let gate = Rc::new(Notify::new());
    *rig.core.end_gate.borrow_mut() = Some(gate.clone());
    let core = &rig.core;
    let (_, restart) = rig
        .drive(|control| async move {
            let restart = control.send(Request::Restart);
            control.wait_for_state("stopping").await;
            for request in [Request::Restart, resume(Profile::Axpc, CONFIG_PATH)] {
                let refused = control.ask(request).await;
                assert_eq!(refused.outcome, Outcome::Refused);
                assert_eq!(refused.field("state"), Some("stopping"));
                assert_eq!(refused.message, "the instance is stopping");
            }
            *core.end_gate.borrow_mut() = None;
            gate.notify_one();
            let restart = restart.await.unwrap();
            control.stop().await;
            restart
        })
        .await;
    assert_eq!(restart.outcome, Outcome::Ok);
    assert_eq!(rig.source.loads.get(), 1, "only the first restart read it");
}

#[tokio::test(flavor = "current_thread")]
async fn restart_and_resume_are_refused_while_resuming_and_nothing_extra_is_read() {
    let rig = Rig::new();
    let gate = Rc::new(Notify::new());
    *rig.source.gate.borrow_mut() = Some(gate.clone());
    let (_, restart) = rig
        .drive(|control| async move {
            let restart = control.send(Request::Restart);
            control.wait_for_state("resuming").await;
            for request in [Request::Restart, resume(Profile::Axpc, CONFIG_PATH)] {
                let refused = control.ask(request).await;
                assert_eq!(refused.outcome, Outcome::Refused);
                assert_eq!(refused.field("state"), Some("resuming"));
            }
            gate.notify_one();
            let restart = restart.await.unwrap();
            control.stop().await;
            restart
        })
        .await;
    assert_eq!(restart.outcome, Outcome::Ok);
    assert_eq!(rig.source.loads.get(), 1);
}

#[tokio::test(flavor = "current_thread")]
async fn a_running_instance_refuses_a_resume_and_names_its_state() {
    let rig = Rig::new();
    let (_, refused) = rig
        .drive(|control| async move {
            let refused = control.ask(resume(Profile::Axpc, CONFIG_PATH)).await;
            control.stop().await;
            refused
        })
        .await;
    assert_eq!(refused.outcome, Outcome::Refused);
    assert_eq!(refused.field("state"), Some("running"));
    assert_eq!(rig.source.loads.get(), 0);
}

#[tokio::test(flavor = "current_thread")]
async fn a_resume_naming_another_path_or_profile_is_refused_and_reads_nothing() {
    let rig = Rig::new();
    rig.source.results.borrow_mut().push_back(invalid());
    let (_, resumed) = rig
        .drive(|control| async move {
            control.ask(Request::Restart).await;
            assert_eq!(control.state().await, "stopped");
            for request in [
                resume(Profile::Axpc, "/etc/marshal/other.json"),
                resume(Profile::Axpc, "/etc/marshal/host.json "),
                resume(Profile::Axpc, "../../etc/passwd"),
                resume(Profile::Axmc, CONFIG_PATH),
            ] {
                let refused = control.ask(request).await;
                assert_eq!(refused.outcome, Outcome::Refused);
                assert_eq!(control.state().await, "stopped");
            }
            let resumed = control.ask(resume(Profile::Axpc, CONFIG_PATH)).await;
            control.stop().await;
            resumed
        })
        .await;
    assert_eq!(resumed.outcome, Outcome::Ok);
    assert_eq!(resumed.field("state"), Some("running"));
    assert_eq!(
        rig.source.loads.get(),
        2,
        "the failed restart and the resume"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_resume_whose_configuration_fails_validation_leaves_the_instance_stopped() {
    let rig = Rig::new();
    rig.source.results.borrow_mut().push_back(invalid());
    rig.source
        .results
        .borrow_mut()
        .push_back(Err(vec!["imageName: bad".to_string()]));
    let (_, ()) = rig
        .drive(|control| async move {
            let restart = control.ask(Request::Restart).await;
            assert_eq!(restart.outcome, Outcome::Refused);
            let status = control.ask(Request::Status).await;
            assert_eq!(status.field("state"), Some("stopped"));
            let reason = status.field("reason").unwrap();
            assert!(
                reason.contains("slots: must be an integer from 1 to 64"),
                "{reason}"
            );
            assert!(
                reason.contains("imageName: required field is missing"),
                "{reason}"
            );

            let resumed = control.ask(resume(Profile::Axpc, CONFIG_PATH)).await;
            assert_eq!(resumed.outcome, Outcome::Refused);
            assert_eq!(resumed.field("state"), Some("stopped"));
            let status = control.ask(Request::Status).await;
            let reason = status.field("reason").unwrap();
            assert!(
                reason.contains("imageName: bad"),
                "the new reason is shown: {reason}"
            );
            assert!(
                !reason.contains("slots"),
                "the old reason is gone: {reason}"
            );

            let resumed = control.ask(resume(Profile::Axpc, CONFIG_PATH)).await;
            assert_eq!(resumed.outcome, Outcome::Ok);
            assert_eq!(control.state().await, "running");
            control.stop().await;
        })
        .await;
    assert_eq!(
        rig.core.runs.get(),
        2,
        "the core runs again only after the good resume"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn restart_stops_and_removes_the_containers_reads_the_configuration_and_runs_the_core_again()
{
    let rig = Rig::new();
    rig.backend.state.borrow_mut().running = vec![JOB.to_string()];
    let (_, restart) = rig
        .drive(|control| async move {
            let restart = control.ask(Request::Restart).await;
            assert_eq!(control.state().await, "running");
            control.stop().await;
            restart
        })
        .await;
    assert_eq!(restart.outcome, Outcome::Ok);
    assert_eq!(rig.source.loads.get(), 1);
    assert_eq!(rig.core.runs.get(), 2);
    assert_eq!(
        rig.backend.calls(),
        [
            Call::StopContainers(StopRequest {
                names: vec![JOB.to_string()],
                grace: Duration::from_secs(15),
                budget: Duration::from_secs(20),
            }),
            Call::RemoveContainers(vec![JOB.to_string()]),
        ]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn restart_of_a_stopped_instance_resumes_it_whatever_path_it_was_started_with() {
    let rig = Rig::new();
    rig.source.results.borrow_mut().push_back(invalid());
    let (_, restart) = rig
        .drive(|control| async move {
            control.ask(Request::Restart).await;
            assert_eq!(control.state().await, "stopped");
            let restart = control.ask(Request::Restart).await;
            control.stop().await;
            restart
        })
        .await;
    assert_eq!(restart.outcome, Outcome::Ok);
    assert_eq!(restart.field("state"), Some("running"));
}

#[tokio::test(flavor = "current_thread")]
async fn a_failed_container_removal_is_reported_in_how_the_instance_ended() {
    let rig = Rig::new();
    {
        let mut state = rig.backend.state.borrow_mut();
        state.running = vec![JOB.to_string()];
        state.remove_error = Some(marshal_cli::runtime::backend::BackendError::new("busy"));
    }
    let (ended, _) = rig
        .drive(|control| async move { control.stop().await })
        .await;
    assert!(!ended.containers_removed);
    assert!(
        rig.log
            .lines()
            .iter()
            .any(|line| line.contains("Removing the containers failed: busy"))
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_removal_that_failed_at_a_restart_is_reported_when_a_later_stop_removes_nothing() {
    let rig = Rig::new();
    {
        let mut state = rig.backend.state.borrow_mut();
        state.running = vec![JOB.to_string()];
        state.remove_error = Some(marshal_cli::runtime::backend::BackendError::new("busy"));
    }
    rig.source.results.borrow_mut().push_back(invalid());
    let (ended, ()) = rig
        .drive(|control| async move {
            control.ask(Request::Restart).await;
            assert_eq!(control.state().await, "stopped");
            assert_eq!(control.stop().await.outcome, Outcome::Ok);
        })
        .await;
    assert!(!ended.containers_removed);
    assert_eq!(
        rig.backend.calls().last(),
        Some(&Call::RemoveContainers(vec![JOB.to_string()])),
        "the stop from the stopped state touched no container"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_core_that_ends_without_a_stop_is_a_failure_that_restarts_it() {
    let rig = Rig::new();
    rig.core.push(CoreStep::EndWithoutStop);
    let (_, ()) = rig
        .drive(|control| async move {
            control.wait_for_state("running").await;
            control.stop().await;
        })
        .await;
    assert_eq!(rig.core.runs.get(), 2);
    assert_eq!(rig.clock.sleeps(), [Duration::from_secs(10)]);
    assert!(
        rig.log
            .lines()
            .iter()
            .any(|line| line.contains("ended without being asked to"))
    );
}

#[tokio::test(flavor = "current_thread")]
async fn status_shows_what_the_core_reports() {
    let rig = Rig::new();
    *rig.core.report.borrow_mut() = vec![
        ("pool", "r: running".to_string()),
        ("slot", "r slot 1: running container r-1-x".to_string()),
    ];
    let (_, status) = rig
        .drive(|control| async move {
            let status = control.ask(Request::Status).await;
            control.stop().await;
            status
        })
        .await;
    let fields: Vec<(&str, &str)> = status
        .fields()
        .iter()
        .map(|(name, value)| (name.as_str(), value.as_str()))
        .collect();
    assert_eq!(
        fields,
        [
            ("state", "running"),
            ("config", CONFIG_PATH),
            ("profile", "axpc"),
            ("pool", "r: running"),
            ("slot", "r slot 1: running container r-1-x"),
        ]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_forced_removal_abandons_a_stop_that_ignores_the_grace_period() {
    let rig = Rig::new();
    {
        let mut state = rig.backend.state.borrow_mut();
        state.running = vec![JOB.to_string()];
        state.stop_hangs = true;
    }
    let (ended, ()) = rig
        .drive(|control| async move {
            let stop = control.send(Request::Stop);
            control.wait_for_state("stopping").await;
            control.force_removal();
            stop.await.unwrap();
        })
        .await;
    assert!(ended.containers_removed);
    let calls = rig.backend.calls();
    assert!(
        matches!(calls.first(), Some(Call::StopContainers(_))),
        "{calls:?}"
    );
    assert_eq!(
        calls.get(1),
        Some(&Call::RemoveContainers(vec![JOB.to_string()]))
    );
    assert!(
        rig.log
            .lines()
            .iter()
            .any(|line| line.contains("grace period is skipped"))
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_stop_still_running_at_the_exit_deadline_is_ended_and_reported_as_not_removed() {
    let rig = Rig::new();
    {
        let mut state = rig.backend.state.borrow_mut();
        state.running = vec![JOB.to_string()];
        state.stop_hangs = true;
    }
    rig.times.set(StopTimes {
        exit_deadline: Duration::from_millis(200),
        ..StopTimes::default()
    });
    let (ended, reply) = rig
        .drive(|control| async move { control.stop().await })
        .await;
    assert_eq!(reply.outcome, Outcome::Ok);
    assert!(!ended.containers_removed, "the removal never ran");
    assert_eq!(rig.backend.calls().len(), 1, "only the stop was asked for");
    assert!(
        rig.log
            .lines()
            .iter()
            .any(|line| line.contains("did not finish within"))
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_core_that_ignores_the_stop_is_dropped_at_the_exit_deadline() {
    let rig = Rig::new();
    rig.core.push(CoreStep::IgnoreStop);
    rig.times.set(StopTimes {
        exit_deadline: Duration::from_millis(200),
        ..StopTimes::default()
    });
    let (ended, ()) = rig
        .drive(|control| async move {
            control.wait_for_state("running").await;
            control.stop().await;
        })
        .await;
    assert!(ended.containers_removed, "the container steps had finished");
    assert_eq!(ended.ending, Ending::Ordinary);
}
