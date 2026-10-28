//! one owner per diagnostic container, and the `diagnostics` request, through the
//! instance, its runtime core and the fake backend together.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod support;

use std::rc::Rc;
use std::time::Duration;

use marshal_cli::instance::fake::FakeSource;
use marshal_cli::instance::lifecycle::{Ended, Instance, Settings};
use marshal_cli::instance::protocol::{Outcome, Request, Response};
use marshal_cli::instance::runtime_core::RuntimeCore;
use marshal_cli::profile::Profile;
use marshal_cli::runtime::backend::{BackendError, DiagnosticRecord, ExportOutcome};
use marshal_cli::runtime::clock::SystemClock;
use marshal_cli::runtime::fake::{Call, FakeBackend, FakeRegistrar, MemoryLog};
use marshal_cli::validate::{Plan, validate};
use support::TempDir;
use support::instance::{CONFIG_PATH, Control, drive, resume};
use tokio::sync::Notify;
use tokio::time::Instant;

/// one pool, `d`, of one slot, with diagnostics on into `directory`.
fn plan(scratch: &TempDir, directory: &str) -> Plan {
    let config = r#"{"imageName":"actions-runner:local","repositories":[{"owner":"o",
        "repository":"d","name":"d","slots":1,"tokenPath":"C:\\d.token","volumes":[],
        "diagnostics":true,"diagnosticDirectory":"C:\\evidence"}]}"#;
    let mut plan = validate(config.as_bytes(), Profile::Axpc, None)
        .outcome
        .unwrap();
    plan.entries[0].token_path = scratch.path().join("token").to_str().unwrap().to_string();
    plan.entries[0].diagnostic_directory = Some(directory.to_string());
    plan
}

/// [`plan`], with a second pool, `other`, whose diagnostics are off.
fn plan_with_other(scratch: &TempDir, directory: &str) -> Plan {
    let config = r#"{"imageName":"actions-runner:local","repositories":[{"owner":"o",
        "repository":"d","name":"d","slots":1,"tokenPath":"C:\\d.token","volumes":[],
        "diagnostics":true,"diagnosticDirectory":"C:\\evidence"},{"owner":"o",
        "repository":"other","name":"other","slots":1,"tokenPath":"C:\\other.token",
        "volumes":[]}]}"#;
    let mut plan = validate(config.as_bytes(), Profile::Axpc, None)
        .outcome
        .unwrap();
    for entry in &mut plan.entries {
        entry.token_path = scratch.path().join("token").to_str().unwrap().to_string();
    }
    plan.entries[0].diagnostic_directory = Some(directory.to_string());
    plan
}

const LEFT_IN_OTHER: &str = "other-1-20240305060708009";

fn record(directory: &str) -> DiagnosticRecord {
    DiagnosticRecord {
        pool: "d".to_string(),
        directory: directory.to_string(),
        raw_records: false,
    }
}

const OLD: &str = r"C:\old";
const NEW: &str = r"C:\new";

/// the seams an instance runs on, with the system's clock, so a slot whose
/// container exits non-zero waits out a real backoff and stays idle until the stop.
struct World {
    scratch: TempDir,
    backend: FakeBackend,
    registrar: FakeRegistrar,
    log: MemoryLog,
    source: FakeSource,
}

impl World {
    fn new() -> Self {
        let scratch = TempDir::new();
        scratch.write_token("token", b"ghp_ownershiptoken\n");
        let source = FakeSource::new(plan(&scratch, OLD));
        let backend = FakeBackend::default();
        backend
            .state
            .borrow_mut()
            .exits
            .extend((0..100).map(|_| Ok(1)));
        Self {
            scratch,
            backend,
            registrar: FakeRegistrar::default(),
            log: MemoryLog::default(),
            source,
        }
    }

    fn plan(&self, directory: &str) -> Plan {
        plan(&self.scratch, directory)
    }

    async fn run<F, Fut, T>(&self, script: F) -> (Ended, T)
    where
        F: FnOnce(Control) -> Fut,
        Fut: std::future::Future<Output = T>,
    {
        self.run_from(self.plan(OLD), script).await
    }

