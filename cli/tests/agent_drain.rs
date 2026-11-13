//! the agent's drain for an update, on fake seams behind its real channel: nothing
//! registers, an idle runner is stopped, a busy one runs to its end, the bound, the
//! state table and the `ending` an update carries.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod support;

use std::time::Duration;

use marshal_cli::agent::Push;
use marshal_cli::agent::machine::{Launch, UPDATE};
use marshal_cli::agent::wire::UpdateEnding;
use marshal_cli::instance::lifecycle::StopTimes;
use marshal_cli::runtime::backend::Backend;
use marshal_cli::runtime::fake::Call;
use serde_json::json;
use support::agent::{Conn, ONE_POOL, Rig, TOKEN, TWO_POOLS};

const LISTENING: &str = "2026-11-12 08:00:01Z: Listening for Jobs";
const JOB: &str = "2026-11-12 08:03:00Z: Running job: build";
const RECONNECTED: &str = "2026-11-12 08:10:00Z: Runner reconnected.";

fn rig(config: &str) -> Rig {
    let rig = Rig::new(config);
    rig.store_token("o", "r", TOKEN);
    rig.store_token("o", "d", TOKEN);
    rig.hold_jobs();
    rig.backend.state.borrow_mut().hold_runs_by_name = true;
    rig
}

fn containers(rig: &Rig) -> Vec<String> {
    rig.backend
        .calls()
        .into_iter()
        .filter_map(|call| match call {
            Call::RunContainer(spec) => Some(spec.name),
            _ => None,
        })
        .collect()
}

fn stopped(rig: &Rig) -> Vec<Vec<String>> {
    rig.backend
        .calls()
        .into_iter()
        .filter_map(|call| match call {
            Call::StopContainers(request) => Some(request.names),
            _ => None,
        })
        .collect()
}

/// waits until `count` job containers have started, and returns their names.
async fn started(conn: &Conn, rig: &Rig, count: usize) -> Vec<String> {
    conn.eventually("the job containers", || containers(rig).len() >= count)
        .await;
    containers(rig)
}

async fn drain(conn: &Conn) -> serde_json::Value {
    conn.call("drain_for_update", json!({ "version": "1.4.0" }))
        .await
        .unwrap()
}

