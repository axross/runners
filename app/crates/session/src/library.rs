//! the connection to the agent through the `marshal-cli` library's client: its handshake,
//! its subscription and its calls, read into the session's own types.

use marshal_cli::agent::wire::ClientInfo;
use marshal_cli::agent::{CallError, Client, ConnectError, Push, connect};
use marshal_cli::instance::roots::Roots;
use tokio::sync::mpsc;

use crate::seam::{AgentInfo, Connected, Dial, DialError, Failure, Link, Method, Pushed};

/// connects to this user's agent at the channel the roots name.
pub struct Library {
    roots: Roots,
    info: ClientInfo,
}

impl Library {
    #[must_use]
    pub fn new(roots: Roots, version: &str) -> Self {
        let mut info = ClientInfo::default();
        info.name = "marshal-app".to_string();
        info.version = version.to_string();
        Self { roots, info }
    }
}

/// a connected agent, reached through the library's client.
pub struct Agent {
    client: Client,
}

impl Dial for Library {
    type Link = Agent;

    async fn dial(&self) -> Result<Connected<Agent>, DialError> {
        let (client, pushes) = connect(&self.roots, &self.info)
            .await
            .map_err(|error| dial_error(&error))?;
        let agent = AgentInfo {
            version: client.welcome.agent_version.clone(),
            drains: client
                .welcome
                .methods
                .iter()
                .any(|method| method == DRAIN_FOR_UPDATE),
        };
        Ok(Connected {
            link: Agent { client },
            pushes: forward(pushes),
            agent,
        })
    }
}

/// the dial error for the library's: `busy` is the agent at its limit, and an agent that
/// does not speak protocol version 2, or a server that is not this user's, is a held lock
/// the app cannot talk to.
#[must_use]
pub fn dial_error(error: &ConnectError) -> DialError {
    match error {
        ConnectError::NoAgent => DialError::NoAgent,
        ConnectError::NotAnswering => DialError::NotAnswering,
        ConnectError::Refused(body) => match body.code.as_str() {
            "busy" | "subscription_limit" => DialError::Busy,
            "unsupported_protocol" => DialError::LockHeld,
            _ => DialError::Failed(body.message.clone()),
        },
        ConnectError::NotProtocol | ConnectError::ServerNotThisUser => DialError::LockHeld,
        other => DialError::Failed(other.to_string()),
    }
}

/// the failure for the library's call error.
#[must_use]
pub fn failure(error: CallError) -> Failure {
    match error {
        CallError::Agent(body) => match body.code.as_str() {
            "busy" | "subscription_limit" => Failure::Busy(body.message),
            "store_failed" => Failure::Store {
                detail: body.store.map(|store| match store.code {
                    Some(code) => format!("kind {}, code {code}", store.kind),
                    None => format!("kind {}", store.kind),
                }),
            },
            _ => Failure::Refused(body.message),
        },
        CallError::NotServed(method) => Failure::NotServed(method),
        CallError::Closed => Failure::Lost,
        CallError::BadAnswer(what) => Failure::Other(what),
        CallError::TooLong => Failure::Other("the request is longer than the agent reads".into()),
        other => Failure::Other(other.to_string()),
    }
}

/// reads the library's pushes into the session's, on a task that ends with the
/// connection.
fn forward(mut pushes: mpsc::UnboundedReceiver<Push>) -> mpsc::UnboundedReceiver<Pushed> {
    let (sender, receiver) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        while let Some(push) = pushes.recv().await {
            let pushed = match push {
                Push::Event { snapshot, .. } => Pushed::Snapshot(snapshot),
                Push::Heartbeat { .. } => Pushed::Heartbeat,
                Push::Ending { reason, update, .. } => Pushed::Ending { reason, update },
                Push::Closed => Pushed::Closed,
                // a push a later agent adds is ignored
                _ => continue,
            };
            let closed = matches!(pushed, Pushed::Closed);
            if sender.send(pushed).is_err() || closed {
                break;
            }
        }
    });
    receiver
}

impl Link for Agent {
    async fn subscribe(&self) -> Result<(), Failure> {
        self.client.subscribe().await.map_err(failure)
    }

    async fn call(&self, method: Method) -> Result<(), Failure> {
        let called = match method {
            Method::StartAll => self.client.call("start_all", serde_json::Value::Null).await,
            Method::StopAll => self.client.call("stop_all", serde_json::Value::Null).await,
            Method::Build => self.client.call("build", serde_json::Value::Null).await,
            Method::Quit => self.client.call("quit", serde_json::Value::Null).await,
            Method::TokenSet { repository, token } => self
                .client
                .token_set(&repository, token.expose())
                .await
                .map(|()| serde_json::Value::Null),
            Method::TokenRemove { repository } => self
                .client
                .token_remove(&repository)
                .await
                .map(|()| serde_json::Value::Null),
        };
        called.map(|_| ()).map_err(failure)
    }

    async fn drain_for_update(&self, version: &str) -> Result<Option<u32>, Failure> {
        let answer = self
            .client
            .call(DRAIN_FOR_UPDATE, serde_json::json!({ "version": version }))
            .await
            .map_err(failure)?;
        Ok(answer
            .get("update")
            .and_then(|update| update.get("pid"))
            .and_then(serde_json::Value::as_u64)
            .and_then(|pid| u32::try_from(pid).ok())
            .filter(|pid| *pid != 0))
    }
}

/// the method that asks the agent to drain for an update.
const DRAIN_FOR_UPDATE: &str = "drain_for_update";
