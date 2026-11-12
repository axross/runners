//! the app starting its own executable in agent mode and connecting to it.

use std::sync::Arc;
use std::time::Duration;

use marshal_session::{Ended, OwnExecutable};

use crate::support::{APP, ONE_POOL, Scratch, run_session};

/// a start, the agent's first snapshot and the pool's preparation, on a slow runner.
/// the app ends with its agent: the agent's `quit` ending can reach it before its own
/// quit finishes.
fn ends_the_app(ended: &Ended) -> bool {
    matches!(ended, Ended::Quit) || *ended == Ended::AgentEnded("quit".to_string())
}

const CONNECTED_WITHIN: Duration = Duration::from_secs(60);

#[tokio::test(flavor = "current_thread")]
async fn the_app_starts_its_own_executable_in_agent_mode_and_connects_to_it() {
    let scratch = Scratch::new();
    scratch.write_config(ONE_POOL);
    let launcher = OwnExecutable::at(APP.into(), Arc::new(crate::support::Lines::default()))
        .with_environment(scratch.agent_environment());

    // nothing answers at first, so the session starts the agent from the executable
    let running = run_session(scratch.roots(), Arc::new(launcher));
    let shown = running
        .screens
        .until("the list of pools", CONNECTED_WITHIN, |shown| {
            shown.view.list().is_some()
        })
        .await;
    let listed = serde_json::to_string(&shown.view).unwrap();
    assert!(
        listed.contains("o/r") || listed.contains("\"r\""),
        "{listed}"
    );

    // Quit quits the agent with the app, and nothing answers afterwards
    running.handle.quit();
    let ended = tokio::time::timeout(Duration::from_secs(60), running.ended)
        .await
        .expect("the app ended")
        .unwrap();
    assert!(ends_the_app(&ended), "{ended:?}");
    crate::support::wait_for_no_agent(&scratch).await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_second_session_connects_to_the_agent_the_first_started_and_starts_none() {
    let scratch = Scratch::new();
    scratch.write_config(ONE_POOL);
    let launcher = OwnExecutable::at(APP.into(), Arc::new(crate::support::Lines::default()))
        .with_environment(scratch.agent_environment());
    let first = run_session(scratch.roots(), Arc::new(launcher));
    first
        .screens
        .until("the list of pools", CONNECTED_WITHIN, |shown| {
            shown.view.list().is_some()
        })
        .await;

    // the second app has no launcher that works: it must find the first one's agent
    let second = run_session(scratch.roots(), Arc::new(crate::support::NoLauncher));
    second
        .screens
        .until("the list of pools", Duration::from_secs(10), |shown| {
            shown.view.list().is_some()
        })
        .await;
    assert!(!second.log.text().contains("this test starts no agent"));

    second.handle.quit();
    let ended = tokio::time::timeout(Duration::from_secs(60), second.ended)
        .await
        .expect("the app ended")
        .unwrap();
    assert!(ends_the_app(&ended), "{ended:?}");
    let ended = tokio::time::timeout(Duration::from_secs(60), first.ended)
        .await
        .expect("the first app ended with the agent")
        .unwrap();
    assert!(
        matches!(ended, Ended::AgentEnded(_) | Ended::Quit),
        "{ended:?}"
    );
}
