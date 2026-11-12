//! the session's part in an update: the checks, the drain it asks for, the wait for the
//! agent's process, the record of "stopped by you", the installer, an older agent, and
//! every end of the agent that installs nothing.

use marshal_cli::agent::Launch;
use marshal_session::{Act, AgentInfo, CHECK_EVERY, EXIT_WITHIN, Ended, Failure};

use crate::support::{
    Agent, Answer, Called, RecordEvent, Running, Updates, World, fixture, patched, secs, start_as,
    start_linking, what,
};

const PID: u32 = 4242;

/// an app at 1.3.0 connected to its agent, also at 1.3.0, which is running; the feed's
/// first check finds 1.4.0.
async fn offered() -> (Agent, Running) {
    let (agent, connected) = Agent::new();
    agent.answer_pid(PID);
    let world = World::default();
    world.then(Answer::Agent(connected));
    let updates = Updates::default();
    updates.offer("1.4.0").process(PID);
    let session = start_as(world, updates, "1.3.0", Launch::Running);
    agent.push(fixture("running"));
    session.settle().await;
    (agent, session)
}

fn drains(agent: &Agent) -> Vec<String> {
    agent
        .calls()
        .into_iter()
        .filter_map(|call| match call {
            Called::Drain { version } => Some(version),
            _ => None,
        })
        .collect()
}

#[tokio::test(start_paused = true)]
async fn a_verified_update_drains_the_agent_then_installs_once_its_process_exits() {
    let (agent, session) = offered().await;
    assert_eq!(drains(&agent), ["1.4.0"]);
    agent.end_for_update("1.4.0", false);
    agent.close();
    session.wait(secs(5)).await;
    assert!(
        session.updates.installed().is_empty(),
        "the agent still runs"
    );
    session.updates.exit(PID);
    session.settle().await;
    assert_eq!(session.updates.installed(), ["1.4.0"]);
    assert!(
        !session
            .updates
            .record()
            .contains(&RecordEvent::Written("1.4.0".into())),
        "the runners were running, so no record"
    );
    assert!(session.world.started().is_empty(), "no agent is started");
    assert_eq!(session.ended().await, Ended::Installing("1.4.0".into()));
}

#[tokio::test(start_paused = true)]
async fn stopped_by_you_is_recorded_for_the_update_before_it_installs() {
    let (agent, session) = offered().await;
    agent.end_for_update("1.4.0", true);
    agent.close();
    session.updates.exit(PID);
    session.settle().await;
    assert_eq!(
        session.updates.record(),
        [RecordEvent::Written("1.4.0".into())]
    );
    assert_eq!(session.updates.installed(), ["1.4.0"]);
}

#[tokio::test(start_paused = true)]
async fn a_process_that_does_not_exit_in_60_s_installs_nothing_and_the_agent_starts_again() {
    let (agent, session) = offered().await;
    agent.end_for_update("1.4.0", false);
    agent.close();
    session.wait(EXIT_WITHIN + secs(1)).await;
    assert!(session.updates.installed().is_empty());
    assert!(session.updates.record().contains(&RecordEvent::Removed));
    assert_eq!(
        session.world.started(),
        [Launch::Running],
        "as after any drop, it is started once more"
    );
    assert!(!session.is_over());
    // the feed is read again an hour later
    let checks = session.updates.checks(session.started_at).len();
    session.wait(CHECK_EVERY).await;
    assert_eq!(session.updates.checks(session.started_at).len(), checks + 1);
}

#[tokio::test(start_paused = true)]
async fn an_installer_that_does_not_start_drops_the_update() {
    let (agent, session) = offered().await;
    session.updates.refuse_install("access denied");
    agent.end_for_update("1.4.0", false);
    agent.close();
    session.updates.exit(PID);
    session.wait(secs(1)).await;
    assert!(session.updates.installed().is_empty());
    assert!(session.updates.record().contains(&RecordEvent::Removed));
    assert_eq!(session.world.started(), [Launch::Running]);
    assert!(!session.is_over());
}

