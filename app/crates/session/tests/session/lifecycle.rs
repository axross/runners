//! the connection lifecycle: connecting, starting the agent once, lost contact, Try
//! Again, `busy`, not answering, a held lock, and the agent ending.

use marshal_cli::agent::HANDSHAKE_TIMEOUT;
use marshal_cli::agent::Launch;
use marshal_session::{Ended, RETRY_EVERY};

use crate::support::{Agent, Answer, World, fixture, patched, secs, start, what};

#[tokio::test(start_paused = true)]
async fn the_agent_that_answers_at_launch_is_connected_to_and_not_started() {
    let (agent, connected) = Agent::new();
    let world = World::default();
    world.then(Answer::Agent(connected));
    let session = start(world);
    session.settle().await;
    assert_eq!(what(&session.screen.view()), "Starting Marshal");
    agent.push(fixture("running"));
    session.settle().await;
    assert_eq!(what(&session.screen.view()), "list");
    assert!(session.world.started().is_empty());
    assert_eq!(session.world.dials(), 1);
}

#[tokio::test(start_paused = true)]
async fn an_agent_that_is_not_there_is_started_once_and_connected_to_when_it_comes_up() {
    let (agent, connected) = Agent::new();
    let world = World::default();
    // the channel opens a moment after the start: three tries find nothing
    world.up_after_start(Answer::Agent(connected));
    let session = start(world);
    session.wait(secs(1)).await;
    assert_eq!(session.world.started(), [Launch::Running]);
    agent.push(fixture("running"));
    session.settle().await;
    assert_eq!(what(&session.screen.view()), "list");
    session.wait(secs(60)).await;
    assert_eq!(
        session.world.started(),
        [Launch::Running],
        "the agent is started once"
    );
}

#[tokio::test(start_paused = true)]
async fn an_agent_that_never_comes_up_leaves_no_agent_and_try_again_starts_it_once_more() {
    let world = World::default();
    let session = start(world);
    session.wait(secs(30)).await;
    assert_eq!(session.world.started(), [Launch::Running]);
    assert_eq!(
        what(&session.screen.view()),
        "Marshal cannot reach its agent"
    );
    let action = session
        .screen
        .view()
        .full()
        .unwrap()
        .action
        .clone()
        .unwrap();
    assert!(action.available, "Try Again is offered");
    // no more starts without the user
    session.wait(secs(120)).await;
    assert_eq!(session.world.started().len(), 1);

    let (agent, connected) = Agent::new();
    session.world.up_after_start(Answer::Agent(connected));
    session.handle.try_again().await.unwrap();
    session.wait(secs(1)).await;
    assert_eq!(session.world.started().len(), 2);
    agent.push(fixture("running"));
    session.settle().await;
    assert_eq!(what(&session.screen.view()), "list");
}

#[tokio::test(start_paused = true)]
async fn try_again_shows_that_marshal_is_starting_at_once() {
    let session = start(World::default());
    session.wait(secs(30)).await;
    session.handle.try_again().await.unwrap();
    assert_eq!(what(&session.screen.view()), "Starting Marshal");
}

#[tokio::test(start_paused = true)]
async fn a_start_that_is_refused_is_not_made_and_says_so_in_the_log() {
    let world = World::default();
    world.refuse_start("the executable /x/marshal-app has changed since the app started");
    let session = start(world);
    session.wait(secs(30)).await;
    assert!(session.world.started().is_empty());
    assert_eq!(
        what(&session.screen.view()),
        "Marshal cannot reach its agent"
    );
    assert!(
        session
            .log
            .lines()
            .iter()
            .any(|line| line.contains("has changed since the app started")),
        "{:?}",
        session.log.lines()
    );
    // not retried by itself, and one dial only: no start was possible
    session.wait(secs(60)).await;
    assert_eq!(session.world.dials(), 1);
}

