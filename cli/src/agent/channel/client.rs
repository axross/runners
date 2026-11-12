//! the public client of protocol version 2: it connects to this user's agent, says
//! hello, and then lets requests and the agent's pushes cross freely on one
//! long-lived connection; it ignores a message, a field and a topic it does not know
//! and calls only the methods `welcome` listed.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio::time::timeout;

use crate::instance::roots::Roots;

use super::super::snapshot::Snapshot;
use super::super::wire::{
    AgentMessage, ClientInfo, ClientMessage, ErrorBody, MAX_AGENT_MESSAGE_BYTES,
    MAX_CLIENT_MESSAGE_BYTES, TOPIC_GITHUB, TOPIC_STATE, UpdateEnding, VERSION,
};
use crate::github_status::GithubStatus;

/// a client gives up connecting after this long and reads it as no agent.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);

/// the agent has `welcome`d within this, or it is not answering.
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);

/// a subscribed connection that has been silent for this long has lost its agent: the
/// agent sends a heartbeat after 15 s without an event.
pub const SILENCE_LIMIT: Duration = Duration::from_secs(45);

/// why no connection was made.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
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
    /// on Windows, the server of the pipe could not be shown to run as this user, so
    /// nothing was sent to it.
    ServerNotThisUser,
    Failed(String),
}

impl fmt::Display for ConnectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoAgent => f.write_str("no agent is running"),
            Self::NotAnswering => f.write_str("the agent did not answer in time"),
            Self::Refused(body) => write!(f, "the agent refused the connection: {}", body.message),
            Self::NotProtocol => f.write_str("what answered is not a protocol version 2 agent"),
            Self::ServerNotThisUser => {
                f.write_str("what answers is not shown to run as this user, so nothing was sent")
            }
            Self::Failed(detail) => write!(f, "the agent could not be reached: {detail}"),
        }
    }
}

impl std::error::Error for ConnectError {}

/// why a call got no result.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum CallError {
    /// the agent answered with an error.
    Agent(ErrorBody),
    /// the agent's `welcome` did not list the method or topic, so it was not sent.
    NotServed(String),
    /// the connection ended before the answer.
    Closed,
    /// the agent answered, but not with what the method's result is, such as a
    /// snapshot that cannot be read.
    BadAnswer(String),
    /// the request is longer than the agent reads, so it was not written.
    TooLong,
}

impl fmt::Display for CallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Agent(body) => write!(f, "{}: {}", body.code, body.message),
            Self::NotServed(method) => write!(f, "this agent does not serve {method}"),
            Self::Closed => f.write_str("the connection to the agent ended"),
            Self::BadAnswer(what) => write!(f, "the agent's answer could not be read: {what}"),
            Self::TooLong => write!(
                f,
                "the request is longer than the {MAX_CLIENT_MESSAGE_BYTES} bytes the agent reads"
            ),
        }
    }
}

impl std::error::Error for CallError {}

/// what the agent sends unasked.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Push {
    /// the snapshot, whole; `seq` rises by one with each event of the connection.
    #[non_exhaustive]
    Event { seq: u64, snapshot: Box<Snapshot> },
    /// the `github` topic's value, whole; `seq` rises as an event's does.
    #[non_exhaustive]
    Github { seq: u64, status: Box<GithubStatus> },
    #[non_exhaustive]
    Heartbeat { seq: u64 },
    /// the agent is about to end, for this reason (an open set), with what an end for
    /// an update adds.
    #[non_exhaustive]
    Ending {
        reason: String,
        update: Option<UpdateEnding>,
    },
    /// the connection ended.
    Closed,
}

/// what the agent said about itself in `welcome`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Welcome {
    pub version: u32,
    pub agent_version: String,
    pub methods: Vec<String>,
    pub topics: Vec<String>,
}

type Answers = Arc<Mutex<BTreeMap<u64, oneshot::Sender<Result<serde_json::Value, CallError>>>>>;

/// a connection to the agent; requests may be made from several tasks at once, and
/// the agent answers each connection's requests in order.
pub struct Client {
    pub welcome: Welcome,
    /// whole request lines for the writing task, which writes each to the end even
    /// when the call that queued it is dropped.
    outgoing: mpsc::UnboundedSender<Vec<u8>>,
    next_id: AtomicU64,
    answers: Answers,
    /// set once the agent accepted a subscription, from when its silence means it is
    /// gone.
    subscribed: Arc<AtomicBool>,
    /// set, with the answers cleared, when the connection ends for any reason.
    closed: Arc<AtomicBool>,
    reading: JoinHandle<()>,
    writing: JoinHandle<()>,
}

