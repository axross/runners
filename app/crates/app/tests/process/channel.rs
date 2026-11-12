//! what the app shows and does when the channel is held by something that is not an
//! agent it can talk to, or by an agent at its connection limit: against real channels,
//! in real time.

use std::sync::Arc;
use std::time::{Duration, Instant};

use marshal_cli::agent::server::Limits;
use marshal_cli::agent::snapshot::Snapshot;
use marshal_cli::agent::wire::ClientInfo;
use marshal_cli::agent::{AgentChannel, HANDSHAKE_TIMEOUT, connect};
use marshal_cli::instance::lock::InstanceLock;
use marshal_session::Ended;
use tokio::sync::{mpsc, watch};

use crate::squatter::{Behaviour, squat};
use crate::support::{NoLauncher, Scratch, heading, run_session};

const RETRY: Duration = Duration::from_secs(5);

/// how long past a 5 s figure the app may take to show what it means.
const SLACK: Duration = Duration::from_secs(3);

#[tokio::test(flavor = "current_thread")]
async fn an_agent_that_never_answers_shows_as_not_answering_after_5_s_and_quit_ends_only_the_app() {
    let scratch = Scratch::new();
    let roots = scratch.roots();
    let squatter = squat(&roots, Behaviour::Silent);
    let started = Instant::now();
    let running = run_session(roots, Arc::new(NoLauncher));

    tokio::time::sleep(HANDSHAKE_TIMEOUT / 2).await;
    let early = running.screens.latest().unwrap();
    assert_ne!(
        heading(&early).as_deref(),
        Some("The agent is not answering"),
        "it said so after {:?}",
        started.elapsed()
    );

    running
        .screens
        .until(
            "the agent is not answering",
            HANDSHAKE_TIMEOUT + SLACK,
            |shown| heading(shown).as_deref() == Some("The agent is not answering"),
        )
        .await;
    assert!(
        started.elapsed() >= HANDSHAKE_TIMEOUT - Duration::from_millis(500),
        "after {:?}",
        started.elapsed()
    );
    assert!(squatter.accepted() >= 1);

    running.handle.quit();
    let ended = tokio::time::timeout(Duration::from_secs(10), running.ended)
        .await
        .expect("Quit ended the app")
        .unwrap();
    assert_eq!(ended, Ended::AppOnly);
}

#[tokio::test(flavor = "current_thread")]
async fn a_channel_that_does_not_answer_version_2_shows_another_marshal_and_is_asked_again_every_5_s()
 {
    let scratch = Scratch::new();
    let roots = scratch.roots();
    let squatter = squat(&roots, Behaviour::NotProtocol);
    let started = Instant::now();
    let running = run_session(roots, Arc::new(NoLauncher));

    running
        .screens
        .until("another Marshal is running", SLACK, |shown| {
            heading(shown).as_deref() == Some("Another Marshal is running")
        })
        .await;
    assert_eq!(squatter.accepted(), 1);

    // the second ask comes with the next 5 s, not before
    tokio::time::sleep(RETRY - Duration::from_secs(2)).await;
    assert_eq!(
        squatter.accepted(),
        1,
        "asked again after {:?}",
        started.elapsed()
    );
    let within = Instant::now() + RETRY + SLACK;
    while squatter.accepted() < 2 {
        assert!(Instant::now() < within, "never asked again");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(
        started.elapsed() >= RETRY - Duration::from_millis(500),
        "asked again after {:?}",
        started.elapsed()
    );
    assert_eq!(
        heading(&running.screens.latest().unwrap()).as_deref(),
        Some("Another Marshal is running")
    );
}

#[tokio::test(flavor = "current_thread")]
async fn an_agent_at_its_connection_limit_is_asked_again_until_it_answers() {
    let scratch = Scratch::new();
    let roots = scratch.roots();
    let lock = InstanceLock::acquire(&roots).unwrap();
    let (calls, _requests) = mpsc::unbounded_channel();
    let (_snapshots_in, snapshots) = watch::channel(Arc::new(Snapshot::default()));
    let limits = Limits {
        connections: 1,
        ..Limits::default()
    };
    let _channel = AgentChannel::open(&lock, &roots, calls, snapshots, limits).unwrap();
    // another client holds the only connection
    let (holder, _pushes) = connect(&roots, &ClientInfo::default()).await.unwrap();

    let started = Instant::now();
    let running = run_session(scratch.roots(), Arc::new(NoLauncher));
    running
        .screens
        .until("the agent is busy", SLACK, |shown| {
            heading(shown).as_deref() == Some("Marshal cannot reach its agent")
        })
        .await;

    drop(holder);
    // the next ask is within 5 s of the first, and connects
    running
        .screens
        .until("connected again", RETRY + SLACK, |shown| {
            shown.view.full().is_none()
        })
        .await;
    assert!(started.elapsed() >= RETRY - Duration::from_millis(500));
}
