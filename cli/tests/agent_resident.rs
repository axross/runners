//! the resident agent on fake seams: stop, start, quit, build and staying up, behind
//! its real channel.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod support;

use std::rc::Rc;
use std::time::{Duration, Instant};

use marshal_cli::agent::machine::{EXECUTABLE_CHANGED, Launch, QUIT};
use marshal_cli::runtime::fake::Call;
use serde_json::{Value, json};
use support::agent::{ONE_POOL, Raw, Rig, TOKEN, TWO_POOLS};
use tokio::sync::Notify;

fn running_rig() -> Rig {
    let rig = Rig::new(ONE_POOL);
    rig.store_token("o", "r", TOKEN);
    rig.hold_jobs();
    rig
}

fn ran(rig: &Rig) -> usize {
    rig.backend
        .calls()
        .iter()
        .filter(|call| matches!(call, Call::RunContainer(_)))
        .count()
}

#[tokio::test(flavor = "current_thread")]
async fn it_runs_a_pool_and_quits() {
    let rig = running_rig();
    let rig = &rig;
    let (ended, ()) = rig
        .drive(Launch::Running, |conn| async move {
            let snapshot = conn.until_runner_runs().await;
            assert_eq!(snapshot.agent.state, "running");
            assert_eq!(snapshot.pools[0].state, "running");
            rig.mark_running();
            let answer = conn.ok("quit").await;
            assert_eq!(answer["state"], "quitting");
        })
        .await;
    assert_eq!(ended.reason, QUIT);
    assert!(ended.containers_removed);
    assert!(rig.lock_is_free());
}

#[tokio::test(flavor = "current_thread")]
async fn stop_all_stops_the_runners_and_start_all_starts_them_again() {
    let rig = running_rig();
    let rig = &rig;
    let (ended, ()) = rig
        .drive(Launch::Running, |conn| async move {
            conn.until_runner_runs().await;
            rig.mark_running();
            let answer = conn.ok("stop_all").await;
            assert_eq!(answer["state"], "stopped");
            let snapshot = conn.status().await;
            assert_eq!(snapshot.agent.state, "stopped");
            assert_eq!(snapshot.agent.reason.unwrap().code, "by_you");
            assert_eq!(snapshot.runners_up, 0);
            // a stop while stopped is answered and changes nothing
            assert_eq!(conn.ok("stop_all").await["state"], "stopped");

            let answer = conn.ok("start_all").await;
            assert_eq!(answer["state"], "resuming");
            conn.until_runner_runs().await;
            // a start while running is answered and changes nothing
            assert_eq!(conn.ok("start_all").await["state"], "running");
            assert_eq!(ran(rig), 2);
            rig.mark_running();
            conn.ok("quit").await;
        })
        .await;
    assert!(ended.containers_removed);
}

