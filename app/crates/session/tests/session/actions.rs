//! the actions of the window and the menu: each shows its transition at once, goes to the
//! agent as one call, and is forgotten when the snapshot shows it or the agent refuses.

use marshal_session::{Act, Failure};
use marshal_view::{Action, BannerKind, IconState};

use crate::support::{Agent, Answer, Called, NOW_MS, World, fixture, patched, secs, start, what};

fn menu_available(view: &marshal_view::View, action: &Action) -> bool {
    view.menu
        .items
        .iter()
        .find(|item| &item.action == action)
        .unwrap()
        .available
}

async fn connected(name: &str) -> (Agent, crate::support::Running) {
    let (agent, link) = Agent::new();
    let world = World::default();
    world.then(Answer::Agent(link));
    let session = start(world);
    agent.push(fixture(name));
    session.settle().await;
    (agent, session)
}

#[tokio::test(start_paused = true)]
async fn start_all_shows_its_transition_before_the_agent_has_answered() {
    let (agent, session) = connected("stopped_by_you").await;
    assert!(menu_available(&session.screen.view(), &Action::StartAll));
    let gate = agent.hold("start_all");
    let handle = session.handle.clone();
    let call = tokio::spawn(async move { handle.act(Act::StartAll).await });
    session.settle().await;
    let view = session.screen.view();
    assert!(!menu_available(&view, &Action::StartAll), "it is starting");
    assert_eq!(view.banners()[0].kind, BannerKind::Starting);
    assert_eq!(agent.calls(), [Called::StartAll]);
    gate.notify_one();
    call.await.unwrap().unwrap();
}

#[tokio::test(start_paused = true)]
async fn an_action_the_agent_refuses_is_taken_back_and_its_words_are_returned() {
    let (agent, session) = connected("stopped_by_you").await;
    agent.answer("start_all", Err(Failure::Busy("busy".to_string())));
    let error = session.handle.act(Act::StartAll).await.unwrap_err();
    assert!(error.contains("busy"), "{error}");
    session.settle().await;
    assert!(menu_available(&session.screen.view(), &Action::StartAll));
    assert_ne!(
        session.screen.view().banners()[0].kind,
        BannerKind::Starting
    );
}

#[tokio::test(start_paused = true)]
async fn stopping_the_build_is_the_agents_stop_all_and_shows_the_build_stopping() {
    let (agent, session) = connected("building").await;
    let gate = agent.hold("stop_all");
    let handle = session.handle.clone();
    let call = tokio::spawn(async move { handle.act(Act::StopBuild).await });
    session.settle().await;
    assert_eq!(agent.calls(), [Called::StopAll]);
    let banner = session.screen.view().banners()[0].heading.clone();
    assert_eq!(banner, "Stopping the build");
    gate.notify_one();
    call.await.unwrap().unwrap();
}

#[tokio::test(start_paused = true)]
async fn each_action_is_the_agents_method_of_its_name() {
    for (name, act, expected) in [
        ("stopped_by_you", Act::StartAll, Called::StartAll),
        ("running", Act::StopAll, Called::StopAll),
        ("stopped_by_you", Act::Build, Called::Build),
    ] {
        let (agent, session) = connected(name).await;
        session.handle.act(act).await.unwrap();
        assert_eq!(agent.calls(), [expected]);
    }
}

#[tokio::test(start_paused = true)]
async fn an_action_without_a_connection_is_refused_without_a_call() {
    let session = start(World::default());
    session.wait(secs(30)).await;
    let error = session.handle.act(Act::StopAll).await.unwrap_err();
    assert!(error.contains("lost contact"), "{error}");
}

#[tokio::test(start_paused = true)]
async fn try_again_is_refused_when_nothing_waits_for_it() {
    let (_agent, session) = connected("running").await;
    assert!(session.handle.try_again().await.is_err());
}