#[tokio::test(start_paused = true)]
async fn a_dropped_connection_keeps_the_list_and_starts_the_agent_once_running() {
    let (first, connected) = Agent::new();
    let world = World::default();
    world.then(Answer::Agent(connected));
    let session = start(world);
    first.push(fixture("running"));
    session.settle().await;

    let (second, reconnected) = Agent::new();
    session.world.up_after_start(Answer::Agent(reconnected));
    first.close();
    session.settle().await;
    let view = session.screen.view();
    assert_eq!(what(&view), "lost contact (dropped)");
    assert!(view.list().unwrap().stale, "the last list stays");
    assert_eq!(view.list().unwrap().pools.len(), 3);

    session.wait(secs(1)).await;
    assert_eq!(session.world.started(), [Launch::Running]);
    second.push(fixture("running"));
    session.settle().await;
    assert_eq!(what(&session.screen.view()), "list");
    assert!(!session.screen.view().list().unwrap().stale);
}

#[tokio::test(start_paused = true)]
async fn an_agent_stopped_by_you_is_started_stopped_after_a_drop() {
    let (first, connected) = Agent::new();
    let world = World::default();
    world.then(Answer::Agent(connected));
    let session = start(world);
    first.push(fixture("stopped_by_you"));
    session.settle().await;
    first.close();
    session.wait(secs(1)).await;
    assert_eq!(session.world.started(), [Launch::Stopped]);
}

#[tokio::test(start_paused = true)]
async fn an_agent_stopped_for_another_reason_is_started_running_after_a_drop() {
    let (first, connected) = Agent::new();
    let world = World::default();
    world.then(Answer::Agent(connected));
    let session = start(world);
    first.push(fixture("stopped_no_pool_started"));
    session.settle().await;
    first.close();
    session.wait(secs(1)).await;
    assert_eq!(session.world.started(), [Launch::Running]);
}

#[tokio::test(start_paused = true)]
async fn after_the_one_restart_fails_try_again_is_offered_and_starts_it_once_more() {
    let (first, connected) = Agent::new();
    let world = World::default();
    world.then(Answer::Agent(connected));
    let session = start(world);
    first.push(fixture("running"));
    session.settle().await;
    first.close();
    session.wait(secs(30)).await;
    assert_eq!(session.world.started(), [Launch::Running]);
    let view = session.screen.view();
    assert_eq!(what(&view), "lost contact (restart failed)");
    let banner = &view.banners()[0];
    assert!(
        banner.action.as_ref().unwrap().available,
        "Try Again is offered"
    );
    session.wait(secs(300)).await;
    assert_eq!(session.world.started().len(), 1, "no restart storm");

    let (second, connected) = Agent::new();
    session.world.up_after_start(Answer::Agent(connected));
    session.handle.try_again().await.unwrap();
    session.wait(secs(1)).await;
    assert_eq!(session.world.started().len(), 2);
    second.push(fixture("running"));
    session.settle().await;
    assert_eq!(what(&session.screen.view()), "list");
}

#[tokio::test(start_paused = true)]
async fn a_connection_that_comes_back_gives_the_agent_one_restart_again_for_the_next_drop() {
    let (first, connected) = Agent::new();
    let world = World::default();
    world.then(Answer::Agent(connected));
    let session = start(world);
    first.push(fixture("running"));
    session.settle().await;

    let (second, connected) = Agent::new();
    session.world.up_after_start(Answer::Agent(connected));
    first.close();
    session.wait(secs(1)).await;
    second.push(fixture("running"));
    session.settle().await;

    let (third, connected) = Agent::new();
    session.world.up_after_start(Answer::Agent(connected));
    second.close();
    session.wait(secs(1)).await;
    assert_eq!(session.world.started().len(), 2, "one start for each drop");
    third.push(fixture("running"));
    session.settle().await;
    assert_eq!(what(&session.screen.view()), "list");
}

#[tokio::test(start_paused = true)]
async fn busy_shows_lost_contact_and_is_asked_again_every_five_seconds() {
    let (agent, connected) = Agent::new();
    let world = World::default();
    world
        .then(Answer::Busy)
        .then(Answer::Busy)
        .then(Answer::Busy);
    world.then(Answer::Agent(connected));
    let session = start(world);
    session.settle().await;
    assert_eq!(what(&session.screen.view()), "lost contact (busy)");
    session.wait(secs(4)).await;
    assert_eq!(session.world.dials(), 1, "not before 5 s");
    session.wait(secs(2)).await;
    assert_eq!(session.world.dials(), 2);
    session.wait(secs(5)).await;
    assert_eq!(session.world.dial_times(session.started_at), [0, 5, 10]);
    session.wait(secs(5)).await;
    assert_eq!(session.world.dial_times(session.started_at), [0, 5, 10, 15]);
    agent.push(fixture("running"));
    session.settle().await;
    assert_eq!(what(&session.screen.view()), "list");
    assert!(
        session.world.started().is_empty(),
        "a busy agent is not started over"
    );
    assert_eq!(RETRY_EVERY, secs(5));
}