    /// leaves [`LEFT_IN_OTHER`] in the engine and runs from [`plan_with_other`], so
    /// the preparation leaves that container to the manual recovery.
    async fn run_with_other<F, Fut, T>(&self, script: F) -> (Ended, T)
    where
        F: FnOnce(Control) -> Fut,
        Fut: std::future::Future<Output = T>,
    {
        self.backend.state.borrow_mut().containers.push(
            marshal_cli::runtime::fake::FakeContainer {
                name: LEFT_IN_OTHER.to_string(),
                diagnostic: true,
            },
        );
        self.run_from(plan_with_other(&self.scratch, OLD), script)
            .await
    }

    async fn run_from<F, Fut, T>(&self, plan: Plan, script: F) -> (Ended, T)
    where
        F: FnOnce(Control) -> Fut,
        Fut: std::future::Future<Output = T>,
    {
        let core = RuntimeCore::new(&self.backend, &SystemClock, &self.registrar, &self.log);
        let settings = Settings {
            config_path: CONFIG_PATH.to_string(),
            profile: Profile::Axpc,
        };
        let instance = Instance::new(
            settings,
            &self.backend,
            &SystemClock,
            &core,
            &self.source,
            &self.log,
        );
        drive(&instance, plan, Duration::from_secs(20), script).await
    }

    /// the containers whose export began, in order.
    fn exports(&self) -> Vec<String> {
        self.backend
            .calls()
            .into_iter()
            .filter_map(|call| match call {
                Call::ExportDiagnostic(name, _) => Some(name),
                _ => None,
            })
            .collect()
    }

    fn runs(&self) -> Vec<String> {
        self.backend
            .calls()
            .into_iter()
            .filter_map(|call| match call {
                Call::RunContainer(spec) => Some(spec.name),
                _ => None,
            })
            .collect()
    }

    fn bundles(&self) -> Vec<(String, DiagnosticRecord)> {
        self.backend.state.borrow().bundles.clone()
    }

    /// waits, giving the instance turns, until `condition` holds.
    async fn until(&self, what: &str, condition: impl Fn(&Self) -> bool) {
        let started = Instant::now();
        while !condition(self) {
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "never: {what}; calls: {:?}",
                self.backend.calls()
            );
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }

    /// holds every export until `gate` gives a permit, and tells `began` when one starts.
    fn gate_exports(&self) -> (Rc<Notify>, Rc<Notify>) {
        let (gate, began) = (Rc::new(Notify::new()), Rc::new(Notify::new()));
        let mut state = self.backend.state.borrow_mut();
        state.export_gate = Some(gate.clone());
        let told = began.clone();
        state.on_export = Some(Box::new(move || told.notify_one()));
        (gate, began)
    }
}

/// whether an answer has not arrived yet, after the instance had time to give it.
async fn still_waiting(answer: &mut tokio::sync::oneshot::Receiver<Response>) -> bool {
    tokio::time::sleep(Duration::from_millis(200)).await;
    answer.try_recv().is_err()
}