impl Drop for Client {
    fn drop(&mut self) {
        self.reading.abort();
        self.writing.abort();
    }
}

/// takes a call's place in the answers away when the call ends, however it ends, so
/// a call dropped while it waits leaves nothing behind.
struct Waiting<'a> {
    answers: &'a Answers,
    id: u64,
}

impl Drop for Waiting<'_> {
    fn drop(&mut self) {
        lock(self.answers).remove(&self.id);
    }
}

impl Client {
    /// sends `method` with `params` and waits for its result. A call fails with
    /// [`CallError::Closed`] once the connection has ended, but it waits as long as the
    /// agent takes on a connection that stays open, so a caller that needs a bound
    /// wraps it in its own timeout. Dropping the call, as a timeout does, is safe: the
    /// request is queued whole and written to the end, its answer is discarded, and
    /// the client serves the next call.
    pub async fn call(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, CallError> {
        if !self.welcome.methods.iter().any(|served| served == method) {
            return Err(CallError::NotServed(method.to_string()));
        }
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let message = ClientMessage::Request {
            id,
            method: method.to_string(),
            params,
        };
        let mut line = serde_json::to_vec(&message).map_err(|_| CallError::Closed)?;
        line.push(b'\n');
        // refused before anything is written, so the connection stays as it was
        if line.len() > MAX_CLIENT_MESSAGE_BYTES {
            return Err(CallError::TooLong);
        }
        let (reply, answer) = oneshot::channel();
        {
            // checked under the lock the connection's end clears the answers with, so a
            // call is either refused here or answered or cleared there
            let mut answers = lock(&self.answers);
            if self.closed.load(Ordering::SeqCst) {
                return Err(CallError::Closed);
            }
            answers.insert(id, reply);
        }
        let _waiting = Waiting {
            answers: &self.answers,
            id,
        };
        if self.outgoing.send(line).is_err() {
            return Err(CallError::Closed);
        }
        match answer.await {
            Ok(answered) => answered,
            Err(_) => Err(CallError::Closed),
        }
    }

    /// the agent's snapshot now.
    pub async fn status(&self) -> Result<Snapshot, CallError> {
        let value = self.call("status", serde_json::Value::Null).await?;
        serde_json::from_value(value).map_err(|error| CallError::BadAnswer(error.to_string()))
    }

    /// subscribes to the `state` topic; the first push is its whole value.
    pub async fn subscribe(&self) -> Result<(), CallError> {
        if !self.welcome.topics.iter().any(|topic| topic == TOPIC_STATE) {
            return Err(CallError::NotServed(format!("the topic {TOPIC_STATE}")));
        }
        self.call("subscribe", serde_json::json!({ "topic": TOPIC_STATE }))
            .await?;
        self.subscribed.store(true, Ordering::SeqCst);
        Ok(())
    }

    /// subscribes to the `github` topic on this connection, which then holds no other
    /// subscription; the agent collects while it is renewed, so renew it every 30 s.
    pub async fn subscribe_github(&self) -> Result<(), CallError> {
        if !self
            .welcome
            .topics
            .iter()
            .any(|topic| topic == TOPIC_GITHUB)
        {
            return Err(CallError::NotServed(format!("the topic {TOPIC_GITHUB}")));
        }
        self.call("subscribe", serde_json::json!({ "topic": TOPIC_GITHUB }))
            .await?;
        self.subscribed.store(true, Ordering::SeqCst);
        Ok(())
    }

    /// renews this connection's `github` subscription; an agent that ended it for
    /// want of renewal answers `not_subscribed`, and the caller subscribes again.
    pub async fn renew_github(&self) -> Result<(), CallError> {
        self.call("renew", serde_json::json!({ "topic": TOPIC_GITHUB }))
            .await
            .map(|_| ())
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
) -> std::sync::MutexGuard<'_, BTreeMap<u64, oneshot::Sender<Result<serde_json::Value, CallError>>>>
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

/// whether the server's user, when it could be read, is the current user; an unread
/// user is not.
#[cfg_attr(not(windows), allow(dead_code))]
fn same_user(server: Option<&str>, current: &str) -> bool {
    server.is_some_and(|server| server.eq_ignore_ascii_case(current))
}

/// says hello on `stream` and waits for the agent's answer.
async fn handshake<S>(
    stream: S,
    client: &ClientInfo,
) -> Result<(Client, mpsc::UnboundedReceiver<Push>), ConnectError>
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
    if welcome.version != VERSION {
        return Err(ConnectError::NotProtocol);
    }
    let answers: Answers = Arc::default();
    let (pushes, receiver) = mpsc::unbounded_channel();
    let subscribed = Arc::new(AtomicBool::new(false));
    let closed = Arc::new(AtomicBool::new(false));
    let (outgoing, queued) = mpsc::unbounded_channel();
    let writing = tokio::spawn(write_lines(
        writer,
        queued,
        Arc::clone(&answers),
        Arc::clone(&closed),
    ));
    let reading = tokio::spawn(read_messages(
        reader,
        Arc::clone(&answers),
        pushes,
        Arc::clone(&subscribed),
        Arc::clone(&closed),
    ));
    let client = Client {
        welcome,
        outgoing,
        next_id: AtomicU64::new(1),
        answers,
        subscribed,
        closed,
        reading,
        writing,
    };
    Ok((client, receiver))
}

async fn read_messages<R: AsyncRead + Unpin>(
    mut reader: BufReader<R>,
    answers: Answers,
    pushes: mpsc::UnboundedSender<Push>,
    subscribed: Arc<AtomicBool>,
    closed: Arc<AtomicBool>,
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
            fail_unreadable_response(&answers, &bytes);
            continue;
        };
        match message {
            AgentMessage::Response { id, result, error } => {
                if let Some(reply) = lock(&answers).remove(&id) {
                    let _ = reply.send(match error {
                        Some(body) => Err(CallError::Agent(body)),
                        None => Ok(result.unwrap_or(serde_json::Value::Null)),
                    });
                }
            }
            AgentMessage::Event { topic, seq, value } => {
                if let Some(snapshot) = AgentMessage::snapshot(&topic, &value) {
                    let _ = pushes.send(Push::Event { seq, snapshot });
                } else if let Some(status) = AgentMessage::github(&topic, &value) {
                    let _ = pushes.send(Push::Github { seq, status });
                }
            }
            AgentMessage::Heartbeat { seq } => {
                let _ = pushes.send(Push::Heartbeat { seq });
            }
            AgentMessage::Ending { reason, update } => {
                let _ = pushes.send(Push::Ending { reason, update });
            }
            AgentMessage::Welcome { .. } | AgentMessage::Error { .. } => {}
        }
    }
    end_answers(&answers, &closed);
    let _ = pushes.send(Push::Closed);
}

