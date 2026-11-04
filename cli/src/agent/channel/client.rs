//! the public client of protocol version 2: it connects to this user's agent, says
//! hello, and then lets requests and the agent's pushes cross freely on one
//! long-lived connection; it ignores a message, a field and a topic it does not know
//! and calls only the methods `welcome` listed.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{
    AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader, WriteHalf,
};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio::time::timeout;

use crate::instance::roots::Roots;

use super::super::snapshot::Snapshot;
use super::super::wire::{
    AgentMessage, ClientInfo, ClientMessage, ErrorBody, MAX_AGENT_MESSAGE_BYTES, TOPIC_STATE,
    VERSION,
};

/// a client gives up connecting after this long and reads it as no agent.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);

/// the agent has `welcome`d within this, or it is not answering.
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);

/// a subscribed connection that has been silent for this long has lost its agent: the
/// agent sends a heartbeat after 15 s without an event.
pub const SILENCE_LIMIT: Duration = Duration::from_secs(45);

/// why no connection was made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectError {
    /// nothing answers at the channel: it does not exist or refuses.
    NoAgent,
    /// the channel exists but the agent did not welcome the client in time, which
    /// reads as an agent that hangs, or a process of another program that holds the
    /// name.
    NotAnswering,
    /// the agent answered with an error: `busy` at its connection limit, or
    /// `unsupported_protocol` with the versions it serves.
    Refused(ErrorBody),
    /// the agent answered with something that is not protocol version 2.
    NotProtocol,
    Failed(String),
}

impl fmt::Display for ConnectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoAgent => f.write_str("no agent is running"),
            Self::NotAnswering => f.write_str("the agent did not answer in time"),
            Self::Refused(body) => write!(f, "the agent refused the connection: {}", body.message),
            Self::NotProtocol => f.write_str("what answered is not a protocol version 2 agent"),
            Self::Failed(detail) => write!(f, "the agent could not be reached: {detail}"),
        }
    }
}

impl std::error::Error for ConnectError {}

/// why a call got no result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallError {
    /// the agent answered with an error.
    Agent(ErrorBody),
    /// the agent's `welcome` did not list the method, so it was not sent.
    NotServed(String),
    /// the connection ended before the answer.
    Closed,
}

impl fmt::Display for CallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Agent(body) => write!(f, "{}: {}", body.code, body.message),
            Self::NotServed(method) => write!(f, "this agent does not serve {method}"),
            Self::Closed => f.write_str("the connection to the agent ended"),
        }
    }
}

impl std::error::Error for CallError {}

/// what the agent sends unasked.
#[derive(Debug, Clone, PartialEq)]
pub enum Push {
    /// the snapshot, whole; `seq` rises by one with each event of the connection.
    Event {
        seq: u64,
        snapshot: Box<Snapshot>,
    },
    Heartbeat {
        seq: u64,
    },
    /// the agent is about to end, for this reason (an open set).
    Ending {
        reason: String,
    },
    /// the connection ended.
    Closed,
}

/// what the agent said about itself in `welcome`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Welcome {
    pub version: u32,
    pub agent_version: String,
    pub methods: Vec<String>,
    pub topics: Vec<String>,
}

type Answers = Arc<Mutex<BTreeMap<u64, oneshot::Sender<Result<serde_json::Value, ErrorBody>>>>>;

/// a connection to the agent; requests may be made from several tasks at once, and
/// the agent answers each connection's requests in order.
pub struct Client<W> {
    pub welcome: Welcome,
    writer: tokio::sync::Mutex<W>,
    next_id: AtomicU64,
    answers: Answers,
    /// set once the agent accepted a subscription, from when its silence means it is
    /// gone.
    subscribed: Arc<AtomicBool>,
    reading: JoinHandle<()>,
}

impl<W> Drop for Client<W> {
    fn drop(&mut self) {
        self.reading.abort();
    }
}

