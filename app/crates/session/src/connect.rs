//! one attempt to reach the agent: connect, and when none answers and the attempt may,
//! start it from the app's own executable and wait for it; then subscribe. What the
//! attempt found is the session's to act on.

use std::time::Duration;

use marshal_cli::agent::Launch;
use tokio::time::{Instant, sleep, timeout};

use crate::seam::{Connected, Dial, DialError, Failure, Launcher, Log};

/// `busy` and a held lock are asked about again this often.
pub const RETRY_EVERY: Duration = Duration::from_secs(5);

/// a subscription the agent has not accepted in this long means it is not answering.
const SUBSCRIBE_WITHIN: Duration = Duration::from_secs(5);

/// how long a started agent has to open its channel.
const START_WITHIN: Duration = Duration::from_secs(10);

/// how often the channel of a started agent is tried.
const START_POLL: Duration = Duration::from_millis(200);

/// what an attempt found.
pub enum Found<L> {
    /// connected and subscribed; the first snapshot follows.
    Connected(Connected<L>),
    /// no agent answered, and none could be started or came up.
    NoAgent,
    NotAnswering,
    Busy,
    LockHeld,
}

pub struct Attempt<L> {
    pub found: Found<L>,
    /// whether the attempt started the agent.
    pub started: bool,
}

/// reaches the agent; with `start`, a missing agent is started with that launch, once.
pub async fn attempt<D: Dial>(
    dial: &D,
    launcher: &dyn Launcher,
    start: Option<Launch>,
    log: &dyn Log,
) -> Attempt<D::Link> {
    let done = |found| Attempt {
        found,
        started: false,
    };
    match dial.dial().await {
        Ok(connected) => return done(subscribed(connected, log).await),
        Err(DialError::NoAgent) => {}
        Err(other) => return done(refused(&other, log)),
    }
    let Some(launch) = start else {
        return done(Found::NoAgent);
    };
    if let Err(why) = launcher.start_agent(launch) {
        log.warn(&format!("The agent was not started: {why}"));
        return done(Found::NoAgent);
    }
    log.info(&format!("Started the agent ({launch:?})."));
    let found = wait_for_agent(dial, log).await;
    Attempt {
        found,
        started: true,
    }
}

async fn wait_for_agent<D: Dial>(dial: &D, log: &dyn Log) -> Found<D::Link> {
    let deadline = Instant::now() + START_WITHIN;
    loop {
        sleep(START_POLL).await;
        match dial.dial().await {
            Ok(connected) => return subscribed(connected, log).await,
            Err(DialError::NoAgent) if Instant::now() < deadline => {}
            Err(DialError::NoAgent) => {
                log.warn("The agent was started and its channel did not open.");
                return Found::NoAgent;
            }
            Err(other) => return refused(&other, log),
        }
    }
}

fn refused<L>(error: &DialError, log: &dyn Log) -> Found<L> {
    match error {
        DialError::NoAgent => Found::NoAgent,
        DialError::NotAnswering => {
            log.warn("The agent did not complete the handshake in 5 s.");
            Found::NotAnswering
        }
        DialError::Busy => {
            log.info("The agent answered busy.");
            Found::Busy
        }
        DialError::LockHeld => {
            log.warn(
                "The channel does not answer protocol version 2; another process holds the lock.",
            );
            Found::LockHeld
        }
        DialError::Failed(detail) => {
            log.warn(&format!("The agent could not be reached: {detail}"));
            Found::NoAgent
        }
    }
}

async fn subscribed<L: crate::seam::Link>(connected: Connected<L>, log: &dyn Log) -> Found<L> {
    match timeout(SUBSCRIBE_WITHIN, connected.link.subscribe()).await {
        Ok(Ok(())) => Found::Connected(connected),
        Ok(Err(Failure::Busy(_))) => {
            log.info("The agent has all its subscriptions taken.");
            Found::Busy
        }
        Ok(Err(failure)) => {
            log.warn(&format!(
                "The agent refused the subscription: {}",
                failure.words()
            ));
            Found::NoAgent
        }
        Err(_) => {
            log.warn("The agent did not accept the subscription in 5 s.");
            Found::NotAnswering
        }
    }
}
