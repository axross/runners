//! the snapshot the agent serves: the runners and pools, the diagnostics, the leftovers
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
//! of a stop, the fit to the message limit, and what it never carries.
mod support;

use std::time::Duration;

use marshal_cli::agent::machine::Launch;
use marshal_cli::agent::server::Limits;
use marshal_cli::agent::snapshot::MAX_SNAPSHOT_BYTES;
use marshal_cli::agent::wire::MAX_AGENT_MESSAGE_BYTES;
use serde_json::json;
use support::agent::{JIT, ONE_POOL, Raw, Rig, TOKEN, many_pools};

#[tokio::test(flavor = "current_thread")]
async fn the_snapshot_shows_the_runners_pools_repositories_and_the_image() {
    let rig = Rig::new(ONE_POOL);
    rig.store_token("o", "r", TOKEN);
    rig.hold_jobs();
    let rig = &rig;
    let (_, ()) = rig
        .drive(Launch::Running, |conn| async move {
            let snapshot = conn.until_runner_runs().await;
            assert_eq!(snapshot.agent.version, "test");
            assert_eq!(snapshot.runners_up, 1);
            assert_eq!(snapshot.runners_wanted, 1);
            assert_eq!(snapshot.configuration.problems.len(), 0);
            assert!(snapshot.configuration.path.ends_with("config.json"));
            assert_eq!(snapshot.repositories[0].repository, "o/r");
            assert!(snapshot.repositories[0].has_token);
            let pool = &snapshot.pools[0];
            assert_eq!(
                (pool.name.as_str(), pool.slots, pool.runners_up),
                ("r", 1, 1)
            );
            let runner = &pool.runners[0];
            assert_eq!(runner.slot, 1);
            assert_eq!(runner.state, "running");
            assert!(
                runner
                    .workload
                    .as_deref()
                    .is_some_and(|name| !name.is_empty())
            );
            assert_eq!(snapshot.images[0].name, "actions-runner:local");
            rig.mark_running();
            conn.ok("quit").await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_stop_that_cannot_remove_a_container_lists_it_as_left() {
    let rig = Rig::new(ONE_POOL);
    rig.store_token("o", "r", TOKEN);
    rig.hold_jobs();
    let rig = &rig;
    let (_, ()) = rig
        .drive(Launch::Running, |conn| async move {
            conn.until_runner_runs().await;
            rig.mark_running();
            rig.backend.state.borrow_mut().remove_error = Some(
                marshal_cli::runtime::backend::BackendError::new("the engine refused"),
            );
            conn.ok("stop_all").await;
            let snapshot = conn.status().await;
            assert!(!snapshot.leftovers.is_empty(), "{snapshot:#?}");
            assert_eq!(snapshot.leftovers[0].kind, "container");
            rig.backend.state.borrow_mut().remove_error = None;
            conn.ok("quit").await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_start_that_never_reaches_the_pools_keeps_the_leftovers_and_one_that_does_clears_them() {
    use marshal_cli::image::check::ImageCheck;
    let rig = Rig::new(ONE_POOL);
    rig.store_token("o", "r", TOKEN);
    rig.hold_jobs();
    let rig = &rig;
    let (_, ()) = rig
        .drive(Launch::Running, |conn| async move {
            conn.until_runner_runs().await;
            rig.mark_running();
            rig.backend.state.borrow_mut().remove_error = Some(
                marshal_cli::runtime::backend::BackendError::new("the engine refused"),
            );
            conn.ok("stop_all").await;
            assert!(!conn.status().await.leftovers.is_empty());
            rig.backend.state.borrow_mut().remove_error = None;

            // the image is missing, so nothing is started and nothing is removed
            rig.backend
                .state
                .borrow_mut()
                .image_answers
                .push_back(ImageCheck::Missing);
            conn.ok("start_all").await;
            let snapshot = conn.until_state("stopped").await;
            assert_eq!(
                snapshot.agent.reason.as_ref().unwrap().code,
                "image_unusable"
            );
            assert!(!snapshot.leftovers.is_empty(), "{snapshot:#?}");

            // a start that gets to the pools prepares them, which removes what is stale
            conn.ok("start_all").await;
            conn.until_runner_runs().await;
            assert!(conn.status().await.leftovers.is_empty());
            rig.mark_running();
            conn.ok("quit").await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_big_snapshot_is_cut_to_the_message_limit_and_says_what_it_left_out() {
    let pools = 40;
    let rig = Rig::new(&many_pools(pools, 16));
    rig.store_many(pools);
    rig.hold_jobs();
    let rig = &rig;
    let (_, ()) = rig
        .drive(Launch::Running, |conn| async move {
            let snapshot = conn
                .until("all runners up", |s| s.runners_up as usize == 40 * 16)
                .await;
            let shown: usize = snapshot.pools.iter().map(|pool| pool.runners.len()).sum();
            assert!(snapshot.omitted.runners > 0, "nothing was cut");
            assert_eq!(shown + snapshot.omitted.runners as usize, 40 * 16);
            // the cut keeps the totals
            assert_eq!(snapshot.runners_up, 640);
            assert_eq!(snapshot.runners_wanted, 640);
            let bytes = serde_json::to_vec(&snapshot).unwrap().len();
            assert!(bytes <= MAX_SNAPSHOT_BYTES, "{bytes} bytes");

            // on the wire, the status response and the event both fit one message
            let mut raw = Raw::hello(&conn.roots).await;
            raw.request(1, "status", json!(null)).await;
            raw.request(2, "subscribe", json!({"topic": "state"})).await;
            raw.next().await.unwrap();
            let mut longest = raw.heard.len();
            raw.next().await.unwrap();
            assert!(
                raw.heard
                    .lines()
                    .all(|line| line.len() < MAX_AGENT_MESSAGE_BYTES)
            );
            longest = longest.max(raw.heard.lines().map(str::len).max().unwrap());
            assert!(longest > 30_000, "the snapshot was not big: {longest}");
            rig.mark_running();
            conn.ok("quit").await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_connection_whose_reader_stalls_on_big_events_is_closed_by_the_write_timer() {
    let pools = 40;
    let rig = Rig::new(&many_pools(pools, 16));
    rig.store_many(pools);
    rig.hold_jobs();
    rig.limits.set(Limits {
        write: Duration::from_millis(300),
        coalesce: Duration::from_millis(10),
        ..Limits::default()
    });
    let rig = &rig;
    let (_, (before, during, closed)) = rig
        .drive(Launch::Running, |conn| async move {
            conn.until("all runners up", |s| s.runners_up == 640).await;
            let before = support::agent::runtime_threads();
            let mut stalled = Vec::new();
            for _ in 0..4 {
                let mut raw = Raw::hello(&conn.roots).await;
                raw.request(1, "subscribe", json!({"topic": "state"})).await;
                stalled.push(raw);
            }
            // each change makes a 60 KiB event the stalled readers never read
            for round in 0..80 {
                if round % 2 == 0 {
                    conn.client.token_remove("o/repository-0000").await.unwrap();
                } else {
                    conn.set_token("o/repository-0000", TOKEN).await.unwrap();
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            let during = support::agent::runtime_threads();
            conn.status().await;
            // after the write timer, the agent has dropped them: reading drains to the end
            let mut closed = 0;
            for raw in &mut stalled {
                if raw.closed_within(Duration::from_secs(5)).await {
                    closed += 1;
                }
            }
            rig.mark_running();
            conn.ok("quit").await;
            (before, during, closed)
        })
        .await;
    assert_eq!(closed, 4, "every stalled connection was closed");
    if let (Some(before), Some(during)) = (before, during) {
        assert_eq!((before, during), (0, 0), "threads held for stalled readers");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn no_token_and_no_jit_configuration_reaches_a_snapshot_a_response_or_a_log_line() {
    let rig = Rig::new(ONE_POOL);
    rig.hold_jobs();
    let rig = &rig;
    let (_, heard) = rig
        .drive(Launch::Stopped, |conn| async move {
            let mut raw = Raw::hello(&conn.roots).await;
            raw.request(1, "subscribe", json!({"topic": "state"})).await;
            raw.request(2, "token_set", json!({"repository": "O/R", "token": TOKEN}))
                .await;
            raw.answer().await;
            conn.ok("start_all").await;
            conn.until_runner_runs().await;
            raw.request(3, "status", json!(null)).await;
            raw.request(4, "token_set", json!({"repository": "o/r", "token": ""}))
                .await;
            raw.request(5, "token_remove", json!({"repository": "o/r"}))
                .await;
            conn.status().await;
            tokio::time::sleep(Duration::from_millis(600)).await;
            rig.mark_running();
            conn.ok("stop_all").await;
            conn.status().await;
            let mut heard = String::new();
            raw.closed_within(Duration::from_millis(300)).await;
            heard.push_str(&raw.heard);
            conn.ok("quit").await;
            heard
        })
        .await;
    assert!(heard.contains("\"id\":2"), "the token_set was answered");
    let log = rig.log.lines().join("\n");
    for secret in [TOKEN, JIT] {
        assert!(!heard.contains(secret), "{secret} reached a connection");
        assert!(!log.contains(secret), "{secret} reached the log");
    }
    assert_eq!(rig.registrar.seen.borrow()[0].token, TOKEN);
    assert!(
        rig.backend
            .state
            .borrow()
            .jit_values
            .iter()
            .all(|jit| jit == JIT)
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_pool_with_diagnostics_lists_its_containers_and_what_the_exports_found() {
    let rig = Rig::new(ONE_POOL);
    // the fake engine never touches the sink, so a Windows path is as good here as any
    let sink = r"C:\\diag";
    rig.write_config(&ONE_POOL.replace(
        r#""volumes":[]}"#,
        &format!(r#""volumes":[],"diagnostics":true,"diagnosticDirectory":"{sink}"}}"#),
    ));
    rig.store_token("o", "r", TOKEN);
    rig.hold_jobs();
    let rig = &rig;
    let (_, ()) = rig
        .drive(Launch::Running, |conn| async move {
            conn.until_runner_runs().await;
            let running = conn.status().await;
            let diagnostics = running.pools[0]
                .diagnostics
                .clone()
                .expect("diagnostics on");
            assert!(diagnostics.limit > 0);
            assert_eq!(diagnostics.containers.len(), 1, "{diagnostics:#?}");
            assert_eq!(diagnostics.containers[0].state, "running");
            assert!(!diagnostics.containers[0].bundle);
            // a job ends while the agent runs: its bundle is admitted
            rig.end_jobs().await;
            let after = conn
                .until("a bundle", |s| {
                    s.pools[0]
                        .diagnostics
                        .as_ref()
                        .is_some_and(|d| d.in_sink == Some(1))
                })
                .await;
            assert_eq!(after.pools[0].diagnostics.as_ref().unwrap().written, 1);
            // a stop exports what is left; an export with a gap is a finding, and a clean
            // one is none
            rig.backend
                .state
                .borrow_mut()
                .export_results
                .push_back(marshal_cli::runtime::backend::ExportOutcome::Gap);
            rig.mark_running();
            conn.ok("stop_all").await;
            let stopped = conn.status().await;
            let diagnostics = stopped.pools[0].diagnostics.clone().unwrap();
            assert!(!diagnostics.findings.is_empty(), "{diagnostics:#?}");
            assert_eq!(diagnostics.findings[0].export, "gap");
            assert!(diagnostics.findings[0].removed);
            assert!(diagnostics.manual_recovery.is_empty());
            conn.ok("quit").await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn quit_is_not_held_by_connections_that_never_read() {
    let pools = 40;
    let rig = Rig::new(&many_pools(pools, 16));
    rig.store_many(pools);
    rig.hold_jobs();
    rig.limits.set(Limits {
        coalesce: Duration::from_millis(10),
        ..Limits::default()
    });
    let rig = &rig;
    let (_, took) = rig
        .drive(Launch::Running, |conn| async move {
            conn.until("all runners up", |s| s.runners_up == 640).await;
            let mut stalled = Vec::new();
            for _ in 0..4 {
                let mut raw = Raw::hello(&conn.roots).await;
                raw.request(1, "subscribe", json!({"topic": "state"})).await;
                stalled.push(raw);
            }
            for round in 0..60 {
                if round % 2 == 0 {
                    conn.client.token_remove("o/repository-0000").await.unwrap();
                } else {
                    conn.set_token("o/repository-0000", TOKEN).await.unwrap();
                }
                tokio::time::sleep(Duration::from_millis(15)).await;
            }
            rig.mark_running();
            let started = std::time::Instant::now();
            conn.ok("quit").await;
            drop(stalled);
            started.elapsed()
        })
        .await;
    assert!(took < Duration::from_secs(1), "quit took {took:?}");
    assert!(rig.lock_is_free());
}