#[tokio::test(flavor = "current_thread")]
async fn a_stop_during_a_job_end_export_waits_for_it_and_one_bundle_results() {
    let world = World::new();
    let (gate, began) = world.gate_exports();
    let world = &world;
    let (ended, ()) = world
        .run(|control| async move {
            began.notified().await;
            assert_eq!(control.stop().await.outcome, Outcome::Ok);
            tokio::time::sleep(Duration::from_millis(200)).await;
            assert_eq!(control.state().await, "stopping");
            assert_eq!(world.exports().len(), 1);
            assert!(world.bundles().is_empty());
            gate.notify_one();
        })
        .await;
    let runs = world.runs();
    assert_eq!(runs.len(), 1);
    assert_eq!(world.exports(), runs);
    assert_eq!(world.bundles(), [(runs[0].clone(), record(OLD))]);
    assert!(world.backend.retained.names().is_empty());
    assert!(world.backend.state.borrow().containers.is_empty());
    assert!(ended.kept.is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn a_restart_during_a_job_end_export_waits_for_it_and_one_bundle_results() {
    let world = World::new();
    let (gate, began) = world.gate_exports();
    let world = &world;
    world
        .run(|control| async move {
            began.notified().await;
            let mut answer = control.send(Request::Restart);
            assert!(still_waiting(&mut answer).await, "the restart did not wait");
            assert_eq!(world.exports().len(), 1, "the restart took nothing");
            assert!(world.bundles().is_empty());
            gate.notify_one();
            world.backend.state.borrow_mut().export_gate = None;
            assert_eq!(answer.await.unwrap().outcome, Outcome::Ok);
            world
                .until("the second run is finalised", |world| {
                    world.exports().len() == 2
                })
                .await;
            control.stop().await;
        })
        .await;
    let runs = world.runs();
    assert_eq!(runs.len(), 2);
    assert_eq!(world.exports(), runs, "each container once");
    assert_eq!(world.bundles().len(), 2);
}

/// the container steps finalise it once the preparation's claim ends, so it is not
/// kept.
#[tokio::test(flavor = "current_thread")]
async fn a_stop_during_a_startup_export_finalises_the_container_the_preparation_left() {
    let world = World::new();
    let left = "d-1-20240305060708009";
    world
        .backend
        .state
        .borrow_mut()
        .containers
        .push(marshal_cli::runtime::fake::FakeContainer {
            name: left.to_string(),
            diagnostic: true,
        });
    let (gate, began) = world.gate_exports();
    let world = &world;
    let (ended, ()) = world
        .run(|control| async move {
            began.notified().await;
            assert_eq!(control.stop().await.outcome, Outcome::Ok);
            world
                .until("the steps export again", |world| world.exports().len() == 2)
                .await;
            world.backend.state.borrow_mut().export_gate = None;
            gate.notify_one();
        })
        .await;
    assert_eq!(
        world.exports(),
        [left, left],
        "the preparation, then the steps"
    );
    assert_eq!(world.bundles(), [(left.to_string(), record(OLD))]);
    assert!(world.backend.retained.names().is_empty());
    assert!(world.backend.state.borrow().containers.is_empty());
    assert!(ended.kept.is_empty(), "{:?}", ended.kept);
    assert!(world.runs().is_empty());
}

/// a panic in the middle of a job-end export unwinds the core; the claim ends with
/// it, and the container steps of the core's failure finalise the container.
#[tokio::test(flavor = "current_thread")]
async fn a_container_whose_job_end_export_panics_is_finalised_by_the_following_container_steps() {
    let world = World::new();
    {
        let mut state = world.backend.state.borrow_mut();
        state.on_export = Some(Box::new(|| panic!("the export broke")));
    }
    let world = &world;
    world
        .run(|control| async move {
            world
                .until("the steps finalise the container", |world| {
                    !world.bundles().is_empty()
                })
                .await;
            control.stop().await;
        })
        .await;
    let runs = world.runs();
    assert_eq!(runs.len(), 1);
    assert_eq!(
        world.exports(),
        [runs[0].clone(), runs[0].clone()],
        "the panicked job end, then the steps"
    );
    assert_eq!(world.bundles(), [(runs[0].clone(), record(OLD))]);
    assert!(world.backend.retained.names().is_empty());
    assert!(
        world
            .backend
            .calls()
            .iter()
            .any(|call| matches!(call, Call::StopContainers(stop) if stop.names == runs))
    );
}

#[tokio::test(flavor = "current_thread")]
async fn diagnostics_skips_a_container_being_finalised_and_no_second_bundle_results() {
    let world = World::new();
    let (gate, began) = world.gate_exports();
    let world = &world;
    world
        .run(|control| async move {
            began.notified().await;
            let answer = control.ask(Request::Diagnostics).await;
            assert_eq!(answer.outcome, Outcome::Ok, "{answer:?}");
            assert_eq!(answer.field("result"), Some("removed"), "{answer:?}");
            assert_eq!(
                answer.field("pool"),
                Some("d: 0 bundles written, 10 of 10 remaining")
            );
            assert_eq!(answer.field("container"), None);
            assert_eq!(world.exports().len(), 1, "diagnostics took nothing");
            gate.notify_one();
            world
                .until("the job end removes its container", |world| {
                    world.backend.retained.names().is_empty()
                })
                .await;
            control.stop().await;
        })
        .await;
    assert_eq!(world.exports().len(), 1);
    assert_eq!(world.bundles().len(), 1);
}

/// the first container's export and removal fail at its job end and again at the
/// restart that leaves the instance stopped; after a resume with a changed
/// directory, the stop finalises it with the directory it ran with.
#[tokio::test(flavor = "current_thread")]
async fn a_resume_after_a_failed_removal_finalises_with_the_recorded_settings() {
    let world = World::new();
    {
        let mut state = world.backend.state.borrow_mut();
        state
            .export_results
            .extend([ExportOutcome::Failed, ExportOutcome::Failed]);
        state.remove_error = Some(BackendError::new("Failed to remove containers"));
    }
    let world = &world;
    let (ended, ()) = world
        .run(|control| async move {
            world
                .until("the job end tries the removal", |world| {
                    world
                        .backend
                        .calls()
                        .iter()
                        .any(|call| matches!(call, Call::RemoveContainers(_)))
                })
                .await;
            world
                .source
                .results
                .borrow_mut()
                .push_back(Err(vec!["not valid".to_string()]));
            let restart = control.ask(Request::Restart).await;
            assert_ne!(restart.outcome, Outcome::Ok, "{restart:?}");
            assert_eq!(control.state().await, "stopped");
            assert_eq!(world.exports().len(), 2);
            assert_eq!(world.backend.retained.names().len(), 1);

            world.backend.state.borrow_mut().remove_error = None;
            // the next container's name, which carries the time, differs from this one's
            tokio::time::sleep(Duration::from_millis(5)).await;
            world
                .source
                .results
                .borrow_mut()
                .push_back(Ok(world.plan(NEW)));
            let resumed = control.ask(resume(Profile::Axpc, CONFIG_PATH)).await;
            assert_eq!(resumed.outcome, Outcome::Ok, "{resumed:?}");
            world
                .until("the second run is finalised", |world| {
                    world.bundles().len() == 1
                })
                .await;
            control.stop().await;
        })
        .await;
    let runs = world.runs();
    assert_eq!(runs.len(), 2);
    assert_ne!(runs[0], runs[1]);
    assert_eq!(
        world.bundles(),
        [
            (runs[1].clone(), record(NEW)),
            (runs[0].clone(), record(OLD))
        ]
    );
    assert!(world.backend.retained.names().is_empty());
    assert!(ended.kept.is_empty());
}

/// a running job's container, ended by the `docker stop` of a stop's container
/// steps, yields exactly one bundle, within the steps' 24 s.
#[tokio::test(flavor = "current_thread")]
async fn a_container_the_stop_ends_yields_one_bundle_within_24_seconds() {
    let world = World::new();
    world.backend.state.borrow_mut().hold_runs = true;
    let world = &world;
    let (ended, (asked, answered)) = world
        .run(|control| async move {
            world
                .until("the job runs", |world| !world.runs().is_empty())
                .await;
            let asked = Instant::now();
            control.stop().await;
            (asked, Instant::now())
        })
        .await;
    let runs = world.runs();
    assert_eq!(world.exports(), runs);
    assert_eq!(world.bundles(), [(runs[0].clone(), record(OLD))]);
    let calls = world.backend.calls();
    let stop = calls
        .iter()
        .find_map(|call| match call {
            Call::StopContainers(stop) if stop.names == runs => Some(stop.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(stop.grace, Duration::from_secs(10));
    assert!(stop.budget <= Duration::from_secs(12));
    let export = calls
        .iter()
        .find_map(|call| match call {
            Call::ExportDiagnostic(_, deadline) => Some(*deadline),
            _ => None,
        })
        .unwrap();
    // the steps' clock started between the request and its answer
    assert!(export >= asked + Duration::from_secs(18));
    assert!(export <= answered + Duration::from_secs(18));
    assert!(ended.kept.is_empty());
}

/// a restart and a core failure each finalise the running container with the
/// settings it ran with, though the configuration changed them first.
#[tokio::test(flavor = "current_thread")]
async fn a_restart_finalises_the_running_container_with_the_settings_it_ran_with() {
    let world = World::new();
    world.backend.state.borrow_mut().hold_runs = true;
    let world = &world;
    world
        .run(|control| async move {
            world
                .until("the job runs", |world| world.runs().len() == 1)
                .await;
            world
                .source
                .results
                .borrow_mut()
                .push_back(Ok(world.plan(NEW)));
            assert_eq!(control.ask(Request::Restart).await.outcome, Outcome::Ok);
            world
                .until("the next job runs", |world| world.runs().len() == 2)
                .await;
            control.stop().await;
        })
        .await;
    let runs = world.runs();
    assert_eq!(
        world.bundles(),
        [
            (runs[0].clone(), record(OLD)),
            (runs[1].clone(), record(NEW))
        ]
    );
    assert_eq!(world.exports(), runs, "no job end took them");
}

#[tokio::test(flavor = "current_thread")]
async fn a_core_failure_finalises_the_container_with_the_settings_it_ran_with() {
    let world = World::new();
    {
        let mut state = world.backend.state.borrow_mut();
        state.hold_runs = true;
        state.on_run = Some(Box::new(|| panic!("the core broke")));
    }
    world
        .source
        .results
        .borrow_mut()
        .push_back(Ok(world.plan(NEW)));
    let world = &world;
    world
        .run(|control| async move {
            world
                .until("the failure's steps finalise the container", |world| {
                    !world.bundles().is_empty()
                })
                .await;
            control.stop().await;
        })
        .await;
    let runs = world.runs();
    assert_eq!(world.bundles(), [(runs[0].clone(), record(OLD))]);
    assert!(world.backend.retained.names().is_empty());
}

/// a second Ctrl+C during a job-end export ends it at once; the container is
/// ended with `docker kill` and kept, and the instance says so.
#[tokio::test(flavor = "current_thread")]
async fn a_forced_stop_during_an_export_kills_and_keeps_the_container() {
    let world = World::new();
    let (_gate, began) = world.gate_exports();
    let (ended, ()) = world
        .run(|control| async move {
            began.notified().await;
            let answer = control.send(Request::Stop);
            tokio::time::sleep(Duration::from_millis(50)).await;
            control.force_removal();
            answer.await.unwrap();
        })
        .await;
    let runs = world.runs();
    assert!(world.bundles().is_empty());
    assert_eq!(ended.kept, runs);
    assert!(
        world
            .backend
            .calls()
            .contains(&Call::KillContainers(runs.clone()))
    );
    assert_eq!(
        world.backend.state.borrow().containers.len(),
        1,
        "the container is kept"
    );
    assert!(world.log.lines().iter().any(|line| line.contains(&format!(
        "Forced stop: diagnostic container {} is ended with docker kill and kept",
        runs[0]
    ))));
}

fn failing_job_end(world: &World) {
    let mut state = world.backend.state.borrow_mut();
    state.export_results.push_back(ExportOutcome::Failed);
    state.remove_error = Some(BackendError::new("Failed to remove containers"));
}

async fn job_end_tried(world: &World) {
    world
        .until("the job end tries the removal", |world| {
            world
                .backend
                .calls()
                .iter()
                .any(|call| matches!(call, Call::RemoveContainers(_)))
        })
        .await;
}

fn fields(response: &Response, name: &str) -> Vec<String> {
    response
        .fields()
        .iter()
        .filter(|(field, _)| field == name)
        .map(|(_, value)| value.clone())
        .collect()
}

#[tokio::test(flavor = "current_thread")]
async fn diagnostics_exports_and_removes_a_container_whose_removal_failed() {
    let world = World::new();
    failing_job_end(&world);
    let world = &world;
    world
        .run(|control| async move {
            job_end_tried(world).await;
            let name = world.runs()[0].clone();

            let again = control.ask(Request::Diagnostics).await;
            assert_eq!(again.field("result"), Some("incomplete"), "{again:?}");
            assert_eq!(
                fields(&again, "pool"),
                ["d: 1 bundles written, 9 of 10 remaining"]
            );
            assert_eq!(
                fields(&again, "container"),
                [format!("{name}: removal failed; it stays retained")]
            );
            assert_eq!(world.backend.retained.names(), std::slice::from_ref(&name));

            world.backend.state.borrow_mut().remove_error = None;
            let removed = control.ask(Request::Diagnostics).await;
            assert_eq!(removed.field("result"), Some("removed"), "{removed:?}");
            assert_eq!(
                fields(&removed, "pool"),
                ["d: 0 bundles written, 9 of 10 remaining"]
            );
            assert!(fields(&removed, "container").is_empty());
            assert!(world.backend.retained.names().is_empty());
            control.stop().await;
        })
        .await;
    assert_eq!(world.bundles().len(), 1, "the retry wrote no second bundle");
    let stops = world
        .backend
        .calls()
        .iter()
        .filter(|call| matches!(call, Call::StopContainers(stop) if stop.grace == Duration::from_secs(10)))
        .count();
    assert_eq!(stops, 2);
}

#[tokio::test(flavor = "current_thread")]
async fn diagnostics_reports_a_container_left_to_the_manual_recovery_and_an_unknown_count() {
    let world = World::new();
    world
        .backend
        .state
        .borrow_mut()
        .bundle_counts
        .push_back(None);
    let world = &world;
    world
        .run_with_other(|control| async move {
            world
                .until("the job end removes its container", |world| {
                    world.bundles().len() == 1 && world.backend.retained.names().is_empty()
                })
                .await;
            let answer = control.ask(Request::Diagnostics).await;
            assert_eq!(answer.field("result"), Some("incomplete"), "{answer:?}");
            // the pool left to the manual recovery has no sink to count
            assert_eq!(
                fields(&answer, "pool"),
                [
                    "d: 0 bundles written, unknown of 10 remaining",
                    "other: 0 bundles written, unknown of 10 remaining"
                ]
            );
            assert_eq!(
                fields(&answer, "container"),
                [format!(
                    "{LEFT_IN_OTHER}: left to the manual recovery, because pool other has diagnostics off"
                )]
            );
            control.stop().await;
        })
        .await;
}

/// a `restart` whose configuration no longer has the pool forgets the container
/// that pool's preparation left to the manual recovery.
#[tokio::test(flavor = "current_thread")]
async fn a_container_left_to_the_manual_recovery_is_forgotten_with_its_pool() {
    let world = World::new();
    let world = &world;
    world
        .run_with_other(|control| async move {
            world
                .until("the preparation leaves the container", |world| {
                    !world.backend.retained.unmanaged().is_empty()
                })
                .await;
            let before = control.ask(Request::Diagnostics).await;
            assert_eq!(
                fields(&before, "container"),
                [format!(
                    "{LEFT_IN_OTHER}: left to the manual recovery, because pool other has diagnostics off"
                )]
            );
            world
                .source
                .results
                .borrow_mut()
                .push_back(Ok(world.plan(OLD)));
            assert_eq!(control.ask(Request::Restart).await.outcome, Outcome::Ok);
            let after = control.ask(Request::Diagnostics).await;
            assert!(fields(&after, "container").is_empty(), "{after:?}");
            assert!(
                !fields(&after, "pool")
                    .iter()
                    .any(|line| line.starts_with("other:")),
                "{after:?}"
            );
            control.stop().await;
        })
        .await;
    assert!(world.backend.retained.unmanaged().is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn diagnostics_is_allowed_while_stopped_and_refused_while_stopping_or_resuming() {
    let world = World::new();
    failing_job_end(&world);
    let read = Rc::new(Notify::new());
    let world = &world;
    world
        .run(|control| async move {
            job_end_tried(world).await;
            world
                .source
                .results
                .borrow_mut()
                .push_back(Err(vec!["not valid".to_string()]));
            control.ask(Request::Restart).await;
            assert_eq!(control.state().await, "stopped");
            world.backend.state.borrow_mut().remove_error = None;
            let stopped = control.ask(Request::Diagnostics).await;
            assert_eq!(stopped.field("result"), Some("removed"), "{stopped:?}");
            assert!(world.backend.retained.names().is_empty());

            *world.source.gate.borrow_mut() = Some(read.clone());
            let (gate, began) = world.gate_exports();
            let resumed = control.send(resume(Profile::Axpc, CONFIG_PATH));
            assert_eq!(control.state().await, "resuming");
            let refused = control.ask(Request::Diagnostics).await;
            assert_eq!(refused.outcome, Outcome::Refused, "{refused:?}");
            assert_eq!(refused.field("state"), Some("resuming"));
            read.notify_one();
            assert_eq!(resumed.await.unwrap().outcome, Outcome::Ok);

            began.notified().await;
            control.stop().await;
            let refused = control.ask(Request::Diagnostics).await;
            assert_eq!(refused.outcome, Outcome::Refused, "{refused:?}");
            assert_eq!(refused.field("state"), Some("stopping"));
            gate.notify_one();
        })
        .await;
}