#[tokio::test(start_paused = true)]
async fn a_snapshot_that_shows_the_action_ends_the_transition() {
    let (agent, session) = connected("stopped_by_you").await;
    session.handle.act(Act::StartAll).await.unwrap();
    session.settle().await;
    assert_eq!(
        session.screen.view().banners()[0].kind,
        BannerKind::Starting
    );
    // the agent is resuming, then stops again for a reason: the start is over, and the
    // window says why it stopped, not that it is starting
    agent.push(patched("stopped_no_pool_started", |value| {
        value["agent"]["since"] = (NOW_MS + 2_000).into();
    }));
    session.settle().await;
    assert_ne!(
        session.screen.view().banners()[0].kind,
        BannerKind::Starting
    );
}

#[tokio::test(start_paused = true)]
async fn a_snapshot_from_before_the_action_does_not_end_the_transition() {
    let (agent, session) = connected("stopped_by_you").await;
    session.handle.act(Act::StartAll).await.unwrap();
    agent.push(fixture("stopped_by_you"));
    session.settle().await;
    assert_eq!(
        session.screen.view().banners()[0].kind,
        BannerKind::Starting
    );
}

#[tokio::test(start_paused = true)]
async fn an_answered_action_the_snapshot_never_shows_stops_being_shown() {
    let (_agent, session) = connected("stopped_by_you").await;
    session.handle.act(Act::StartAll).await.unwrap();
    session.settle().await;
    assert_eq!(
        session.screen.view().banners()[0].kind,
        BannerKind::Starting
    );
    session.wait(secs(6)).await;
    assert_ne!(
        session.screen.view().banners()[0].kind,
        BannerKind::Starting
    );
}

#[tokio::test(start_paused = true)]
async fn a_dropped_connection_forgets_the_action() {
    let (agent, session) = connected("stopped_by_you").await;
    let gate = agent.hold("start_all");
    let handle = session.handle.clone();
    let _call = tokio::spawn(async move { handle.act(Act::StartAll).await });
    session.settle().await;
    agent.close();
    session.settle().await;
    assert_eq!(what(&session.screen.view()), "lost contact (dropped)");
    drop(gate);
}

#[tokio::test(start_paused = true)]
async fn the_view_moves_with_the_clock_while_a_build_runs() {
    let (_agent, session) = connected("building").await;
    let before = session.screen.shown().len();
    session.wait(secs(5)).await;
    let shown = session.screen.shown();
    assert!(shown.len() >= before + 4, "{} then {}", before, shown.len());
    let first = format!("{:?}", shown[before - 1].view.banners());
    let last = format!("{:?}", shown.last().unwrap().view.banners());
    assert_ne!(first, last, "the elapsed time moved");
}

#[tokio::test(start_paused = true)]
async fn the_settled_count_shows_a_rise_at_once_and_a_fall_after_thirty_seconds() {
    let (agent, session) = connected("all_up").await;
    assert_eq!(session.screen.view().icon, IconState::AllUp);
    agent.push(patched("all_up", |value| value["runners_up"] = 2.into()));
    session.settle().await;
    assert_eq!(
        session.screen.view().icon,
        IconState::AllUp,
        "a fall is held"
    );
    session.wait(secs(25)).await;
    assert_eq!(session.screen.view().icon, IconState::AllUp);
    // no snapshot comes in the meantime: the clock alone brings it
    session.wait(secs(7)).await;
    assert_eq!(session.screen.view().icon, IconState::SomeNotUp);
    agent.push(fixture("all_up"));
    session.settle().await;
    assert_eq!(
        session.screen.view().icon,
        IconState::AllUp,
        "a rise is at once"
    );
}

#[tokio::test(start_paused = true)]
async fn a_banner_that_appears_is_announced_once() {
    let (agent, session) = connected("running").await;
    let before = session.screen.announced().len();
    agent.close();
    session.settle().await;
    let said: Vec<String> = session.screen.announced().split_off(before);
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(said[0].contains("contact"), "{said:?}");
    session.wait(secs(3)).await;
    assert_eq!(session.screen.announced().len(), before + 1);
}