/// the `ending` this connection is told, skipping events.
async fn ending(conn: &mut Conn) -> (String, Option<UpdateEnding>) {
    loop {
        if let Push::Ending { reason, update, .. } = conn.push().await {
            return (reason, update);
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn an_idle_runner_is_stopped_and_a_busy_one_finishes_before_the_agent_ends() {
    let rig = rig(TWO_POOLS);
    let rig = &rig;
    let (ended, ()) = rig
        .drive(Launch::Running, |mut conn| async move {
            let names = started(&conn, rig, 2).await;
            let (idle, busy) = (&names[0], &names[1]);
            rig.backend.print(idle, LISTENING);
            rig.backend.print(idle, RECONNECTED);
            rig.backend.print(busy, LISTENING);
            rig.backend.print(busy, JOB);
            conn.client.subscribe().await.unwrap();

            let answer = drain(&conn).await;
            assert_eq!(answer["state"], "running");
            assert_eq!(answer["update"]["version"], "1.4.0");

            conn.eventually("the idle runner stopped", || {
                stopped(rig)
                    .iter()
                    .any(|names| names == &vec![idle.clone()])
            })
            .await;
            let snapshot = conn
                .until("one runner waited for", |s| {
                    s.agent.update.as_ref().is_some_and(|u| u.waiting == 1)
                })
                .await;
            let update = snapshot.agent.update.unwrap();
            assert_eq!(update.version, "1.4.0");
            assert_eq!(update.pid, std::process::id());
            assert_eq!(snapshot.agent.state, "running");
            assert!(
                !stopped(rig).iter().flatten().any(|name| name == busy),
                "the busy runner was stopped"
            );
            // nothing registers while the busy job runs
            tokio::time::sleep(Duration::from_millis(600)).await;
            assert_eq!(containers(rig).len(), 2);

            rig.end_jobs().await;
            let (reason, update) = ending(&mut conn).await;
            assert_eq!(reason, UPDATE);
            assert_eq!(update, Some(UpdateEnding::new("1.4.0", false)));
        })
        .await;
    assert_eq!(ended.reason, UPDATE);
    assert_eq!(ended.update, Some(UpdateEnding::new("1.4.0", false)));
    assert_eq!(
        containers(rig).len(),
        2,
        "a runner registered during the drain"
    );
    assert!(rig.lock_is_free());
}

#[tokio::test(flavor = "current_thread")]
async fn an_idle_runner_whose_stop_fails_is_killed() {
    let rig = rig(ONE_POOL);
    rig.backend.state.borrow_mut().stop_error = Some(
        marshal_cli::runtime::backend::BackendError::new("the engine refused"),
    );
    let rig = &rig;
    let (ended, ()) = rig
        .drive(Launch::Running, |mut conn| async move {
            let names = started(&conn, rig, 1).await;
            let idle = names[0].clone();
            rig.backend.print(&idle, LISTENING);
            drain(&conn).await;
            assert_eq!(ending(&mut conn).await.0, UPDATE);
            let calls = rig.backend.calls();
            let stop = calls
                .iter()
                .position(|call| matches!(call, Call::StopContainers(r) if r.names == vec![idle.clone()]))
                .expect("the idle runner was not stopped");
            assert!(
                calls[stop..]
                    .iter()
                    .any(|call| matches!(call, Call::KillContainers(names) if names == &vec![idle.clone()])),
                "the idle runner was not killed after its stop failed: {calls:?}"
            );
        })
        .await;
    assert_eq!(ended.reason, UPDATE);
    assert_eq!(
        containers(rig).len(),
        1,
        "a runner registered during the drain"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_runner_that_printed_neither_line_is_waited_for() {
    let rig = rig(ONE_POOL);
    let rig = &rig;
    let (ended, ()) = rig
        .drive(Launch::Running, |mut conn| async move {
            started(&conn, rig, 1).await;
            conn.client.subscribe().await.unwrap();
            drain(&conn).await;
            conn.until("one runner waited for", |s| {
                s.agent.update.as_ref().is_some_and(|u| u.waiting == 1)
            })
            .await;
            tokio::time::sleep(Duration::from_millis(600)).await;
            assert!(
                stopped(rig).is_empty(),
                "a runner of unknown state was stopped"
            );
            rig.end_jobs().await;
            assert_eq!(ending(&mut conn).await.0, UPDATE);
        })
        .await;
    assert_eq!(ended.reason, UPDATE);
}

#[tokio::test(flavor = "current_thread")]
async fn the_bound_stops_a_busy_runner_and_ends_the_agent() {
    let rig = rig(ONE_POOL);
    rig.drain_bound.set(Duration::from_millis(500));
    let rig = &rig;
    let (ended, ()) = rig
        .drive(Launch::Running, |mut conn| async move {
            let names = started(&conn, rig, 1).await;
            rig.backend.print(&names[0], JOB);
            rig.mark_running();
            conn.client.subscribe().await.unwrap();
            drain(&conn).await;
            let (reason, update) = ending(&mut conn).await;
            assert_eq!(reason, UPDATE);
            assert_eq!(update, Some(UpdateEnding::new("1.4.0", false)));
        })
        .await;
    assert_eq!(ended.reason, UPDATE);
    assert!(
        stopped(rig)
            .iter()
            .flatten()
            .any(|name| name == &containers(rig)[0]),
        "the busy runner was not stopped at the bound"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_drain_while_stopped_by_you_ends_at_once_and_keeps_stopped_by_you() {
    let rig = rig(ONE_POOL);
    let rig = &rig;
    let (ended, ()) = rig
        .drive(Launch::Stopped, |mut conn| async move {
            conn.client.subscribe().await.unwrap();
            let answer = drain(&conn).await;
            // the app waits on this process before it installs
            assert_eq!(answer["state"], "quitting");
            assert_eq!(answer["update"]["version"], "1.4.0");
            assert_eq!(answer["update"]["pid"], std::process::id());
            let (reason, update) = ending(&mut conn).await;
            assert_eq!(reason, UPDATE);
            assert_eq!(update, Some(UpdateEnding::new("1.4.0", true)));
        })
        .await;
    assert_eq!(ended.update, Some(UpdateEnding::new("1.4.0", true)));
    assert!(containers(rig).is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn during_a_drain_starts_are_refused_and_stop_all_ends_it_stopped_by_you() {
    let rig = rig(ONE_POOL);
    let rig = &rig;
    let (ended, ()) = rig
        .drive(Launch::Running, |mut conn| async move {
            let names = started(&conn, rig, 1).await;
            rig.backend.print(&names[0], JOB);
            rig.mark_running();
            conn.client.subscribe().await.unwrap();
            drain(&conn).await;
            for method in ["start_all", "build"] {
                assert_eq!(conn.code(method, json!(null)).await, "refused", "{method}");
            }
            assert_eq!(
                conn.code("drain_for_update", json!({ "version": "1.5.0" }))
                    .await,
                "refused"
            );
            assert_eq!(conn.ok("stop_all").await["state"], "quitting");
            let (reason, update) = ending(&mut conn).await;
            assert_eq!(reason, UPDATE);
            assert_eq!(update, Some(UpdateEnding::new("1.4.0", true)));
        })
        .await;
    assert_eq!(ended.update, Some(UpdateEnding::new("1.4.0", true)));
}

#[tokio::test(flavor = "current_thread")]
async fn a_drain_is_refused_while_the_agent_resumes() {
    let rig = rig(ONE_POOL);
    let gate = std::rc::Rc::new(tokio::sync::Notify::new());
    rig.backend.state.borrow_mut().available_gate = Some(gate.clone());
    let rig = &rig;
    let (_, ()) = rig
        .drive(Launch::Running, |conn| async move {
            conn.until_state("resuming").await;
            assert_eq!(
                conn.code("drain_for_update", json!({ "version": "1.4.0" }))
                    .await,
                "refused"
            );
            conn.ok("quit").await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn quit_during_a_drain_ends_the_agent_as_a_quit() {
    let rig = rig(ONE_POOL);
    rig.backend.state.borrow_mut().stop_hangs = true;
    rig.times.set(StopTimes {
        exit_deadline: Duration::from_secs(1),
        ..StopTimes::default()
    });
    let rig = &rig;
    let (ended, ()) = rig
        .drive(Launch::Running, |conn| async move {
            let names = started(&conn, rig, 1).await;
            rig.backend.print(&names[0], JOB);
            rig.mark_running();
            drain(&conn).await;
            // the stop hangs until the exit deadline, so the abandoned drain must have
            // left the snapshot while the agent still stops
            let other = conn.another().await;
            let quit = async { conn.ok("quit").await };
            let meanwhile = async {
                let snapshot = other.until_state("stopping").await;
                assert_eq!(snapshot.agent.update, None);
            };
            tokio::join!(quit, meanwhile);
        })
        .await;
    assert_eq!(ended.reason, "quit");
    assert_eq!(ended.update, None);
}

#[tokio::test(flavor = "current_thread")]
async fn the_drain_is_cancelled_when_its_connection_closes() {
    let rig = rig(TWO_POOLS);
    let rig = &rig;
    let (ended, ()) = rig
        .drive(Launch::Running, |conn| async move {
            let names = started(&conn, rig, 2).await;
            rig.backend.print(&names[0], LISTENING);
            // a busy runner keeps the drain open until the requester leaves
            rig.backend.print(&names[1], JOB);
            // the stopped idle runner exits as a stopped container does
            rig.backend.state.borrow_mut().exits.push_back(Ok(137));
            let requester = conn.another().await;
            drain(&requester).await;
            conn.eventually("the idle runner stopped", || !stopped(rig).is_empty())
                .await;
            drop(requester);
            conn.until("the drain cancelled", |s| s.agent.update.is_none())
                .await;
            conn.eventually("a runner registered again", || containers(rig).len() >= 3)
                .await;
            let lines = rig.log.lines().join("\n");
            assert!(
                !lines.contains("exited with code 137"),
                "the drain's own stop was taken for a failure: {lines}"
            );
            rig.mark_running();
            conn.ok("quit").await;
        })
        .await;
    assert_eq!(ended.reason, "quit");
}

/// `config` with diagnostics on for every pool; the fake engine never touches the sink.
fn with_diagnostics(config: &str) -> String {
    config.replace(
        r#""volumes":[]}"#,
        r#""volumes":[],"diagnostics":true,"diagnosticDirectory":"C:\\diag"}"#,
    )
}

fn exported(rig: &Rig, name: &str) -> bool {
    rig.backend
        .calls()
        .iter()
        .any(|call| matches!(call, Call::ExportDiagnostic(exported, _) if exported == name))
}

#[tokio::test(flavor = "current_thread")]
async fn a_diagnostic_job_that_finishes_in_a_drain_is_finalised_before_the_agent_ends() {
    let rig = rig(ONE_POOL);
    rig.write_json(&with_diagnostics(ONE_POOL));
    let rig = &rig;
    let (ended, name) = rig
        .drive(Launch::Running, |mut conn| async move {
            let names = started(&conn, rig, 1).await;
            rig.backend.print(&names[0], JOB);
            conn.client.subscribe().await.unwrap();
            drain(&conn).await;
            rig.end_jobs().await;
            assert_eq!(ending(&mut conn).await.0, UPDATE);
            names[0].clone()
        })
        .await;
    assert_eq!(ended.reason, UPDATE);
    assert!(exported(rig, &name), "the finished job was not exported");
    assert!(ended.kept.is_empty(), "{:?}", ended.kept);
}

#[tokio::test(flavor = "current_thread")]
async fn a_diagnostic_job_still_running_at_the_bound_is_finalised_before_the_agent_ends() {
    let rig = rig(ONE_POOL);
    rig.write_json(&with_diagnostics(ONE_POOL));
    rig.drain_bound.set(Duration::from_millis(500));
    let rig = &rig;
    let (ended, name) = rig
        .drive(Launch::Running, |mut conn| async move {
            let names = started(&conn, rig, 1).await;
            rig.backend.print(&names[0], JOB);
            conn.client.subscribe().await.unwrap();
            drain(&conn).await;
            assert_eq!(ending(&mut conn).await.0, UPDATE);
            names[0].clone()
        })
        .await;
    assert_eq!(ended.reason, UPDATE);
    assert!(
        exported(rig, &name),
        "the running job was not exported at the bound"
    );
    assert!(ended.kept.is_empty(), "{:?}", ended.kept);
}

/// the core fails during the drain (an export that panics as the idle runner's
/// container is finalised): the container steps run, the core is not started again,
/// and the agent ends for the update.
#[tokio::test(flavor = "current_thread")]
async fn a_core_failure_during_a_drain_runs_the_container_steps_and_starts_nothing_again() {
    let rig = rig(TWO_POOLS);
    rig.write_json(&with_diagnostics(TWO_POOLS));
    let rig = &rig;
    let (ended, busy) = rig
        .drive(Launch::Running, |mut conn| async move {
            let names = started(&conn, rig, 2).await;
            rig.backend.print(&names[0], LISTENING);
            rig.backend.print(&names[1], JOB);
            // only the first export fails, so the container steps after it can export
            let failed = std::rc::Rc::new(std::cell::Cell::new(false));
            rig.backend.state.borrow_mut().on_export = Some(Box::new(move || {
                if !failed.replace(true) {
                    panic!("the core broke during the drain");
                }
            }));
            conn.client.subscribe().await.unwrap();
            drain(&conn).await;
            let (reason, update) = ending(&mut conn).await;
            assert_eq!(reason, UPDATE);
            assert_eq!(update, Some(UpdateEnding::new("1.4.0", false)));
            names[1].clone()
        })
        .await;
    assert_eq!(ended.reason, UPDATE);
    assert_eq!(
        containers(rig).len(),
        2,
        "a runner registered after the failure"
    );
    let lines = rig.log.lines().join("\n");
    assert!(lines.contains("The runtime core failed"), "{lines}");
    assert!(!lines.contains("Restarting the runtime core"), "{lines}");
    assert!(
        exported(rig, &busy),
        "the busy runner's container was not finalised"
    );
    assert!(ended.kept.is_empty(), "{:?}", ended.kept);
}

/// fails the core at its first job container only, as a core that broke once.
fn break_the_core_once(rig: &Rig) {
    let broke = std::cell::Cell::new(false);
    rig.backend.state.borrow_mut().on_run = Some(Box::new(move || {
        if !broke.replace(true) {
            panic!("the core broke");
        }
    }));
}

fn waits_for_docker(rig: &Rig) -> usize {
    rig.backend
        .calls()
        .iter()
        .filter(|call| matches!(call, Call::WaitUntilAvailable))
        .count()
}

/// a drain that begins while a failed core waits to restart ends the agent for the
/// update without starting the core again.
#[tokio::test(flavor = "current_thread")]
async fn a_drain_during_a_cores_restart_wait_starts_nothing_again() {
    let rig = rig(ONE_POOL);
    rig.clock.pause.set(Duration::from_secs(30));
    break_the_core_once(&rig);
    let rig = &rig;
    let (ended, ()) = rig
        .drive(Launch::Running, |mut conn| async move {
            conn.until("a restart due", |s| s.agent.restart_due.is_some())
                .await;
            let before = containers(rig).len();
            conn.client.subscribe().await.unwrap();
            let asked = std::time::Instant::now();
            assert_eq!(drain(&conn).await["state"], "running");
            let (reason, update) = ending(&mut conn).await;
            // the drain cut the 30 s wait short
            assert!(
                asked.elapsed() < Duration::from_secs(10),
                "{:?}",
                asked.elapsed()
            );
            assert_eq!(reason, UPDATE);
            assert_eq!(update, Some(UpdateEnding::new("1.4.0", false)));
            assert_eq!(containers(rig).len(), before, "the core started again");
        })
        .await;
    assert_eq!(ended.reason, UPDATE);
}

/// a drain that begins while a restarted core waits for Docker drops that preparation,
/// so no runner registers, and the agent ends for the update.
#[tokio::test(flavor = "current_thread")]
async fn a_drain_while_a_restarted_core_waits_for_docker_registers_nothing() {
    let rig = rig(ONE_POOL);
    rig.clock.pause.set(Duration::from_millis(100));
    break_the_core_once(&rig);
    let gate = std::rc::Rc::new(tokio::sync::Notify::new());
    let rig = &rig;
    let (ended, ()) = rig
        .drive(Launch::Running, |mut conn| async move {
            conn.until("a restart due", |s| s.agent.restart_due.is_some())
                .await;
            let waits = waits_for_docker(rig);
            // Docker is down when the core starts again
            rig.backend.state.borrow_mut().available_gate = Some(gate.clone());
            conn.eventually("the restarted core waiting for Docker", || {
                waits_for_docker(rig) > waits
            })
            .await;
            let before = containers(rig).len();
            conn.client.subscribe().await.unwrap();
            drain(&conn).await;
            let (reason, update) = ending(&mut conn).await;
            assert_eq!(reason, UPDATE);
            assert_eq!(update, Some(UpdateEnding::new("1.4.0", false)));
            assert_eq!(containers(rig).len(), before, "a runner registered");
        })
        .await;
    assert_eq!(ended.reason, UPDATE);
}

/// a requester that closes while a failed core's container steps run, the drain's last
/// steps, does not cancel the drain or start the core again.
#[tokio::test(flavor = "current_thread")]
async fn a_requester_that_closes_after_a_core_failure_in_a_drain_cancels_nothing() {
    let rig = rig(TWO_POOLS);
    rig.write_json(&with_diagnostics(TWO_POOLS));
    let gate = std::rc::Rc::new(tokio::sync::Notify::new());
    let rig = &rig;
    let (ended, ()) = rig
        .drive(Launch::Running, |mut conn| async move {
            let names = started(&conn, rig, 2).await;
            rig.backend.print(&names[0], LISTENING);
            rig.backend.print(&names[1], JOB);
            // the idle runner's export breaks the core (the fake drops the hook as it
            // panics); the failure's container steps then wait on the gate
            rig.backend.state.borrow_mut().on_export = Some(Box::new(|| {
                panic!("the core broke during the drain");
            }));
            rig.backend.state.borrow_mut().export_gate = Some(gate.clone());
            conn.client.subscribe().await.unwrap();
            let requester = conn.another().await;
            drain(&requester).await;
            let busy = names[1].clone();
            conn.eventually("the failure's container steps", || exported(rig, &busy))
                .await;
            drop(requester);
            tokio::time::sleep(Duration::from_millis(300)).await;
            let release = async {
                loop {
                    gate.notify_waiters();
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
            };
            let (reason, update) = tokio::select! {
                end = ending(&mut conn) => end,
                () = release => unreachable!(),
            };
            assert_eq!(reason, UPDATE);
            assert_eq!(update, Some(UpdateEnding::new("1.4.0", false)));
        })
        .await;
    assert_eq!(ended.reason, UPDATE);
    let lines = rig.log.lines().join("\n");
    assert!(!lines.contains("starts again"), "{lines}");
    assert_eq!(
        containers(rig).len(),
        2,
        "a runner registered after the failure"
    );
}

/// a stop all that arrives while the drain's last container steps run, here held by a
/// diagnostic export at the bound, still keeps the runners stopped after the update.
#[tokio::test(flavor = "current_thread")]
async fn stop_all_during_the_drains_last_steps_keeps_stopped_by_you() {
    let rig = rig(ONE_POOL);
    rig.write_json(&with_diagnostics(ONE_POOL));
    rig.drain_bound.set(Duration::from_millis(300));
    let gate = std::rc::Rc::new(tokio::sync::Notify::new());
    let rig = &rig;
    let (ended, ()) = rig
        .drive(Launch::Running, |mut conn| async move {
            let names = started(&conn, rig, 1).await;
            rig.backend.print(&names[0], JOB);
            conn.client.subscribe().await.unwrap();
            rig.backend.state.borrow_mut().export_gate = Some(gate.clone());
            drain(&conn).await;
            conn.eventually("the export at the bound", || exported(rig, &names[0]))
                .await;
            let answer = conn.ok("stop_all").await;
            assert_eq!(answer["state"], "quitting");
            gate.notify_waiters();
            let (reason, update) = ending(&mut conn).await;
            assert_eq!(reason, UPDATE);
            assert_eq!(update, Some(UpdateEnding::new("1.4.0", true)));
        })
        .await;
    assert_eq!(ended.update, Some(UpdateEnding::new("1.4.0", true)));
}

#[tokio::test(flavor = "current_thread")]
async fn quit_during_the_drains_last_steps_drops_the_drain_from_the_snapshot() {
    let rig = rig(ONE_POOL);
    rig.write_json(&with_diagnostics(ONE_POOL));
    rig.drain_bound.set(Duration::from_millis(300));
    let gate = std::rc::Rc::new(tokio::sync::Notify::new());
    let rig = &rig;
    let (ended, ()) = rig
        .drive(Launch::Running, |conn| async move {
            let names = started(&conn, rig, 1).await;
            rig.backend.print(&names[0], JOB);
            rig.backend.state.borrow_mut().export_gate = Some(gate.clone());
            drain(&conn).await;
            conn.eventually("the export at the bound", || exported(rig, &names[0]))
                .await;
            assert_eq!(conn.ok("quit").await["state"], "quitting");
            // the export still holds the last steps open
            assert_eq!(conn.status().await.agent.update, None);
            gate.notify_waiters();
        })
        .await;
    assert_eq!(ended.reason, "quit");
    assert_eq!(ended.update, None);
}

/// a slot that backs off when the drain begins does not register again.
#[tokio::test(flavor = "current_thread")]
async fn a_backing_off_slot_registers_nothing_during_a_drain() {
    let rig = rig(TWO_POOLS);
    rig.clock.pause.set(Duration::from_millis(300));
    let rig = &rig;
    let (ended, ()) = rig
        .drive(Launch::Running, |mut conn| async move {
            let names = started(&conn, rig, 2).await;
            rig.backend.print(&names[1], JOB);
            conn.client.subscribe().await.unwrap();
            // the first runner's job fails, so its slot backs off
            rig.backend.state.borrow_mut().exits.push_back(Ok(1));
            rig.backend
                .stop_containers(&marshal_cli::runtime::backend::StopRequest {
                    names: vec![names[0].clone()],
                    grace: Duration::ZERO,
                    budget: Duration::from_secs(1),
                })
                .await
                .unwrap();
            conn.until("a slot backing off", |s| {
                s.pools
                    .iter()
                    .flat_map(|p| &p.runners)
                    .any(|r| r.state == "backing_off")
            })
            .await;
            drain(&conn).await;
            tokio::time::sleep(Duration::from_millis(900)).await;
            assert_eq!(containers(rig).len(), 2, "the backing-off slot registered");
            rig.end_jobs().await;
            assert_eq!(ending(&mut conn).await.0, UPDATE);
        })
        .await;
    assert_eq!(ended.reason, UPDATE);
    assert_eq!(containers(rig).len(), 2);
}

/// a pool that waited for a token does not start during a drain, and starts once the
/// drain is cancelled.
#[tokio::test(flavor = "current_thread")]
async fn a_pool_waiting_for_a_token_starts_only_after_the_drain_is_cancelled() {
    let rig = Rig::new(TWO_POOLS);
    rig.store_token("o", "r", TOKEN);
    rig.hold_jobs();
    rig.backend.state.borrow_mut().hold_runs_by_name = true;
    let rig = &rig;
    let (_, ()) = rig
        .drive(Launch::Running, |conn| async move {
            let names = started(&conn, rig, 1).await;
            rig.backend.print(&names[0], JOB);
            let requester = conn.another().await;
            drain(&requester).await;
            conn.set_token("o/d", TOKEN).await.unwrap();
            tokio::time::sleep(Duration::from_millis(600)).await;
            assert_eq!(containers(rig).len(), 1, "a pool started during the drain");
            drop(requester);
            conn.eventually("the late pool started", || containers(rig).len() >= 2)
                .await;
            rig.mark_running();
            conn.ok("quit").await;
        })
        .await;
}
