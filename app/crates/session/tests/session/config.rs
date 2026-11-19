//! the editor's path to the agent's configuration methods: a request reaches the agent
//! as it was made, its answer and its failure come back whole, and under lost contact
//! nothing is sent.

use marshal_cli::agent::config::{ConfigRead, Problem};
use marshal_session::{ConfigAnswer, ConfigRequest, Failure};

use crate::support::{Agent, Answer, Called, World, fixture, start};

fn read() -> ConfigRead {
    serde_json::from_value(serde_json::json!({
        "revision": "r1",
        "status": "valid",
        "settings": {"routing_label": "axpc"},
        "pools": [{"name": "ci"}],
    }))
    .unwrap()
}

#[tokio::test(start_paused = true)]
async fn a_request_reaches_the_agent_and_its_answer_or_refusal_comes_back_whole() {
    let (agent, link) = Agent::new();
    let world = World::default();
    world.then(Answer::Agent(link));
    let session = start(world);
    agent.push(fixture("running"));
    session.settle().await;

    agent.answer_config(Ok(ConfigAnswer::Read(read())));
    let answer = session.handle.config(ConfigRequest::Read).await.unwrap();
    assert_eq!(answer, ConfigAnswer::Read(read()));

    let problem: Problem = serde_json::from_value(serde_json::json!({
        "path": "pool[0].runners", "line": 9, "message": "must be at least 1"
    }))
    .unwrap();
    let refusal = Failure::Config {
        code: "config_invalid".into(),
        message: "the result breaks the rules".into(),
        problems: vec![problem],
    };
    agent.answer_config(Err(refusal.clone()));
    let edit = ConfigRequest::Edit {
        revision: "r1".into(),
        edits: serde_json::json!([{"op": "set", "path": "pool[0].runners", "value": 0}]),
        check: true,
    };
    assert_eq!(session.handle.config(edit.clone()).await, Err(refusal));
    assert_eq!(
        agent.calls(),
        [
            Called::Config(format!("{:?}", ConfigRequest::Read)),
            Called::Config(format!("{edit:?}")),
        ]
    );
}

#[tokio::test(start_paused = true)]
async fn without_an_agent_a_request_is_lost_contact_and_sends_nothing() {
    let session = start(World::default());
    session.settle().await;
    assert_eq!(
        session.handle.config(ConfigRequest::Read).await,
        Err(Failure::Lost)
    );
}

#[tokio::test(start_paused = true)]
async fn released_token_removal_returns_the_store_cause_and_a_retry_can_succeed() {
    let (agent, link) = Agent::new();
    let world = World::default();
    world.then(Answer::Agent(link));
    let session = start(world);
    agent.push(fixture("running"));
    session.settle().await;
    let request = ConfigRequest::RemoveReleasedToken {
        repository: "owner/released".into(),
    };
    let failure = Failure::Store {
        detail: Some("kind access_refused, code 5".into()),
    };
    agent.answer_config(Err(failure.clone()));
    assert_eq!(session.handle.config(request.clone()).await, Err(failure));
    agent.answer_config(Ok(ConfigAnswer::TokenRemoved));
    assert_eq!(
        session.handle.config(request).await,
        Ok(ConfigAnswer::TokenRemoved)
    );
}