#[tokio::test(start_paused = true)]
async fn a_process_that_does_not_exit_starts_the_agent_again_stopped_as_ending_said() {
    let (agent, session) = offered().await;
    agent.end_for_update("1.4.0", true);
    agent.close();
    session.wait(EXIT_WITHIN + secs(1)).await;
    assert!(session.updates.installed().is_empty());
    assert_eq!(
        session.world.started(),
        [Launch::Stopped],
        "the last snapshot showed the runners running, but `ending` said stopped by you"
    );
}

#[tokio::test(start_paused = true)]
async fn the_snapshot_that_settles_an_action_lets_the_held_update_drain() {
    let (agent, connected) = Agent::new();
    agent.answer_pid(PID);
    let world = World::default();
    world.then(Answer::Agent(connected));
    let updates = Updates::default();
    updates.offer("1.4.0").process(PID);
    let session = start_as(world, updates, "1.3.0", Launch::Running);
    session.settle().await;
    session.handle.act(Act::StopAll).await.unwrap();
    // the agent's last snapshot, a single `stopped` after Stop all, settles the action
    agent.push(fixture("stopped_by_you"));
    session.settle().await;
    assert_eq!(drains(&agent), ["1.4.0"]);
}

#[tokio::test(start_paused = true)]
async fn another_clients_drain_is_waited_for_and_its_end_installs_nothing() {
    let (agent, connected) = Agent::new();
    agent.answer_pid(PID);
    let world = World::default();
    world.then(Answer::Agent(connected));
    let updates = Updates::default();
    updates.offer("1.4.0").process(PID);
    let session = start_as(world, updates, "1.3.0", Launch::Running);
    agent.push(patched("running", |value| {
        value["agent"]["update"] =
            serde_json::json!({ "version": "1.5.0", "waiting": 1, "deadline": 0, "pid": PID });
    }));
    session.settle().await;
    assert!(
        drains(&agent).is_empty(),
        "another client's drain is under way"
    );
    agent.end_for_update("1.5.0", false);
    agent.close();
    session.updates.exit(PID);
    session.wait(secs(1)).await;
    assert!(session.updates.installed().is_empty());
    assert_eq!(
        session.world.started(),
        [Launch::Running],
        "handled as any drop"
    );
}

#[tokio::test(start_paused = true)]
async fn another_clients_update_from_stopped_by_you_starts_the_agent_again_stopped() {
    let (agent, connected) = Agent::new();
    let world = World::default();
    world.then(Answer::Agent(connected));
    let session = start_as(world, Updates::default(), "1.3.0", Launch::Running);
    agent.push(fixture("running"));
    session.settle().await;
    agent.end_for_update("1.5.0", true);
    agent.close();
    session.wait(secs(1)).await;
    assert_eq!(session.world.started(), [Launch::Stopped]);
}

#[tokio::test(start_paused = true)]
async fn an_agent_at_the_librarys_version_is_not_replaced_whatever_the_apps_version() {
    let (agent, connected) = Agent::reporting(AgentInfo {
        version: "1.3.0".into(),
        drains: true,
    });
    let world = World::default();
    world.then(Answer::Agent(connected));
    let session = start_linking(world, Updates::default(), "1.4.0", "1.3.0", Launch::Running);
    agent.push(fixture("running"));
    session.settle().await;
    assert!(drains(&agent).is_empty(), "it is the agent this app starts");
}

#[tokio::test(start_paused = true)]
async fn an_unknown_end_during_the_drain_installs_nothing() {
    let (agent, session) = offered().await;
    agent.end("signal_sleep");
    agent.close();
    session.updates.exit(PID);
    session.wait(secs(1)).await;
    assert!(session.updates.installed().is_empty());
    assert_eq!(session.world.started(), [Launch::Running]);
}

