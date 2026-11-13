//! the `github` subscription: held on a second connection only while the window is
//! shown, renewed every 30 s, subscribed again when the agent ended it, asked for again
//! every 60 s when the agent has no room, and none at all from an agent without the
//! topic; and Open run's address, or a job that has ended.

use marshal_cli::github_status::GithubStatus;
use marshal_session::{Failure, GITHUB_ASK_AGAIN, RENEW_EVERY};
use marshal_view::View;

use crate::support::{Agent, Answer, Called, NOW_MS, Running, World, fixture, secs, start};

/// the recorded `github` value, with every repository checked at `checked_at`.
fn github(checked_at: u64) -> GithubStatus {
    let path = format!(
        "{}/../view/tests/fixtures/github/running.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let mut value: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    for repository in value["repositories"].as_array_mut().unwrap() {
        repository["checked_at"] = checked_at.into();
    }
    serde_json::from_value(value).unwrap()
}

/// a session connected to an agent that pushed the `running` snapshot, with a second
/// agent connection waiting for the `github` subscription.
async fn connected() -> (Running, Agent, Agent) {
    let (main, first) = Agent::new();
    let (side, second) = Agent::new();
    let world = World::default();
    world.then(Answer::Agent(first)).then(Answer::Agent(second));
    let session = start(world);
    session.settle().await;
    main.push(fixture("running"));
    session.settle().await;
    (session, main, side)
}

fn rows(view: &View) -> Vec<(String, Option<String>)> {
    view.list()
        .unwrap()
        .pools
        .iter()
        .flat_map(|card| &card.runners)
        .map(|row| {
            (
                row.container.clone().unwrap_or_default(),
                row.github.clone(),
            )
        })
        .collect()
}

fn github_calls(agent: &Agent) -> Vec<Called> {
    agent
        .calls()
        .into_iter()
        .filter(|call| matches!(call, Called::SubscribeGithub | Called::RenewGithub))
        .collect()
}

#[tokio::test(start_paused = true)]
async fn nothing_is_subscribed_while_the_window_is_hidden_and_showing_it_subscribes_on_a_second_connection()
 {
    let (session, main, side) = connected().await;
    session.wait(secs(120)).await;
    assert_eq!(
        session.world.dials(),
        1,
        "no second connection while hidden"
    );
    assert!(github_calls(&main).is_empty());
    assert!(
        rows(&session.screen.view())
            .iter()
            .all(|(_, github)| github.is_none()),
        "no GitHub state while hidden"
    );

    session.handle.window_shown(true);
    session.settle().await;
    assert_eq!(session.world.dials(), 2);
    assert_eq!(github_calls(&side), [Called::SubscribeGithub]);
    assert!(
        github_calls(&main).is_empty(),
        "the state connection is left as it was"
    );
    let view = session.screen.view();
    assert_eq!(
        rows(&view)[0].1.as_deref(),
        Some("GitHub: checking…"),
        "a runner reads as checking until GitHub answers"
    );
    assert_eq!(
        view.window.footer.github,
        ["GitHub requests: not known yet"]
    );

    session.handle.window_shown(false);
    session.settle().await;
    assert!(
        side.let_go(),
        "hiding the window closes the connection, which ends the subscription"
    );
    assert!(!main.let_go());
    assert!(session.screen.view().window.footer.github.is_empty());
}

#[tokio::test(start_paused = true)]
async fn the_value_the_agent_sent_before_the_window_opened_reads_as_checking_and_a_later_one_shows()
{
    let (session, _main, side) = connected().await;
    session.handle.window_shown(true);
    session.settle().await;
    side.push_github(github(NOW_MS - 20_000));
    session.settle().await;
    assert!(
        rows(&session.screen.view())
            .iter()
            .filter_map(|(_, github)| github.as_deref())
            .all(|github| github == "GitHub: checking…"),
        "{:?}",
        rows(&session.screen.view())
    );

    session.wait(secs(10)).await;
    side.push_github(github(NOW_MS + 9_000));
    session.settle().await;
    let view = session.screen.view();
    assert_eq!(
        rows(&view)[..3],
        [
            (
                "app-1-20261104031522114".to_string(),
                Some("GitHub: running a job".to_string())
            ),
            (
                "app-2-20261104031530871".to_string(),
                Some("GitHub: idle".to_string())
            ),
            (
                "site-1-20261104033907412".to_string(),
                Some("GitHub: running a job".to_string())
            ),
        ]
    );
    assert!(
        view.list()
            .unwrap()
            .header
            .line
            .ends_with(" · 2 running jobs"),
        "{}",
        view.list().unwrap().header.line
    );
    assert!(
        !view
            .menu
            .items
            .iter()
            .any(|item| item.label.contains("running job")),
        "the menu shows no GitHub status"
    );
}