impl<W: AsyncWrite + Unpin> Client<W> {
    /// sends `method` with `params` and waits for its result.
    pub async fn call(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, CallError> {
        if !self.welcome.methods.iter().any(|served| served == method) {
            return Err(CallError::NotServed(method.to_string()));
        }
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (reply, answer) = oneshot::channel();
        lock(&self.answers).insert(id, reply);
        let message = ClientMessage::Request {
            id,
            method: method.to_string(),
            params,
        };
        if self.write(&message).await.is_err() {
            lock(&self.answers).remove(&id);
            return Err(CallError::Closed);
        }
        match answer.await {
            Ok(Ok(result)) => Ok(result),
            Ok(Err(body)) => Err(CallError::Agent(body)),
            Err(_) => Err(CallError::Closed),
        }
    }

    async fn write(&self, message: &ClientMessage) -> std::io::Result<()> {
        let mut line = serde_json::to_vec(message).map_err(std::io::Error::other)?;
        line.push(b'\n');
        let mut writer = self.writer.lock().await;
        writer.write_all(&line).await?;
        writer.flush().await
    }

    /// the agent's snapshot now.
    pub async fn status(&self) -> Result<Snapshot, CallError> {
        let value = self.call("status", serde_json::Value::Null).await?;
        serde_json::from_value(value).map_err(|_| CallError::Closed)
    }

    /// subscribes to the `state` topic; the first push is its whole value.
    pub async fn subscribe(&self) -> Result<(), CallError> {
        self.call("subscribe", serde_json::json!({ "topic": TOPIC_STATE }))
            .await?;
        self.subscribed.store(true, Ordering::SeqCst);
        Ok(())
    }

    /// stores the repository's token; the token is sent once, in this call.
    pub async fn token_set(&self, repository: &str, token: &str) -> Result<(), CallError> {
        self.call(
            "token_set",
            serde_json::json!({ "repository": repository, "token": token }),
        )
        .await
        .map(|_| ())
    }

    pub async fn token_remove(&self, repository: &str) -> Result<(), CallError> {
        self.call(
            "token_remove",
            serde_json::json!({ "repository": repository }),
        )
        .await
        .map(|_| ())
    }
}

fn lock(
    answers: &Answers,
) -> std::sync::MutexGuard<'_, BTreeMap<u64, oneshot::Sender<Result<serde_json::Value, ErrorBody>>>>
{
    answers
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// what a stream of lines from the agent gives.
async fn read_line<R: AsyncRead + Unpin>(reader: &mut BufReader<R>) -> Option<Vec<u8>> {
    let mut line = Vec::new();
    let mut limited = reader.take(MAX_AGENT_MESSAGE_BYTES as u64);
    match limited.read_until(b'\n', &mut line).await {
        Ok(count) if count > 0 && line.last() == Some(&b'\n') => Some(line),
        _ => None,
    }
}

/// says hello on `stream` and waits for the agent's answer.
async fn handshake<S>(
    stream: S,
    client: &ClientInfo,
) -> Result<(Client<WriteHalf<S>>, mpsc::UnboundedReceiver<Push>), ConnectError>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (read, mut writer) = tokio::io::split(stream);
    let hello = ClientMessage::Hello {
        versions: vec![VERSION],
        client: client.clone(),
    };
    let mut line =
        serde_json::to_vec(&hello).map_err(|error| ConnectError::Failed(error.to_string()))?;
    line.push(b'\n');
    let mut reader = BufReader::new(read);
    let answered = timeout(HANDSHAKE_TIMEOUT, async {
        writer.write_all(&line).await?;
        writer.flush().await?;
        Ok::<_, std::io::Error>(read_line(&mut reader).await)
    })
    .await;
    let line = match answered {
        Err(_) => return Err(ConnectError::NotAnswering),
        Ok(Err(error)) => return Err(ConnectError::Failed(error.to_string())),
        Ok(Ok(None)) => return Err(ConnectError::NotProtocol),
        Ok(Ok(Some(line))) => line,
    };
    let welcome = match serde_json::from_slice::<AgentMessage>(&line) {
        Ok(AgentMessage::Welcome {
            version,
            agent,
            methods,
            topics,
        }) => Welcome {
            version,
            agent_version: agent.version,
            methods,
            topics,
        },
        Ok(AgentMessage::Error { error }) => return Err(ConnectError::Refused(error)),
        _ => return Err(ConnectError::NotProtocol),
    };
    let answers: Answers = Arc::default();
    let (pushes, receiver) = mpsc::unbounded_channel();
    let subscribed = Arc::new(AtomicBool::new(false));
    let reading = tokio::spawn(read_messages(
        reader,
        Arc::clone(&answers),
        pushes,
        Arc::clone(&subscribed),
    ));
    let client = Client {
        welcome,
        writer: tokio::sync::Mutex::new(writer),
        next_id: AtomicU64::new(1),
        answers,
        subscribed,
        reading,
    };
    Ok((client, receiver))
}

async fn read_messages<R: AsyncRead + Unpin>(
    mut reader: BufReader<R>,
    answers: Answers,
    pushes: mpsc::UnboundedSender<Push>,
    subscribed: Arc<AtomicBool>,
) {
    let mut line = Vec::new();
    loop {
        // the bytes of a line read so far stay in `line` when the wait is cut short
        let room = (MAX_AGENT_MESSAGE_BYTES - line.len()) as u64;
        let read = timeout(
            SILENCE_LIMIT,
            (&mut reader).take(room).read_until(b'\n', &mut line),
        )
        .await;
        match read {
            Err(_) if subscribed.load(Ordering::SeqCst) => break,
            Err(_) => continue,
            Ok(Ok(0) | Err(_)) => break,
            Ok(Ok(_)) => {}
        }
        if line.last() != Some(&b'\n') {
            if line.len() >= MAX_AGENT_MESSAGE_BYTES {
                break;
            }
            continue;
        }
        let bytes = std::mem::take(&mut line);
        let Ok(message) = serde_json::from_slice::<AgentMessage>(&bytes) else {
            continue;
        };
        match message {
            AgentMessage::Response { id, result, error } => {
                if let Some(reply) = lock(&answers).remove(&id) {
                    let _ = reply.send(match error {
                        Some(body) => Err(body),
                        None => Ok(result.unwrap_or(serde_json::Value::Null)),
                    });
                }
            }
            AgentMessage::Event { topic, seq, value } if topic == TOPIC_STATE => {
                let _ = pushes.send(Push::Event {
                    seq,
                    snapshot: value,
                });
            }
            AgentMessage::Heartbeat { seq } => {
                let _ = pushes.send(Push::Heartbeat { seq });
            }
            AgentMessage::Ending { reason } => {
                let _ = pushes.send(Push::Ending { reason });
            }
            AgentMessage::Welcome { .. }
            | AgentMessage::Error { .. }
            | AgentMessage::Event { .. } => {}
        }
    }
    lock(&answers).clear();
    let _ = pushes.send(Push::Closed);
}

#[cfg(unix)]
mod platform {
    use tokio::net::UnixStream;