#[tokio::test(start_paused = true)]
async fn busy_after_a_drop_keeps_the_last_list_under_the_busy_banner() {
    let (first, connected) = Agent::new();
    let world = World::default();
    world.then(Answer::Agent(connected));
    let session = start(world);
    first.push(fixture("running"));
    session.settle().await;
    session.world.always(Answer::Busy);
    first.close();
    session.wait(secs(1)).await;
    let view = session.screen.view();
    assert_eq!(what(&view), "lost contact (busy)");
    assert!(view.list().unwrap().stale);
}

#[tokio::test(start_paused = true)]
async fn an_agent_that_has_not_answered_at_five_seconds_is_not_answering() {
    assert_eq!(HANDSHAKE_TIMEOUT, secs(5));
    let world = World::default();
    world.then(Answer::After(
        HANDSHAKE_TIMEOUT,
        Box::new(Answer::NotAnswering),
    ));
    let session = start(world);
    session.wait(secs(4)).await;
    assert_eq!(what(&session.screen.view()), "Starting Marshal");
    session.wait(secs(2)).await;
    assert_eq!(what(&session.screen.view()), "The agent is not answering");
    // it is not asked again by itself, and not started over
    session.wait(secs(300)).await;
    assert_eq!(session.world.dials(), 1);
    assert!(session.world.started().is_empty());
}

#[tokio::test(start_paused = true)]
async fn quit_with_an_agent_that_is_not_answering_ends_only_the_app() {
    let world = World::default();
    world.then(Answer::NotAnswering);
    let session = start(world);
    session.settle().await;
    session.handle.quit();
    assert_eq!(session.ended().await, Ended::AppOnly);
}

#[tokio::test(start_paused = true)]
async fn quit_with_no_agent_to_reach_ends_only_the_app() {
    let session = start(World::default());
    session.wait(secs(30)).await;
    session.handle.quit();
    assert_eq!(session.ended().await, Ended::AppOnly);
}

#[tokio::test(start_paused = true)]
async fn a_held_lock_shows_another_marshal_and_is_checked_again_every_five_seconds() {
    let (agent, connected) = Agent::new();
    let world = World::default();
    world.then(Answer::LockHeld).then(Answer::LockHeld);
    world.then(Answer::LockHeld);
    world.then(Answer::Agent(connected));
    let session = start(world);
    session.settle().await;
    let view = session.screen.view();
    assert_eq!(what(&view), "Another Marshal is running");
    assert!(view.full().unwrap().action.is_none(), "it offers no action");
    session.wait(secs(4)).await;
    assert_eq!(session.world.dials(), 1);
    session.wait(secs(2)).await;
    session.wait(secs(5)).await;
    assert_eq!(session.world.dial_times(session.started_at), [0, 5, 10]);
    session.wait(secs(5)).await;
    agent.push(fixture("running"));
    session.settle().await;
    assert_eq!(what(&session.screen.view()), "list");
    assert!(
        session.world.started().is_empty(),
        "the agent is not started"
    );
}

#[tokio::test(start_paused = true)]
async fn an_agent_that_quits_ends_the_app() {
    let (agent, connected) = Agent::new();
    let world = World::default();
    world.then(Answer::Agent(connected));
    let session = start(world);
    agent.push(fixture("running"));
    session.settle().await;
    agent.end("quit");
    assert_eq!(session.ended().await, Ended::AgentEnded("quit".to_string()));
}

#[tokio::test(start_paused = true)]
async fn an_agent_whose_executable_changed_ends_the_app_and_is_not_started_again() {
    let (agent, connected) = Agent::new();
    let world = World::default();
    world.then(Answer::Agent(connected));
    let session = start(world);
    agent.push(fixture("running"));
    session.settle().await;
    let world = session.world.clone();
    agent.end("executable_changed");
    agent.close();
    assert_eq!(
        session.ended().await,
        Ended::AgentEnded("executable_changed".to_string())
    );
    assert!(world.started().is_empty());
}