/// ends the call that a response line answers when the line has a readable id but an
/// envelope that is not, so the call does not wait for the connection to close; the
/// text names no content of the line.
fn fail_unreadable_response(answers: &Answers, bytes: &[u8]) {
    #[derive(serde::Deserialize)]
    struct Envelope {
        #[serde(rename = "type")]
        kind: String,
        id: u64,
    }
    let Ok(Envelope { kind, id }) = serde_json::from_slice(bytes) else {
        return;
    };
    if kind == "response"
        && let Some(reply) = lock(answers).remove(&id)
    {
        let _ = reply.send(Err(CallError::BadAnswer(
            "the response could not be read".to_string(),
        )));
    }
}

/// marks the connection ended and fails every call that waits.
fn end_answers(answers: &Answers, closed: &AtomicBool) {
    let mut answers = lock(answers);
    closed.store(true, Ordering::SeqCst);
    answers.clear();
}

/// writes each queued line to the end, one after the other; a write that fails ends
/// the connection for every call.
async fn write_lines<W: AsyncWrite + Unpin>(
    mut writer: W,
    mut queued: mpsc::UnboundedReceiver<Vec<u8>>,
    answers: Answers,
    closed: Arc<AtomicBool>,
) {
    while let Some(line) = queued.recv().await {
        if writer.write_all(&line).await.is_err() || writer.flush().await.is_err() {
            break;
        }
    }
    end_answers(&answers, &closed);
    // the agent sees the end of what the client sends, and ends the connection
    let _ = writer.shutdown().await;
}