#[tokio::test(flavor = "current_thread")]
async fn an_agent_launched_stopped_runs_nothing_until_it_is_told_to() {
    let rig = running_rig();
    let rig = &rig;
    let (_, ()) = rig
        .drive(Launch::Stopped, |conn| async move {
            let snapshot = conn.status().await;
            assert_eq!(snapshot.agent.state, "stopped");
            assert_eq!(snapshot.agent.reason.unwrap().code, "by_you");
            assert_eq!(snapshot.pools[0].state, "stopped");
            assert_eq!(ran(rig), 0);
            conn.ok("start_all").await;
            conn.until_runner_runs().await;
            rig.mark_running();
            conn.ok("quit").await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn quit_from_the_stopped_state_frees_the_lock_within_a_second() {
    let rig = Rig::new(ONE_POOL);
    let rig = &rig;
    let (_, took) = rig
        .drive(Launch::Stopped, |conn| async move {
            let started = Instant::now();
            conn.ok("quit").await;
            started.elapsed()
        })
        .await;
    assert!(took < Duration::from_secs(1), "quit took {took:?}");
    assert!(rig.lock_is_free());
}

#[tokio::test(flavor = "current_thread")]
async fn quit_ends_the_agent_while_a_connection_leaves_its_messages_unread() {
    let rig = running_rig();
    let rig = &rig;
    let (ended, ()) = rig
        .drive(Launch::Running, |conn| async move {
            conn.until_runner_runs().await;
            // the raw stream is never read after the subscription is asked for, unlike
            // the library's client, which drains its socket
            let mut stalled = Raw::hello(&conn.roots).await;
            stalled
                .request(1, "subscribe", json!({"topic": "state"}))
                .await;
            for _ in 0..20 {
                conn.set_token("o/r", TOKEN).await.unwrap();
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            rig.mark_running();
            conn.ok("quit").await;
            drop(stalled);
        })
        .await;
    assert_eq!(ended.reason, QUIT);
    assert!(rig.lock_is_free());
}

#[tokio::test(flavor = "current_thread")]
async fn the_agent_ends_by_itself_when_its_executable_is_renamed() {
    let rig = running_rig();
    let file = rig.root.path().join("marshal-under-test");
    std::fs::write(&file, b"v1").unwrap();
    *rig.executable.borrow_mut() = Some(file.clone());
    let rig = &rig;
    let renamed = file.with_extension("old");
    let started = Instant::now();
    let (ended, ()) = rig
        .drive(Launch::Running, |mut conn| async move {
            conn.until_runner_runs().await;
            conn.client.subscribe().await.unwrap();
            rig.mark_running();
            std::fs::rename(&file, &renamed).unwrap();
            let mut ending = None;
            while ending.is_none() {
                if let marshal_cli::agent::Push::Ending { reason, .. } = conn.push().await {
                    ending = Some(reason);
                }
            }
            assert_eq!(ending.unwrap(), EXECUTABLE_CHANGED);
        })
        .await;
    assert_eq!(ended.reason, EXECUTABLE_CHANGED);
    assert!(started.elapsed() < Duration::from_secs(5));
    assert!(rig.lock_is_free());
}

#[tokio::test(flavor = "current_thread")]
async fn the_agent_logs_when_it_cannot_watch_its_executable() {
    let rig = Rig::new(ONE_POOL);
    *rig.executable.borrow_mut() = Some(rig.root.path().join("no-such-executable"));
    let rig = &rig;
    let (_, ()) = rig
        .drive(Launch::Stopped, |conn| async move {
            conn.ok("quit").await;
        })
        .await;
    assert!(
        rig.log
            .lines()
            .iter()
            .any(|line| line.contains("cannot watch") && line.contains("no-such-executable")),
        "{:?}",
        rig.log.lines()
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_second_agent_is_refused_while_the_first_holds_the_lock() {
    let rig = Rig::new(ONE_POOL);
    let rig = &rig;
    let (_, ()) = rig
        .drive(Launch::Stopped, |conn| async move {
            assert!(!rig.lock_is_free());
            conn.ok("quit").await;
        })
        .await;
}

// staying up -----------------------------------------------------------------------

async fn stays_up_stopped(config: Option<&str>, code: &str) {
    let rig = Rig::new(ONE_POOL);
    match config {
        Some(text) => rig.write_config(text),
        None => std::fs::remove_file(rig.root.path().join("config.json")).unwrap(),
    }
    let rig = &rig;
    let code = code.to_string();
    let (ended, ()) = rig
        .drive(Launch::Running, |conn| async move {
            let snapshot = conn.until_state("stopped").await;
            assert_eq!(snapshot.agent.reason.unwrap().code, code);
            // it still answers, and a start after a repair works
            conn.ok("status").await;
            conn.ok("quit").await;
        })
        .await;
    assert_eq!(ended.reason, QUIT);
}

#[tokio::test(flavor = "current_thread")]
async fn it_stays_up_without_a_configuration() {
    stays_up_stopped(None, "no_configuration").await;
}

#[tokio::test(flavor = "current_thread")]
async fn it_stays_up_with_an_invalid_configuration() {
    stays_up_stopped(Some("{ not json"), "invalid_configuration").await;
}

#[tokio::test(flavor = "current_thread")]
async fn it_stays_up_when_the_image_is_missing() {
    let rig = Rig::new(ONE_POOL);
    rig.backend
        .state
        .borrow_mut()
        .image_answers
        .push_back(marshal_cli::image::check::ImageCheck::Missing);
    let rig = &rig;
    let (_, ()) = rig
        .drive(Launch::Running, |conn| async move {
            let snapshot = conn.until_state("stopped").await;
            assert_eq!(snapshot.agent.reason.unwrap().code, "image_unusable");
            conn.ok("quit").await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn it_stays_up_when_no_pool_can_start() {
    // no token is stored, so the only pool is skipped
    let rig = Rig::new(ONE_POOL);
    let rig = &rig;
    let (_, ()) = rig
        .drive(Launch::Running, |conn| async move {
            let snapshot = conn.until_state("stopped").await;
            assert_eq!(snapshot.agent.reason.unwrap().code, "no_pool_started");
            // a pool skipped for good is listed stopped, and keeps why
            assert_eq!(snapshot.pools[0].state, "stopped");
            assert!(
                snapshot.pools[0]
                    .reason
                    .as_deref()
                    .unwrap()
                    .contains("no token")
            );
            conn.ok("quit").await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_start_after_a_repair_runs_the_pools() {
    let rig = Rig::new(ONE_POOL);
    rig.write_config("{ not json");
    rig.store_token("o", "r", TOKEN);
    rig.hold_jobs();
    let rig = &rig;
    let (_, ()) = rig
        .drive(Launch::Running, |conn| async move {
            conn.until_state("stopped").await;
            rig.write_config(ONE_POOL);
            let answer = conn.ok("start_all").await;
            assert_eq!(answer["state"], "resuming");
            conn.until_runner_runs().await;
            rig.mark_running();
            conn.ok("quit").await;
        })
        .await;
}

// start_all names why it could not start ------------------------------------------

#[tokio::test(flavor = "current_thread")]
async fn start_all_answers_stopped_with_the_reason_when_it_cannot_load_or_use_the_image() {
    let rig = Rig::new(ONE_POOL);
    rig.store_token("o", "r", TOKEN);
    rig.hold_jobs();
    let rig = &rig;
    let (_, ()) = rig
        .drive(Launch::Stopped, |conn| async move {
            std::fs::remove_file(rig.root.path().join("config.json")).unwrap();
            let answer = conn.ok("start_all").await;
            assert_eq!(answer["state"], "stopped");
            assert_eq!(answer["reason"]["code"], "no_configuration");

            rig.write_config("{ not json");
            let answer = conn.ok("start_all").await;
            assert_eq!(answer["state"], "stopped");
            assert_eq!(answer["reason"]["code"], "invalid_configuration");

            rig.write_config(ONE_POOL);
            rig.backend
                .state
                .borrow_mut()
                .image_answers
                .push_back(marshal_cli::image::check::ImageCheck::Missing);
            let answer = conn.ok("start_all").await;
            assert_eq!(answer["state"], "stopped");
            assert_eq!(answer["reason"]["code"], "image_unusable");
            assert_eq!(ran(rig), 0);

            // all repaired: it resumes, and the snapshot says when it runs
            let answer = conn.ok("start_all").await;
            assert_eq!(answer["state"], "resuming");
            conn.until_runner_runs().await;
            rig.mark_running();
            conn.ok("quit").await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn start_all_does_not_claim_running_for_pools_that_cannot_start() {
    // every pool lacks a token: the answer is resuming, and the snapshot carries the
    // outcome, never running
    let rig = Rig::new(ONE_POOL);
    let rig = &rig;
    let (_, ()) = rig
        .drive(Launch::Stopped, |conn| async move {
            let mut watcher = conn.another().await;
            watcher.client.subscribe().await.unwrap();
            let answer = conn.ok("start_all").await;
            assert_eq!(answer["state"], "resuming");
            let snapshot = conn.until_state("stopped").await;
            assert_eq!(snapshot.agent.reason.unwrap().code, "no_pool_started");
            while let marshal_cli::agent::Push::Event { snapshot, .. } = watcher.push().await {
                assert_ne!(snapshot.agent.state, "running", "it never ran");
                if snapshot.agent.state == "stopped" && snapshot.agent.reason.is_some() {
                    break;
                }
            }
            conn.ok("quit").await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_started_agent_is_running_once_its_pools_are_up() {
    let rig = Rig::new(ONE_POOL);
    rig.store_token("o", "r", TOKEN);
    rig.hold_jobs();
    let rig = &rig;
    let (_, ()) = rig
        .drive(Launch::Stopped, |conn| async move {
            conn.ok("start_all").await;
            let snapshot = conn.until_runner_runs().await;
            assert_eq!(snapshot.agent.state, "running");
            rig.mark_running();
            conn.ok("quit").await;
        })
        .await;
}

// resuming ---------------------------------------------------------------------------

/// which part of resuming the fake Docker holds: the image check that comes first, or
/// the wait for Docker that comes after the configuration is read.
#[derive(Clone, Copy, Debug)]
enum Held {
    ImageCheck,
    Availability,
}

fn resuming_rig(held: Held) -> Rig {
    let rig = Rig::new(ONE_POOL);
    rig.store_token("o", "r", TOKEN);
    rig.hold_jobs();
    let gate = Some(Rc::new(Notify::new()));
    let mut state = rig.backend.state.borrow_mut();
    match held {
        Held::ImageCheck => state.image_gate = gate,
        Held::Availability => state.available_gate = gate,
    }
    drop(state);
    rig
}

const HELD: [Held; 2] = [Held::ImageCheck, Held::Availability];

#[tokio::test(flavor = "current_thread")]
async fn start_all_and_build_are_busy_while_the_agent_resumes() {
    for held in HELD {
        let rig = resuming_rig(held);
        let rig = &rig;
        let (_, ()) = rig
            .drive(Launch::Running, |conn| async move {
                conn.until_state("resuming").await;
                assert_eq!(
                    conn.code("start_all", Value::Null).await,
                    "busy",
                    "{held:?}"
                );
                let error = match conn.call("build", Value::Null).await {
                    Err(marshal_cli::agent::CallError::Agent(body)) => body,
                    other => panic!("{held:?}: build was not refused: {other:?}"),
                };
                assert_eq!(error.code, "busy", "{held:?}");
                assert!(error.message.contains("resuming"), "{}", error.message);
                // what does not change the state is still answered
                assert_eq!(conn.status().await.agent.state, "resuming");
                conn.ok("quit").await;
            })
            .await;
    }
}

#[tokio::test(flavor = "current_thread")]
async fn stop_all_abandons_a_resume_and_leaves_the_agent_stopped_by_you() {
    for held in HELD {
        let rig = resuming_rig(held);
        let rig = &rig;
        let (_, ()) = rig
            .drive(Launch::Running, |conn| async move {
                conn.until_state("resuming").await;
                assert_eq!(conn.ok("stop_all").await["state"], "stopped", "{held:?}");
                let snapshot = conn.status().await;
                assert_eq!(snapshot.agent.state, "stopped", "{held:?}");
                assert_eq!(snapshot.agent.reason.unwrap().code, "by_you");
                assert_eq!(ran(rig), 0, "{held:?}: a job container was run");
                conn.ok("quit").await;
            })
            .await;
    }
}

#[tokio::test(flavor = "current_thread")]
async fn quit_abandons_a_resume_and_ends_the_agent() {
    for held in HELD {
        let rig = resuming_rig(held);
        let rig = &rig;
        let (ended, ()) = rig
            .drive(Launch::Running, |conn| async move {
                conn.until_state("resuming").await;
                assert_eq!(conn.ok("quit").await["state"], "quitting", "{held:?}");
            })
            .await;
        assert_eq!(ended.reason, QUIT, "{held:?}");
        assert!(rig.lock_is_free(), "{held:?}");
    }
}

// build ----------------------------------------------------------------------------

#[tokio::test(flavor = "current_thread")]
async fn build_reports_building_with_its_start_and_ends_ready() {
    let rig = Rig::new(ONE_POOL);
    rig.builder.hold.set(true);
    let rig = &rig;
    let (_, ()) = rig
        .drive(Launch::Stopped, |conn| async move {
            let answer = conn.ok("build").await;
            assert_eq!(answer["state"], "building");
            assert!(answer["started"].as_u64().unwrap() > 0);
            let snapshot = conn.status().await;
            assert_eq!(snapshot.agent.state, "building");
            assert_eq!(snapshot.agent.build.unwrap().outcome, "building");
            assert_eq!(conn.code("start_all", Value::Null).await, "busy");
            assert_eq!(conn.code("build", Value::Null).await, "busy");
            rig.builder.release.notify_one();
            let snapshot = conn.until_state("stopped").await;
            // the outcome is in `build`; the reason the agent is stopped for is unchanged
            assert_eq!(snapshot.agent.reason.unwrap().code, "by_you");
            let build = snapshot.agent.build.unwrap();
            assert_eq!(build.outcome, "ready");
            assert!(build.finished.is_some());
            conn.ok("quit").await;
        })
        .await;
    assert_eq!(*rig.builder.images.borrow(), ["actions-runner:local"]);
}

#[tokio::test(flavor = "current_thread")]
async fn a_successful_build_keeps_the_reason_the_agent_was_stopped_for() {
    let rig = Rig::new(ONE_POOL);
    rig.backend
        .state
        .borrow_mut()
        .image_answers
        .push_back(marshal_cli::image::check::ImageCheck::Missing);
    let rig = &rig;
    let (_, ()) = rig
        .drive(Launch::Running, |conn| async move {
            let snapshot = conn.until_state("stopped").await;
            assert_eq!(snapshot.agent.reason.unwrap().code, "image_unusable");
            conn.ok("build").await;
            let snapshot = conn
                .until("the build to end", |s| {
                    s.agent.state == "stopped"
                        && s.agent.build.as_ref().is_some_and(|b| b.outcome == "ready")
                })
                .await;
            assert_eq!(snapshot.agent.reason.unwrap().code, "image_unusable");
            conn.ok("quit").await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_failed_build_stops_the_agent_with_its_reason() {
    let rig = Rig::new(ONE_POOL);
    rig.builder
        .results
        .borrow_mut()
        .push_back(Err("the step failed".to_string()));
    let rig = &rig;
    let (_, ()) = rig
        .drive(Launch::Stopped, |conn| async move {
            conn.ok("build").await;
            let snapshot = conn.until_state("stopped").await;
            let reason = snapshot.agent.reason.unwrap();
            assert_eq!(reason.code, "image_build_failed");
            assert!(reason.message.contains("the step failed"));
            assert_eq!(snapshot.agent.build.unwrap().outcome, "failed");
            conn.ok("quit").await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn stop_all_cancels_a_build() {
    let rig = Rig::new(ONE_POOL);
    rig.builder.hold.set(true);
    let rig = &rig;
    let (_, ()) = rig
        .drive(Launch::Stopped, |conn| async move {
            conn.ok("build").await;
            let answer = conn.ok("stop_all").await;
            assert_eq!(answer["state"], "stopped");
            let snapshot = conn.status().await;
            assert_eq!(snapshot.agent.reason.unwrap().code, "by_you");
            assert_eq!(snapshot.agent.build.unwrap().outcome, "cancelled");
            conn.ok("quit").await;
        })
        .await;
    assert!(rig.builder.stopped.get());
}

#[tokio::test(flavor = "current_thread")]
async fn quit_cancels_a_build_and_ends_the_agent() {
    let rig = Rig::new(ONE_POOL);
    rig.builder.hold.set(true);
    let rig = &rig;
    let (ended, ()) = rig
        .drive(Launch::Stopped, |conn| async move {
            conn.ok("build").await;
            conn.ok("quit").await;
        })
        .await;
    assert_eq!(ended.reason, QUIT);
    assert!(rig.builder.stopped.get());
}

#[tokio::test(flavor = "current_thread")]
async fn build_is_refused_while_the_runners_run_and_without_a_usable_configuration() {
    let rig = running_rig();
    let rig = &rig;
    let (_, ()) = rig
        .drive(Launch::Running, |conn| async move {
            conn.until_runner_runs().await;
            assert_eq!(conn.code("build", Value::Null).await, "refused");
            rig.mark_running();
            conn.ok("stop_all").await;
            rig.write_config("{ not json");
            assert_eq!(conn.code("build", Value::Null).await, "refused");
            std::fs::remove_file(rig.root.path().join("config.json")).unwrap();
            assert_eq!(conn.code("build", Value::Null).await, "refused");
            // the refusals changed nothing
            assert_eq!(conn.status().await.agent.state, "stopped");
            conn.ok("quit").await;
        })
        .await;
    assert!(rig.builder.images.borrow().is_empty());
}

// pools ----------------------------------------------------------------------------

#[tokio::test(flavor = "current_thread")]
async fn a_pool_without_a_token_is_skipped_while_the_other_runs() {
    let rig = Rig::new(TWO_POOLS);
    rig.store_token("o", "r", TOKEN);
    rig.hold_jobs();
    let rig = &rig;
    let (_, ()) = rig
        .drive(Launch::Running, |conn| async move {
            conn.until_runner_runs().await;
            let snapshot = conn.status().await;
            let by_name = |name: &str| {
                snapshot
                    .pools
                    .iter()
                    .find(|pool| pool.name == name)
                    .unwrap()
            };
            assert_eq!(by_name("r").state, "running");
            assert_eq!(by_name("d").state, "skipped");
            assert!(!by_name("d").has_token);
            rig.mark_running();
            conn.ok("quit").await;
        })
        .await;
}