#[tokio::test(start_paused = true)]
async fn an_ending_reason_the_app_does_not_know_is_ignored_and_the_drop_that_follows_restarts_once()
{
    let (first, connected) = Agent::new();
    let world = World::default();
    world.then(Answer::Agent(connected));
    let session = start(world);
    first.push(fixture("running"));
    session.settle().await;
    let (second, reconnected) = Agent::new();
    session.world.up_after_start(Answer::Agent(reconnected));
    first.end("update");
    session.settle().await;
    assert!(!session.is_over(), "an unknown reason does not end the app");
    first.close();
    session.wait(secs(1)).await;
    assert_eq!(session.world.started(), [Launch::Running]);
    second.push(fixture("running"));
    session.settle().await;
    assert_eq!(what(&session.screen.view()), "list");
}

#[tokio::test(start_paused = true)]
async fn the_users_quit_asks_the_agent_and_ends_the_app_when_the_connection_ends() {
    let (agent, connected) = Agent::new();
    let world = World::default();
    world.then(Answer::Agent(connected));
    let session = start(world);
    agent.push(fixture("running"));
    session.settle().await;
    session.handle.quit();
    session.settle().await;
    assert_eq!(what(&session.screen.view()), "Quitting Marshal");
    assert_eq!(agent.calls(), [crate::support::Called::Quit]);
    agent.close();
    assert_eq!(session.ended().await, Ended::Quit);
}

#[tokio::test(start_paused = true)]
async fn the_users_quit_ends_the_app_even_when_the_connection_never_closes() {
    let (agent, connected) = Agent::new();
    let world = World::default();
    world.then(Answer::Agent(connected));
    let session = start(world);
    agent.push(fixture("running"));
    session.settle().await;
    session.handle.quit();
    assert_eq!(session.ended().await, Ended::Quit);
}

#[tokio::test(start_paused = true)]
async fn a_quit_the_agent_refuses_leaves_the_app_running() {
    let (agent, connected) = Agent::new();
    agent.answer(
        "quit",
        Err(marshal_session::Failure::Refused("not now".to_string())),
    );
    let world = World::default();
    world.then(Answer::Agent(connected));
    let session = start(world);
    agent.push(fixture("running"));
    session.settle().await;
    session.handle.quit();
    session.wait(secs(60)).await;
    assert!(!session.is_over());
    assert_eq!(what(&session.screen.view()), "list");
}

#[tokio::test(start_paused = true)]
async fn the_subscription_the_agent_refuses_as_full_is_busy() {
    let (_agent, connected) = Agent::new();
    _agent.subscription(Err(marshal_session::Failure::Busy("full".to_string())));
    let world = World::default();
    world.then(Answer::Agent(connected));
    let session = start(world);
    session.settle().await;
    assert_eq!(what(&session.screen.view()), "lost contact (busy)");
}

#[tokio::test(start_paused = true)]
async fn lost_contact_marks_the_list_with_the_time_it_was_last_heard() {
    let (agent, connected) = Agent::new();
    let world = World::default();
    world.then(Answer::Agent(connected));
    let session = start(world);
    agent.push(patched("running", |_| {}));
    session.settle().await;
    session.wait(secs(100)).await;
    // heartbeats are contact too
    agent.heartbeat();
    session.settle().await;
    let heard = crate::support::NOW_MS + 100_000;
    session.world.always(Answer::Busy);
    agent.close();
    session.settle().await;
    let view = session.screen.view();
    let text = format!("{:?}", view.banners()[0]);
    let clock = marshal_view::Clock::new(heard, 0);
    assert!(text.contains(&clock.when(heard)), "{text}");
}

#[tokio::test(start_paused = true)]
async fn a_failure_to_reach_the_agent_that_is_not_a_missing_one_is_no_agent_and_starts_nothing() {
    let world = World::default();
    world.always(Answer::Failed);
    let session = start(world);
    session.wait(secs(60)).await;
    assert_eq!(
        what(&session.screen.view()),
        "Marshal cannot reach its agent"
    );
    assert!(
        session.world.started().is_empty(),
        "an agent that may exist is not started over"
    );
    assert_eq!(session.world.dials(), 1);
}