#[tokio::test(start_paused = true)]
async fn the_subscription_is_renewed_every_30_s_and_subscribed_again_when_the_agent_ended_it() {
    let (session, _main, side) = connected().await;
    session.handle.window_shown(true);
    session.settle().await;
    side.renew_answer(Ok(()));
    side.renew_answer(Err(Failure::NotSubscribed));
    session.wait(RENEW_EVERY + secs(1)).await;
    assert_eq!(
        github_calls(&side),
        [Called::SubscribeGithub, Called::RenewGithub]
    );
    session.wait(RENEW_EVERY).await;
    assert_eq!(
        github_calls(&side),
        [
            Called::SubscribeGithub,
            Called::RenewGithub,
            Called::RenewGithub,
            Called::SubscribeGithub
        ],
        "an ended subscription is taken again on the same connection"
    );
    assert!(!side.let_go());
    assert_eq!(session.world.dials(), 2);
}

#[tokio::test(start_paused = true)]
async fn a_subscription_the_agent_has_no_room_for_is_asked_for_again_every_60_s() {
    let (main, first) = Agent::new();
    let (full, second) = Agent::new();
    let (side, third) = Agent::new();
    full.github_answer(Err(Failure::Busy("no room".to_string())));
    let world = World::default();
    world
        .then(Answer::Agent(first))
        .then(Answer::Agent(second))
        .then(Answer::Agent(third));
    let session = start(world);
    session.settle().await;
    main.push(fixture("running"));
    session.settle().await;
    session.handle.window_shown(true);
    session.settle().await;
    assert_eq!(
        session.screen.view().window.footer.github,
        [
            "GitHub status unavailable for now: the agent has no room for another subscription. Asking again every 60 s."
        ]
    );
    assert!(
        rows(&session.screen.view())
            .iter()
            .all(|(_, github)| github.is_none())
    );
    // it waits its turn, 5 s after the region was last written to
    session.wait(secs(6)).await;
    assert!(
        session
            .screen
            .announced()
            .iter()
            .any(|said| said.starts_with("GitHub status unavailable for now")),
        "it is announced"
    );

    session.wait(GITHUB_ASK_AGAIN - secs(7)).await;
    assert_eq!(
        session.world.dials(),
        2,
        "not asked again within the minute"
    );
    session.wait(secs(2)).await;
    assert_eq!(session.world.dials(), 3);
    assert_eq!(github_calls(&side), [Called::SubscribeGithub]);
    assert_eq!(
        session.screen.view().window.footer.github,
        ["GitHub requests: not known yet"]
    );
}

#[tokio::test(start_paused = true)]
async fn an_agent_without_the_topic_gets_no_more_asking_and_the_window_says_so() {
    let (main, first) = Agent::new();
    let (side, second) = Agent::new();
    side.github_answer(Err(Failure::NotServed("the topic github".to_string())));
    let world = World::default();
    world.then(Answer::Agent(first)).then(Answer::Agent(second));
    let session = start(world);
    session.settle().await;
    main.push(fixture("running"));
    session.settle().await;
    session.handle.window_shown(true);
    session.settle().await;
    assert_eq!(
        session.screen.view().window.footer.github,
        ["GitHub status unavailable: the running agent does not report it."]
    );
    session.handle.window_shown(false);
    session.handle.window_shown(true);
    session.wait(secs(300)).await;
    assert_eq!(
        session.world.dials(),
        2,
        "not asked again until another agent is reached"
    );
    assert_eq!(
        session.screen.view().window.footer.github,
        ["GitHub status unavailable: the running agent does not report it."]
    );
}