#[tokio::test(start_paused = true)]
async fn the_feed_is_read_at_launch_and_hourly_but_not_while_an_update_is_held() {
    let (agent, connected) = Agent::new();
    let world = World::default();
    world.then(Answer::Agent(connected));
    let updates = Updates::default();
    updates.fail_check("timed out");
    let session = start_as(world, updates, "1.3.0", Launch::Running);
    // the agent is building, so a verified update waits for it to settle
    agent.push(fixture("building"));
    session.settle().await;
    assert_eq!(session.updates.checks(session.started_at), [0]);
    assert!(
        session
            .log
            .lines()
            .iter()
            .any(|line| line.contains("timed out"))
    );
    session.updates.offer("1.4.0");
    session.wait(CHECK_EVERY + secs(1)).await;
    assert_eq!(session.updates.checks(session.started_at), [0, 3600]);
    assert!(drains(&agent).is_empty(), "no drain while the agent builds");
    session.wait(CHECK_EVERY * 3).await;
    assert_eq!(
        session.updates.checks(session.started_at),
        [0, 3600],
        "no check while an update is held"
    );
    agent.push(fixture("running"));
    session.settle().await;
    assert_eq!(drains(&agent), ["1.4.0"]);
}

#[tokio::test(start_paused = true)]
async fn a_development_build_never_reads_the_feed() {
    let (agent, connected) = Agent::new();
    let world = World::default();
    world.then(Answer::Agent(connected));
    let updates = Updates::default();
    updates.offer("1.4.0");
    let session = start_as(world, updates, "0.0.0-dev", Launch::Running);
    agent.push(fixture("running"));
    session.wait(CHECK_EVERY * 2).await;
    assert!(session.updates.checks(session.started_at).is_empty());
    assert!(drains(&agent).is_empty());
}

#[tokio::test(start_paused = true)]
async fn a_refused_drain_is_asked_for_again() {
    let (agent, connected) = Agent::new();
    agent.answer(
        "drain_for_update",
        Err(Failure::Refused("the agent is resuming".into())),
    );
    let world = World::default();
    world.then(Answer::Agent(connected));
    let updates = Updates::default();
    updates.offer("1.4.0");
    let session = start_as(world, updates, "1.3.0", Launch::Running);
    agent.push(fixture("running"));
    session.settle().await;
    assert_eq!(drains(&agent).len(), 1);
    session.wait(secs(61)).await;
    assert_eq!(drains(&agent).len(), 2);
}

#[tokio::test(start_paused = true)]
async fn a_lost_connection_with_the_agent_still_running_reconnects_and_asks_again() {
    let (agent, session) = offered().await;
    let (again, connected) = Agent::new();
    session.world.then(Answer::Agent(connected));
    agent.close();
    session.settle().await;
    assert!(
        session.world.started().is_empty(),
        "the running agent is not started again"
    );
    again.push(fixture("running"));
    session.settle().await;
    assert_eq!(drains(&again), ["1.4.0"]);
    assert!(session.updates.installed().is_empty());
}

#[tokio::test(start_paused = true)]
async fn a_lost_connection_with_no_agent_answering_waits_for_its_process_then_installs() {
    let (agent, session) = offered().await;
    session.world.then(Answer::NoAgent);
    agent.close();
    session.settle().await;
    assert!(
        session.updates.installed().is_empty(),
        "its process still runs"
    );
    assert_eq!(
        what(&session.screen.view()),
        "lost contact (dropped)",
        "nothing was started, and Try Again is not taken while the app waits"
    );
    session.updates.exit(PID);
    session.settle().await;
    assert!(
        session.world.started().is_empty(),
        "the agent is not started again"
    );
    assert_eq!(session.updates.installed(), ["1.4.0"]);
}

#[tokio::test(start_paused = true)]
async fn a_lost_connection_with_the_agent_exited_installs_keeping_stopped_by_you() {
    let (agent, session) = offered().await;
    agent.push(patched("stopped_by_you", |_| {}));
    session.settle().await;
    session.updates.exit(PID);
    agent.close();
    session.settle().await;
    assert!(
        session.world.started().is_empty(),
        "the agent is not started again"
    );
    assert_eq!(
        session.updates.record(),
        [RecordEvent::Written("1.4.0".into())]
    );
    assert_eq!(session.updates.installed(), ["1.4.0"]);
}

