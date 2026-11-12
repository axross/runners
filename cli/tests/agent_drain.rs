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
            drain(&conn).await;
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
            conn.ok("stop_all").await;
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
    let rig = &rig;
    let (ended, ()) = rig
        .drive(Launch::Running, |conn| async move {
            let names = started(&conn, rig, 1).await;
            rig.backend.print(&names[0], JOB);
            rig.mark_running();
            drain(&conn).await;
            conn.ok("quit").await;
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
            let requester = conn.another().await;
            drain(&requester).await;
            conn.eventually("the idle runner stopped", || !stopped(rig).is_empty())
                .await;
            drop(requester);
            conn.until("the drain cancelled", |s| s.agent.update.is_none())
                .await;
            conn.eventually("a runner registered again", || containers(rig).len() >= 3)
                .await;
            rig.mark_running();
            conn.ok("quit").await;
        })
        .await;
    assert_eq!(ended.reason, "quit");
}