#[tokio::test(start_paused = true)]
async fn open_run_gets_the_job_page_built_from_the_pools_repository_or_says_the_job_ended() {
    let (session, _main, side) = connected().await;
    session.handle.window_shown(true);
    session.settle().await;
    side.push_github(github(NOW_MS + 1_000));
    session.settle().await;

    let page = session
        .handle
        .open_job("app".to_string(), "app-1-20261104031522114".to_string())
        .await;
    assert_eq!(
        page,
        Ok(Some(
            "https://github.com/owner/app/actions/runs/123/job/456".to_string()
        ))
    );

    // an idle runner, a runner of another pool, and one no longer listed have no job
    for (pool, runner) in [
        ("app", "app-2-20261104031530871"),
        ("app", "site-1-20261104033907412"),
        ("app", "app-1-20261104000000000"),
    ] {
        let page = session
            .handle
            .open_job(pool.to_string(), runner.to_string())
            .await;
        assert_eq!(page, Ok(None), "{runner}");
    }
    session.settle().await;
    let view = session.screen.view();
    let card = &view.list().unwrap().pools[0];
    assert!(
        card.github_notes
            .iter()
            .any(|note| note.text == "That job has ended."),
        "{:?}",
        card.github_notes
    );

    let refused = session
        .handle
        .open_job(
            "elsewhere".to_string(),
            "app-1-20261104031522114".to_string(),
        )
        .await;
    assert_eq!(refused, Err("Marshal has no such pool.".to_string()));
}

#[tokio::test(start_paused = true)]
async fn a_job_has_no_page_once_contact_is_lost() {
    let (session, main, side) = connected().await;
    session.handle.window_shown(true);
    session.settle().await;
    side.push_github(github(NOW_MS + 1_000));
    session.settle().await;
    main.close();
    session.settle().await;
    let view = session.screen.view();
    let row = &view.list().unwrap().pools[0].runners[0];
    assert_eq!(row.github.as_deref(), Some("GitHub: running a job"));
    assert!(
        row.open_run.is_none(),
        "Open run is not offered after contact is lost"
    );
    let page = session
        .handle
        .open_job("app".to_string(), "app-1-20261104031522114".to_string())
        .await;
    assert_eq!(
        page,
        Err("Marshal has lost contact with the agent.".to_string())
    );
}

#[tokio::test(start_paused = true)]
async fn that_job_has_ended_is_said_for_a_minute() {
    let (session, _main, side) = connected().await;
    session.handle.window_shown(true);
    session.settle().await;
    side.push_github(github(NOW_MS + 1_000));
    session.settle().await;
    let ended = |session: &Running| {
        session.screen.view().list().unwrap().pools[0]
            .github_notes
            .iter()
            .any(|note| note.text == "That job has ended.")
    };
    let page = session
        .handle
        .open_job("app".to_string(), "app-2-20261104031530871".to_string())
        .await;
    assert_eq!(page, Ok(None));
    session.settle().await;
    assert!(ended(&session));
    session.wait(secs(58)).await;
    assert!(ended(&session));
    session.wait(secs(3)).await;
    assert!(!ended(&session), "the notice goes after a minute");
}

#[tokio::test(start_paused = true)]
async fn a_subscription_taken_again_reads_as_checking_until_the_agent_checks_again() {
    let (session, _main, side) = connected().await;
    session.handle.window_shown(true);
    session.settle().await;
    side.push_github(github(NOW_MS + 1_000));
    session.settle().await;
    assert_eq!(
        rows(&session.screen.view())[0].1.as_deref(),
        Some("GitHub: running a job")
    );
    side.renew_answer(Err(Failure::NotSubscribed));
    session.wait(RENEW_EVERY + secs(1)).await;
    // the agent's full value on the new subscription is from before it stopped
    side.push_github(github(NOW_MS + 20_000));
    session.settle().await;
    assert_eq!(
        rows(&session.screen.view())[0].1.as_deref(),
        Some("GitHub: checking…")
    );
    session.wait(secs(5)).await;
    side.push_github(github(NOW_MS + 36_000));
    session.settle().await;
    assert_eq!(
        rows(&session.screen.view())[0].1.as_deref(),
        Some("GitHub: running a job")
    );
}