    use crate::instance::roots::Roots;
    use crate::private;

    use super::ConnectError;

    pub type Stream = UnixStream;

    pub async fn connect(roots: &Roots) -> Result<Stream, ConnectError> {
        let failed = |error: &dyn std::fmt::Display| ConnectError::Failed(error.to_string());
        let uid = private::current_uid().map_err(|error| failed(&error))?;
        match private::check_private_directory(roots.channel_directory(), uid) {
            Ok(()) => {}
            Err(private::PrivateError::Missing(_)) => return Err(ConnectError::NoAgent),
            Err(error) => return Err(failed(&error)),
        }
        UnixStream::connect(roots.socket_path())
            .await
            .map_err(|error| match error.kind() {
                std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused => {
                    ConnectError::NoAgent
                }
                _ => failed(&error),
            })
    }
}

#[cfg(windows)]
mod platform {
    use std::ffi::OsStr;
    use std::io;
    use std::time::Duration;

    use interprocess::os::windows::named_pipe::pipe_mode;
    use interprocess::os::windows::named_pipe::tokio::DuplexPipeStream;

    use crate::instance::roots::Roots;
    use crate::instance::windows::current_user_sid;

    use super::ConnectError;

    pub type Stream = DuplexPipeStream<pipe_mode::Bytes>;

    /// `ERROR_PIPE_BUSY`: every instance of the pipe is serving a client, and one is
    /// about to be free.
    const ERROR_PIPE_BUSY: i32 = 231;

    pub async fn connect(roots: &Roots) -> Result<Stream, ConnectError> {
        let sid = current_user_sid().map_err(|error| ConnectError::Failed(error.to_string()))?;
        let name = roots.control_pipe_name(&sid);
        loop {
            match Stream::connect_by_path(OsStr::new(&name)).await {
                Ok(stream) => return Ok(stream),
                Err(error) if error.raw_os_error() == Some(ERROR_PIPE_BUSY) => {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    return Err(ConnectError::NoAgent);
                }
                Err(error) => return Err(ConnectError::Failed(error.to_string())),
            }
        }
    }
}

/// connects to this user's agent and speaks protocol version 2 with it; the second
/// value carries what the agent sends unasked.
pub async fn connect(
    roots: &Roots,
    client: &ClientInfo,
) -> Result<
    (
        Client<WriteHalf<platform::Stream>>,
        mpsc::UnboundedReceiver<Push>,
    ),
    ConnectError,
> {
    let stream = timeout(CONNECT_TIMEOUT, platform::connect(roots))
        .await
        .map_err(|_| ConnectError::NoAgent)??;
    handshake(stream, client).await
}

#[cfg(test)]
mod tests {
    use tokio::io::duplex;

    use super::*;
    use crate::agent::snapshot::{RunnerState, WarningKind};