#[tokio::test(start_paused = true)]
async fn quit_during_a_drain_installs_nothing() {
    let (agent, session) = offered().await;
    session.handle.quit();
    session.settle().await;
    agent.end("quit");
    agent.close();
    session.updates.exit(PID);
    assert_eq!(session.ended().await, Ended::AgentEnded("quit".into()));
}

#[tokio::test(start_paused = true)]
async fn an_older_agent_that_drains_is_replaced_keeping_stopped_by_you() {
    let (agent, connected) = Agent::reporting(AgentInfo {
        version: "1.2.0".into(),
        drains: true,
    });
    agent.answer_pid(PID);
    let world = World::default();
    world.then(Answer::Agent(connected));
    let updates = Updates::default();
    updates.process(PID);
    let session = start_as(world, updates, "1.3.0", Launch::Running);
    agent.push(fixture("stopped_by_you"));
    session.settle().await;
    assert_eq!(
        drains(&agent),
        ["1.3.0"],
        "drained for the version of the app's own agent"
    );
    agent.end_for_update("1.3.0", true);
    agent.close();
    session.updates.exit(PID);
    session.wait(secs(1)).await;
    assert_eq!(session.world.started(), [Launch::Stopped]);
    assert!(session.updates.installed().is_empty());
}

#[tokio::test(start_paused = true)]
async fn an_older_agent_without_the_drain_is_used_as_it_is_and_nothing_installs() {
    let (agent, connected) = Agent::reporting(AgentInfo {
        version: "1.2.0".into(),
        drains: false,
    });
    let world = World::default();
    world.then(Answer::Agent(connected));
    let updates = Updates::default();
    updates.offer("1.4.0");
    let session = start_as(world, updates, "1.3.0", Launch::Running);
    agent.push(fixture("running"));
    session.wait(secs(5)).await;
    agent.push(fixture("running"));
    session.settle().await;
    assert!(drains(&agent).is_empty());
    let told = session
        .log
        .lines()
        .iter()
        .filter(|line| line.contains("cannot drain"))
        .count();
    assert_eq!(told, 1, "logged once");
    assert!(session.updates.installed().is_empty());
}

#[tokio::test(start_paused = true)]
async fn a_launch_after_an_update_from_stopped_by_you_starts_the_agent_stopped() {
    let world = World::default();
    let session = start_as(world, Updates::default(), "1.4.0", Launch::Stopped);
    session.wait(secs(1)).await;
    assert_eq!(session.world.started(), [Launch::Stopped]);
}

#[test]
fn the_record_applies_once_to_its_version_within_30_minutes() {
    use marshal_session::{RECORD_APPLIES_FOR, RECORD_FILE, Record, RecordFile};
    let directory = crate::support::temp_dir("record");
    let record = RecordFile::new(directory.join(RECORD_FILE));
    let written = 1_000_000;
    let limit = u64::try_from(RECORD_APPLIES_FOR.as_millis()).unwrap();
    let cases = [
        ("1.4.0", written + limit, true),
        ("1.4.0", written + limit + 1, false),
        ("1.3.0", written + 1, false),
        // a clock that moved back does not make a record apply
        ("1.4.0", written - 1, false),
    ];
    for (version, now, applies) in cases {
        record.write("1.4.0", written).unwrap();
        assert_eq!(record.take(version, now), applies, "{version} at {now}");
        assert!(
            !directory.join(RECORD_FILE).exists(),
            "every launch removes the record"
        );
        assert!(!record.take("1.4.0", written), "it applies once");
    }
    std::fs::write(directory.join(RECORD_FILE), "not a record").unwrap();
    assert!(!record.take("1.4.0", written));
    assert!(!directory.join(RECORD_FILE).exists());
}

#[tokio::test(start_paused = true)]
async fn stopped_by_you_survives_a_first_attempt_that_starts_nothing() {
    let world = World::default();
    world.then(Answer::LockHeld);
    let session = start_as(world, Updates::default(), "1.4.0", Launch::Stopped);
    session.wait(secs(6)).await;
    assert_eq!(session.world.started(), [Launch::Stopped]);
}
