//! the revision each shown view carries, which the page orders its views by.

use crate::support::{Agent, Answer, World, fixture, secs, start};

#[tokio::test(start_paused = true)]
async fn each_view_shown_has_a_higher_revision_than_the_last() {
    let (agent, connected) = Agent::new();
    let world = World::default();
    world.then(Answer::Agent(connected));
    let session = start(world);
    session.settle().await;
    agent.push(fixture("running"));
    session.settle().await;
    session.wait(secs(60)).await;
    let revisions: Vec<u64> = session
        .screen
        .shown()
        .iter()
        .map(|shown| shown.revision)
        .collect();
    assert!(revisions.len() >= 2, "{revisions:?}");
    assert!(
        revisions.windows(2).all(|pair| pair[0] < pair[1]),
        "{revisions:?}"
    );
}

#[tokio::test(start_paused = true)]
async fn a_read_carries_the_revision_of_the_view_last_shown() {
    let (agent, connected) = Agent::new();
    let world = World::default();
    world.then(Answer::Agent(connected));
    let session = start(world);
    session.settle().await;
    let early = session.handle.read_view(None).await.unwrap();
    assert_eq!(early.revision, session.screen.latest().revision);
    agent.push(fixture("running"));
    session.settle().await;
    let later = session.handle.read_view(None).await.unwrap();
    assert_eq!(later.revision, session.screen.latest().revision);
    assert!(later.revision > early.revision);
}