    #[tokio::test]
    async fn a_line_that_is_not_protocol_version_2_is_not_a_welcome() {
        let (near, far) = duplex(1024);
        let agent = tokio::spawn(async move {
            let (read, mut write) = tokio::io::split(far);
            let mut reader = BufReader::new(read);
            let mut hello = String::new();
            reader.read_line(&mut hello).await.unwrap();
            write.write_all(b"ok\nstate running\nend\n").await.unwrap();
        });
        let outcome = handshake(near, &ClientInfo::default()).await;
        assert!(matches!(outcome, Err(ConnectError::NotProtocol)));
        agent.await.unwrap();
    }

    #[tokio::test]
    async fn a_method_the_welcome_did_not_list_is_not_sent() {
        let (near, far) = duplex(4096);
        let agent = tokio::spawn(async move {
            let (read, mut write) = tokio::io::split(far);
            let mut reader = BufReader::new(read);
            let mut hello = String::new();
            reader.read_line(&mut hello).await.unwrap();
            write
                .write_all(b"{\"type\":\"welcome\",\"version\":2,\"methods\":[\"status\"],\"topics\":[\"state\"],\"later\":1}\n")
                .await
                .unwrap();
            let mut next = String::new();
            reader.read_line(&mut next).await.unwrap();
            next
        });
        let (client, _pushes) = handshake(near, &ClientInfo::default()).await.unwrap();
        assert_eq!(
            client.call("quit", serde_json::Value::Null).await,
            Err(CallError::NotServed("quit".to_string()))
        );
        drop(client);
        assert_eq!(agent.await.unwrap(), "", "nothing was sent");
    }

    #[tokio::test]
    async fn unknown_messages_are_ignored_and_a_closed_connection_is_a_push() {
        let (near, far) = duplex(4096);
        tokio::spawn(async move {
            let (read, mut write) = tokio::io::split(far);
            let mut reader = BufReader::new(read);
            let mut hello = String::new();
            reader.read_line(&mut hello).await.unwrap();
            write
                .write_all(
                    b"{\"type\":\"welcome\",\"version\":2,\"methods\":[],\"topics\":[]}\n\
                      {\"type\":\"something_later\",\"x\":1}\n\
                      {\"type\":\"heartbeat\",\"seq\":4,\"extra\":true}\n",
                )
                .await
                .unwrap();
        });
        let (_client, mut pushes) = handshake(near, &ClientInfo::default()).await.unwrap();
        assert_eq!(pushes.recv().await, Some(Push::Heartbeat { seq: 4 }));
        assert_eq!(pushes.recv().await, Some(Push::Closed));
    }

    #[tokio::test]
    async fn a_snapshot_with_runner_states_and_warnings_it_does_not_know_still_arrives() {
        let (near, far) = duplex(8192);
        tokio::spawn(async move {
            let (read, mut write) = tokio::io::split(far);
            let mut reader = BufReader::new(read);
            let mut hello = String::new();
            reader.read_line(&mut hello).await.unwrap();
            let event = r#"{"type":"event","topic":"state","seq":1,"value":{
                "pools":[{"name":"p","runners":[
                    {"slot":1,"state":"booting_vm","workload":"vm-1","reason":"the VM is starting","since":1,"vm_ip":"x"},
                    {"slot":2,"state":"running","workload":"c-2","since":2}]}],
                "leftovers":[{"kind":"vm","name":"vm-9"}],
                "images":[
                    {"name":"runner","warnings":[{"code":"image_old","days":50},{"code":"signed_by_nobody","days":1}]},
                    {"name":"golden","lines":[],"warnings":[{"code":"golden_old"}]}]}}"#
                .replace('\n', "");
            write
                .write_all(
                    format!(
                        "{{\"type\":\"welcome\",\"version\":2,\"methods\":[],\"topics\":[\"state\"]}}\n{event}\n"
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
        });
        let (_client, mut pushes) = handshake(near, &ClientInfo::default()).await.unwrap();
        let Some(Push::Event { snapshot, .. }) = pushes.recv().await else {
            panic!("the event did not arrive");
        };
        let runners = &snapshot.pools[0].runners;
        assert_eq!(
            runners[0].kind(),
            RunnerState::Unknown("booting_vm".to_string())
        );
        assert_eq!(runners[0].unknown_state_text(), Some("the VM is starting"));
        assert_eq!(runners[1].kind(), RunnerState::Running);
        assert_eq!(runners[1].unknown_state_text(), None);
        let known: Vec<_> = snapshot.images[0]
            .known_warnings()
            .map(|(kind, _)| kind)
            .collect();
        assert_eq!(
            known,
            [WarningKind::ImageOld],
            "the unknown warning is ignored"
        );
        assert_eq!(snapshot.images[1].known_warnings().count(), 0);
        assert_eq!(snapshot.leftovers[0].kind, "vm");
    }
}