#[cfg(unix)]
pub mod platform {
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
pub mod platform {
    use std::ffi::OsStr;
    use std::io;
    use std::time::Duration;

    use interprocess::os::windows::named_pipe::pipe_mode;
    use interprocess::os::windows::named_pipe::tokio::DuplexPipeStream;

    use crate::instance::roots::Roots;
    use crate::instance::windows::{current_user_sid, process_user_sid};

    use super::{ConnectError, same_user};

    pub type Stream = DuplexPipeStream<pipe_mode::Bytes>;

    /// `ERROR_PIPE_BUSY`: every instance of the pipe is serving a client, and one is
    /// about to be free.
    const ERROR_PIPE_BUSY: i32 = 231;

    /// lets the connection through only when the process serving it runs as `sid`; a
    /// pipe of that name could be held by another account's process, and a request
    /// such as `token_set` must not reach it. Any doubt is a refusal, and the stream
    /// is dropped before a byte is sent.
    fn verified(stream: Stream, sid: &str) -> Result<Stream, ConnectError> {
        let server = stream.server_process_id().and_then(process_user_sid).ok();
        if same_user(server.as_deref(), sid) {
            Ok(stream)
        } else {
            Err(ConnectError::ServerNotThisUser)
        }
    }

    pub async fn connect(roots: &Roots) -> Result<Stream, ConnectError> {
        let sid = current_user_sid().map_err(|error| ConnectError::Failed(error.to_string()))?;
        let name = roots.control_pipe_name(&sid);
        loop {
            match Stream::connect_by_path(OsStr::new(&name)).await {
                Ok(stream) => return verified(stream, &sid),
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
) -> Result<(Client, mpsc::UnboundedReceiver<Push>), ConnectError> {
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

    #[test]
    fn a_server_is_this_users_only_when_its_user_was_read_and_is_the_same() {
        let me = "S-1-5-21-1-2-3-1001";
        assert!(same_user(Some(me), me));
        assert!(same_user(Some("s-1-5-21-1-2-3-1001"), me));
        assert!(!same_user(Some("S-1-5-21-1-2-3-1002"), me));
        assert!(!same_user(Some("S-1-5-21-1-2-3-100"), me));
        assert!(
            !same_user(None, me),
            "a user that cannot be read is refused"
        );
    }

    #[test]
    fn the_refusal_to_trust_a_server_names_no_command_and_no_token() {
        let text = ConnectError::ServerNotThisUser.to_string();
        assert!(
            !text.contains("docker") && !text.contains("token"),
            "{text}"
        );
    }

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

    /// an agent that answers each request line with an empty result, and gives back
    /// the lines it read once the client is dropped.
    fn answering_agent(far: tokio::io::DuplexStream) -> tokio::task::JoinHandle<Vec<String>> {
        tokio::spawn(async move {
            let (read, mut write) = tokio::io::split(far);
            let mut reader = BufReader::new(read);
            let mut hello = String::new();
            reader.read_line(&mut hello).await.unwrap();
            write
                .write_all(b"{\"type\":\"welcome\",\"version\":2,\"methods\":[\"status\",\"token_set\"],\"topics\":[]}\n")
                .await
                .unwrap();
            let mut seen = Vec::new();
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).await.unwrap() == 0 {
                    return seen;
                }
                let request: serde_json::Value = serde_json::from_str(&line).unwrap();
                let id = request["id"].as_u64().unwrap();
                seen.push(line);
                write
                    .write_all(
                        format!("{{\"type\":\"response\",\"id\":{id},\"result\":{{}}}}\n")
                            .as_bytes(),
                    )
                    .await
                    .unwrap();
            }
        })
    }

    /// `token_set` params that make the request line exactly `length` bytes with its
    /// line break.
    fn params_for_a_line_of(length: usize) -> serde_json::Value {
        let line_with = |pad: usize| {
            let message = ClientMessage::Request {
                id: 1,
                method: "token_set".to_string(),
                params: serde_json::json!({ "token": "x".repeat(pad) }),
            };
            serde_json::to_vec(&message).unwrap().len() + 1
        };
        let pad = length - line_with(0);
        serde_json::json!({ "token": "x".repeat(pad) })
    }

    #[tokio::test]
    async fn a_request_longer_than_the_agent_reads_is_refused_before_it_is_written() {
        let (near, far) = duplex(64 * 1024);
        let agent = answering_agent(far);
        let (client, _pushes) = handshake(near, &ClientInfo::default()).await.unwrap();
        let too_long = params_for_a_line_of(MAX_CLIENT_MESSAGE_BYTES + 1);
        assert_eq!(
            client.call("token_set", too_long).await,
            Err(CallError::TooLong)
        );
        // the connection is as it was: a request after it is answered, and one of the
        // longest the agent reads is sent
        assert_eq!(
            client.call("status", serde_json::Value::Null).await,
            Ok(serde_json::json!({}))
        );
        let longest = params_for_a_line_of(MAX_CLIENT_MESSAGE_BYTES);
        assert_eq!(
            client.call("token_set", longest).await,
            Ok(serde_json::json!({}))
        );
        drop(client);
        let seen = agent.await.unwrap();
        assert_eq!(
            seen.len(),
            2,
            "only the status and the longest were written"
        );
        assert_eq!(seen[1].len(), MAX_CLIENT_MESSAGE_BYTES);
    }

    #[tokio::test]
    async fn a_response_that_cannot_be_read_ends_its_call_and_the_next_call_is_answered() {
        let (near, far) = duplex(4096);
        tokio::spawn(async move {
            let (read, mut write) = tokio::io::split(far);
            let mut reader = BufReader::new(read);
            let mut line = String::new();
            reader.read_line(&mut line).await.unwrap();
            write
                .write_all(b"{\"type\":\"welcome\",\"version\":2,\"methods\":[\"status\"],\"topics\":[]}\n")
                .await
                .unwrap();
            // an error body without its `code`, then an answer that reads
            for answer in [
                &b"{\"type\":\"response\",\"id\":1,\"error\":{\"message\":\"no code\"}}\n"[..],
                &b"{\"type\":\"response\",\"id\":2,\"result\":{}}\n"[..],
            ] {
                line.clear();
                reader.read_line(&mut line).await.unwrap();
                write.write_all(answer).await.unwrap();
            }
            // the connection stays open
            std::future::pending::<()>().await;
        });
        let (client, _pushes) = handshake(near, &ClientInfo::default()).await.unwrap();
        let first = timeout(
            Duration::from_secs(5),
            client.call("status", serde_json::Value::Null),
        )
        .await
        .expect("the call waited for the connection to close");
        assert!(matches!(first, Err(CallError::BadAnswer(_))), "{first:?}");
        assert_eq!(
            client.call("status", serde_json::Value::Null).await,
            Ok(serde_json::json!({}))
        );
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
    async fn a_call_after_the_connection_ended_fails_instead_of_waiting_forever() {
        let (near, far) = duplex(4096);
        tokio::spawn(async move {
            let (read, mut write) = tokio::io::split(far);
            let mut reader = BufReader::new(read);
            let mut hello = String::new();
            reader.read_line(&mut hello).await.unwrap();
            write
                .write_all(b"{\"type\":\"welcome\",\"version\":2,\"methods\":[\"status\"],\"topics\":[]}\n")
                .await
                .unwrap();
            // the agent's end of the connection drops here
        });
        let (client, mut pushes) = handshake(near, &ClientInfo::default()).await.unwrap();
        assert_eq!(pushes.recv().await, Some(Push::Closed));
        let answer = timeout(
            Duration::from_secs(5),
            client.call("status", serde_json::Value::Null),
        )
        .await
        .expect("the call waited for an answer that cannot come");
        assert_eq!(answer, Err(CallError::Closed));
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

    #[tokio::test]
    async fn a_welcome_for_another_version_is_not_protocol_version_2() {
        let (near, far) = duplex(4096);
        tokio::spawn(async move {
            let (read, mut write) = tokio::io::split(far);
            let mut reader = BufReader::new(read);
            let mut hello = String::new();
            reader.read_line(&mut hello).await.unwrap();
            write
                .write_all(b"{\"type\":\"welcome\",\"version\":3,\"methods\":[\"status\"],\"topics\":[\"state\"]}\n")
                .await
                .unwrap();
        });
        let outcome = handshake(near, &ClientInfo::default()).await;
        assert!(matches!(outcome, Err(ConnectError::NotProtocol)));
    }

    #[tokio::test]
    async fn a_topic_the_welcome_did_not_list_is_not_subscribed_to() {
        let (near, far) = duplex(4096);
        let agent = tokio::spawn(async move {
            let (read, mut write) = tokio::io::split(far);
            let mut reader = BufReader::new(read);
            let mut hello = String::new();
            reader.read_line(&mut hello).await.unwrap();
            write
                .write_all(b"{\"type\":\"welcome\",\"version\":2,\"methods\":[\"subscribe\"],\"topics\":[]}\n")
                .await
                .unwrap();
            let mut next = String::new();
            reader.read_line(&mut next).await.unwrap();
            next
        });
        let (client, _pushes) = handshake(near, &ClientInfo::default()).await.unwrap();
        assert_eq!(
            client.subscribe().await,
            Err(CallError::NotServed("the topic state".to_string()))
        );
        drop(client);
        assert_eq!(agent.await.unwrap(), "", "nothing was sent");
    }

    #[tokio::test]
    async fn a_call_dropped_while_it_writes_is_still_written_whole_and_leaves_no_answer_behind() {
        // a buffer far smaller than the request, so the write is not done when the
        // call is dropped
        let (near, far) = duplex(64);
        let (read, mut write) = tokio::io::split(far);
        let mut reader = BufReader::new(read);
        let mut hello = String::new();
        let info = ClientInfo::default();
        let handshaking = handshake(near, &info);
        let agent_hello = async {
            reader.read_line(&mut hello).await.unwrap();
            write
                .write_all(b"{\"type\":\"welcome\",\"version\":2,\"methods\":[\"status\"],\"topics\":[]}\n")
                .await
                .unwrap();
        };
        let (client, ()) = tokio::join!(async { handshaking.await.unwrap().0 }, agent_hello);
        let padding = "x".repeat(5000);
        let dropped = timeout(
            Duration::from_millis(50),
            client.call("status", serde_json::json!({ "pad": padding })),
        )
        .await;
        assert!(dropped.is_err(), "the call finished with nobody reading");
        assert!(
            lock(&client.answers).is_empty(),
            "the dropped call left its place"
        );
        // the agent reads: the dropped call's line is whole, and the next call's follows it
        let next =
            tokio::spawn(async move { client.call("status", serde_json::Value::Null).await });
        let mut first = String::new();
        reader.read_line(&mut first).await.unwrap();
        let first: serde_json::Value = serde_json::from_str(first.trim_end()).unwrap();
        assert_eq!(first["method"], "status");
        assert_eq!(first["params"]["pad"].as_str().unwrap().len(), 5000);
        let mut second = String::new();
        reader.read_line(&mut second).await.unwrap();
        let second: serde_json::Value = serde_json::from_str(second.trim_end()).unwrap();
        assert_eq!(
            second["id"].as_u64().unwrap(),
            first["id"].as_u64().unwrap() + 1
        );
        // the answer to the dropped call is discarded; the next call gets its own
        write
            .write_all(
                format!(
                    "{{\"type\":\"response\",\"id\":{},\"result\":{{}}}}\n{{\"type\":\"response\",\"id\":{},\"result\":{{\"ok\":true}}}}\n",
                    first["id"], second["id"]
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        let answer = timeout(Duration::from_secs(5), next)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(answer, Ok(serde_json::json!({"ok": true})));
    }

    #[tokio::test]
    async fn a_status_that_is_not_a_snapshot_is_a_bad_answer_and_not_a_closed_connection() {
        let (near, far) = duplex(4096);
        tokio::spawn(async move {
            let (read, mut write) = tokio::io::split(far);
            let mut reader = BufReader::new(read);
            let mut line = String::new();
            reader.read_line(&mut line).await.unwrap();
            write
                .write_all(b"{\"type\":\"welcome\",\"version\":2,\"methods\":[\"status\"],\"topics\":[]}\n")
                .await
                .unwrap();
            line.clear();
            reader.read_line(&mut line).await.unwrap();
            write
                .write_all(b"{\"type\":\"response\",\"id\":1,\"result\":{\"pools\":5}}\n")
                .await
                .unwrap();
            // the connection stays open
            tokio::time::sleep(Duration::from_secs(5)).await;
        });
        let (client, _pushes) = handshake(near, &ClientInfo::default()).await.unwrap();
        assert!(matches!(
            client.status().await,
            Err(CallError::BadAnswer(_))
        ));
    }
}
