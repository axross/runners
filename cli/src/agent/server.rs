//! one connection of protocol version 2, as the agent serves it: the handshake, the
//! requests answered in order, the subscription's coalesced events and heartbeats,
//! and the `ending` message; every limit is a timer on the agent's own runtime, so a
//! silent or stalled client holds no task of its own past its timers. On Windows the
//! pipe crate flushes on a blocking thread, which only the explicit flushes of the
//! last messages start; see `docs/operations/cli.md`.

use std::future::pending;
use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll};
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::JoinHandle;
use tokio::time::{Instant, sleep_until, timeout};

use super::api::{ApiError, ApiResult, Call, Method, Parsed, Topic, parse};
use super::snapshot::Snapshot;
use super::wire::{
    AgentVersion, ClientMessage, EndingNotice, ErrorBody, ErrorCode, EventValue,
    MAX_AGENT_MESSAGE_BYTES, MAX_CLIENT_MESSAGE_BYTES, METHODS, ServerMessage, TOPICS, VERSION,
};
use crate::github_status::GithubStatus;

/// the time and count limits of the channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// a connection that has not completed its handshake by then is closed.
    pub handshake: Duration,
    /// a connection with no subscription and no request pending that has sent
    /// nothing for this long is closed.
    pub idle: Duration,
    /// a message that cannot be written in this long closes its connection.
    pub write: Duration,
    /// a subscribed connection gets a heartbeat after this long without an event.
    pub heartbeat: Duration,
    /// a subscribed connection gets at most one event per this.
    pub coalesce: Duration,
    /// connections open at once.
    pub connections: usize,
    /// connections that hold a subscription.
    pub subscriptions: usize,
    /// a `github` subscription not renewed for this long ends.
    pub renewal: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            handshake: Duration::from_secs(5),
            idle: Duration::from_secs(60),
            write: Duration::from_secs(5),
            heartbeat: Duration::from_secs(15),
            coalesce: Duration::from_millis(250),
            connections: 16,
            subscriptions: 4,
            renewal: Duration::from_secs(90),
        }
    }
}

/// the connections open now, and those that hold a subscription.
#[derive(Debug, Default)]
pub struct Counts {
    open: AtomicUsize,
    refused: AtomicUsize,
    subscribed: AtomicUsize,
}

/// how a new connection is treated.
#[derive(Debug)]
pub enum Admission {
    /// served, until the guard drops.
    Open(Slot),
    /// beyond the limit: it gets `busy` when its first line arrives.
    Busy(Slot),
}

#[derive(Debug, Clone, Copy)]
enum Kind {
    Open,
    Refused,
}

/// a place among the open connections, or among those waiting for `busy`.
#[derive(Debug)]
pub struct Slot {
    counts: Arc<Counts>,
    kind: Kind,
}

impl Drop for Slot {
    fn drop(&mut self) {
        match self.kind {
            Kind::Open => &self.counts.open,
            Kind::Refused => &self.counts.refused,
        }
        .fetch_sub(1, Ordering::SeqCst);
    }
}

/// a place among the subscriptions.
struct Subscribed(Arc<Counts>);

impl Drop for Subscribed {
    fn drop(&mut self) {
        self.0.subscribed.fetch_sub(1, Ordering::SeqCst);
    }
}

impl Counts {
    /// places a new connection, or `None` when even the ones waiting for `busy` are
    /// at the limit, and the connection is closed unanswered.
    #[must_use]
    pub fn admit(self: &Arc<Self>, limits: &Limits) -> Option<Admission> {
        if self.open.fetch_add(1, Ordering::SeqCst) < limits.connections {
            return Some(Admission::Open(Slot {
                counts: Arc::clone(self),
                kind: Kind::Open,
            }));
        }
        self.open.fetch_sub(1, Ordering::SeqCst);
        if self.refused.fetch_add(1, Ordering::SeqCst) < limits.connections {
            return Some(Admission::Busy(Slot {
                counts: Arc::clone(self),
                kind: Kind::Refused,
            }));
        }
        self.refused.fetch_sub(1, Ordering::SeqCst);
        None
    }

    /// the number of connections open now.
    #[must_use]
    pub fn open(&self) -> usize {
        self.open.load(Ordering::SeqCst)
    }

    /// the number of connections open or waiting to be told `busy`.
    #[must_use]
    pub fn all(&self) -> usize {
        self.open() + self.refused.load(Ordering::SeqCst)
    }

    fn subscribe(self: &Arc<Self>, limits: &Limits) -> Option<Subscribed> {
        if self.subscribed.fetch_add(1, Ordering::SeqCst) < limits.subscriptions {
            Some(Subscribed(Arc::clone(self)))
        } else {
            self.subscribed.fetch_sub(1, Ordering::SeqCst);
            None
        }
    }
}

/// the `github` subscriptions alive now; GitHub status is collected while there is
/// one.
#[derive(Debug)]
pub struct Watchers {
    count: AtomicUsize,
    active: watch::Sender<bool>,
}

impl Watchers {
    /// no watcher yet, and the receiver that says whether there is one.
    #[must_use]
    pub fn new() -> (Arc<Self>, watch::Receiver<bool>) {
        let (active, receiver) = watch::channel(false);
        (
            Arc::new(Self {
                count: AtomicUsize::new(0),
                active,
            }),
            receiver,
        )
    }

    fn join(self: &Arc<Self>) -> Watching {
        if self.count.fetch_add(1, Ordering::SeqCst) == 0 {
            self.active.send_replace(true);
        }
        Watching(Arc::clone(self))
    }
}

/// a live `github` subscription, counted until it drops.
struct Watching(Arc<Watchers>);

impl Drop for Watching {
    fn drop(&mut self) {
        if self.0.count.fetch_sub(1, Ordering::SeqCst) == 1 {
            self.0.active.send_replace(false);
        }
    }
}

/// what every connection shares with the agent.
pub struct Shared {
    pub calls: mpsc::UnboundedSender<Call>,
    pub snapshots: watch::Receiver<Arc<Snapshot>>,
    /// the `github` topic's value.
    pub github: watch::Receiver<Arc<GithubStatus>>,
    pub watchers: Arc<Watchers>,
    /// set when the agent is about to end.
    pub ending: watch::Receiver<Option<EndingNotice>>,
    pub limits: Limits,
    pub counts: Arc<Counts>,
    pub agent_version: String,
}

enum Read {
    Line(Vec<u8>),
    /// the line did not end within the client's limit.
    TooLong,
    Closed,
}

/// stops its task when dropped.
struct Reader(JoinHandle<()>);

impl Drop for Reader {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// reads the client's lines on a task of its own, so waiting for a line never
/// splits one, and holds the next line back until the previous one is taken.
fn read_lines<R: AsyncRead + Unpin + Send + 'static>(read: R) -> (mpsc::Receiver<Read>, Reader) {
    let (sender, lines) = mpsc::channel(1);
    let task = tokio::spawn(async move {
        let mut reader = BufReader::new(read);
        loop {
            let mut line = Vec::new();
            let mut limited = (&mut reader).take(MAX_CLIENT_MESSAGE_BYTES as u64);
            let read = match limited.read_until(b'\n', &mut line).await {
                Ok(count) if count > 0 && line.last() == Some(&b'\n') => Read::Line(line),
                Ok(count) if count >= MAX_CLIENT_MESSAGE_BYTES => Read::TooLong,
                _ => Read::Closed,
            };
            let last = !matches!(read, Read::Line(_));
            if sender.send(read).await.is_err() || last {
                return;
            }
        }
    });
    (lines, Reader(task))
}

/// the write half of a connection, which can be dropped without waiting for the peer
/// to read what was written to it.
pub trait Abandon {
    /// takes back the wait for the peer to read, so dropping the half returns at once.
    /// On Windows the pipe crate otherwise sends a stream whose peer has not read to
    /// a blocking thread that waits for it, for as long as the peer stays.
    fn abandon(&mut self);
}

impl<S> Abandon for tokio::io::WriteHalf<S> {
    fn abandon(&mut self) {}
}

/// a write half that is abandoned when it is dropped, for whatever reason the
/// connection ended; what must reach the peer is flushed on purpose, in a bounded time.
struct Writer<W: Abandon>(W);

impl<W: Abandon> Drop for Writer<W> {
    fn drop(&mut self) {
        self.0.abandon();
    }
}

impl<W: AsyncWrite + Abandon + Unpin> AsyncWrite for Writer<W> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.0).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0).poll_shutdown(cx)
    }
}

/// whether a message is only queued for the peer, or must reach it because the
/// connection ends with it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Delivery {
    Queued,
    Final,
}

/// writes a message in at most `within`; false when it was not written, and the
/// connection ends. A `Final` message is also flushed in that time, which on Windows
/// waits for the peer to read it; a stalled peer costs the time and, there, one
/// blocking thread until it reads or closes, which the end of the process does not wait for.
async fn write<W: AsyncWrite + Unpin>(
    writer: &mut W,
    message: &ServerMessage<'_>,
    within: Duration,
    delivery: Delivery,
) -> bool {
    let Some(line) = message.line() else {
        return false;
    };
    if line.len() > MAX_AGENT_MESSAGE_BYTES {
        return false;
    }
    let written = async {
        writer.write_all(&line).await?;
        if delivery == Delivery::Final {
            writer.flush().await?;
        }
        Ok::<(), io::Error>(())
    };
    timeout(within, written)
        .await
        .is_ok_and(|outcome| outcome.is_ok())
}

/// the value a subscription follows.
enum Feed {
    State(watch::Receiver<Arc<Snapshot>>),
    Github {
        values: watch::Receiver<Arc<GithubStatus>>,
        /// when it was subscribed or last renewed.
        renewed: Instant,
        _watching: Watching,
    },
}

impl Feed {
    fn topic(&self) -> Topic {
        match self {
            Self::State(_) => Topic::State,
            Self::Github { .. } => Topic::Github,
        }
    }
}

struct Subscription {
    feed: Feed,
    seq: u64,
    /// when the latest event or heartbeat was written.
    last_push: Instant,
    /// when the next event is due, once the snapshot changed.
    due: Option<Instant>,
    _slot: Subscribed,
}

/// the answer to a request still with the agent.
type Pending = Option<(u64, oneshot::Receiver<ApiResult>)>;

async fn answered(pending: &mut Pending) -> Result<ApiResult, oneshot::error::RecvError> {
    match pending {
        Some((_, answer)) => answer.await,
        None => std::future::pending().await,
    }
}

async fn changed(subscription: &mut Option<Subscription>) {
    let changed = match subscription
        .as_mut()
        .map(|subscription| &mut subscription.feed)
    {
        Some(Feed::State(snapshots)) => {
            let changed = snapshots.changed().await.is_ok();
            snapshots.borrow_and_update();
            changed
        }
        Some(Feed::Github { values, .. }) => {
            let changed = values.changed().await.is_ok();
            values.borrow_and_update();
            changed
        }
        None => false,
    };
    if !changed {
        pending::<()>().await;
    }
}

/// the reason the agent is ending, once it is; never, when the agent drops it unset.
async fn ending_reason(ending: &mut watch::Receiver<Option<EndingNotice>>) -> EndingNotice {
    loop {
        if let Some(notice) = ending.borrow_and_update().clone() {
            return notice;
        }
        if ending.changed().await.is_err() {
            pending::<()>().await;
        }
    }
}

async fn at(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => sleep_until(deadline).await,
        None => pending().await,
    }
}

fn error(code: ErrorCode, message: &str) -> ErrorBody {
    ErrorBody::new(code, message)
}

/// the `id` of a line that says it is a request and has an `id` that is an unsigned
/// integer, however else it is malformed.
fn readable_request_id(line: &[u8]) -> Option<u64> {
    let value: serde_json::Value = serde_json::from_slice(line).ok()?;
    (value.get("type")?.as_str()? == "request")
        .then(|| value.get("id")?.as_u64())
        .flatten()
}

/// serves one connection until it ends, for any reason.
pub async fn serve<R, W>(read: R, writer: W, shared: Arc<Shared>, admission: Admission)
where
    R: AsyncRead + Unpin + Send + 'static,
    W: AsyncWrite + Abandon + Unpin + Send,
{
    let limits = shared.limits;
    let mut writer = Writer(writer);
    let (mut lines, _reader) = read_lines(read);
    let mut ending = shared.ending.clone();
    let first = tokio::select! {
        first = timeout(limits.handshake, lines.recv()) => match first {
            Ok(Some(Read::Line(line))) => line,
            _ => return,
        },
        notice = ending_reason(&mut ending) => {
            write(
                &mut writer,
                &ServerMessage::Ending {
                    reason: &notice.reason,
                    update: notice.update.as_ref(),
                },
                limits.write,
                Delivery::Final,
            )
            .await;
            return;
        }
    };
    let _slot = match admission {
        Admission::Busy(slot) => {
            let body = error(
                ErrorCode::Busy,
                "the agent has the most connections it serves; try again later",
            );
            write(
                &mut writer,
                &ServerMessage::Error { error: &body },
                limits.write,
                Delivery::Final,
            )
            .await;
            drop(slot);
            return;
        }
        Admission::Open(slot) => slot,
    };
    let welcomed = match serde_json::from_slice::<ClientMessage>(&first) {
        Ok(ClientMessage::Hello { versions, .. }) => versions.contains(&VERSION),
        _ => false,
    };
    if !welcomed {
        let body = ErrorBody::unsupported_protocol();
        write(
            &mut writer,
            &ServerMessage::Error { error: &body },
            limits.write,
            Delivery::Final,
        )
        .await;
        return;
    }
    let welcome = ServerMessage::Welcome {
        version: VERSION,
        agent: AgentVersion {
            version: shared.agent_version.clone(),
        },
        methods: &METHODS,
        topics: &TOPICS,
    };
    if !write(&mut writer, &welcome, limits.write, Delivery::Queued).await {
        return;
    }
    Session {
        shared: &shared,
        writer,
        lines,
        subscription: None,
        pending: None,
        last_received: Instant::now(),
        drain_requests: Vec::new(),
    }
    .run()
    .await;
}

struct Session<'a, W: Abandon> {
    shared: &'a Shared,
    writer: Writer<W>,
    lines: mpsc::Receiver<Read>,
    subscription: Option<Subscription>,
    pending: Pending,
    last_received: Instant,
    /// dropped with the session, which tells the agent that the connection that asked
    /// for a drain has closed.
    drain_requests: Vec<oneshot::Sender<()>>,
}

impl<W: AsyncWrite + Abandon + Unpin> Session<'_, W> {
    async fn send(&mut self, message: &ServerMessage<'_>) -> bool {
        write(
            &mut self.writer,
            message,
            self.shared.limits.write,
            Delivery::Queued,
        )
        .await
    }

    async fn run(mut self) {
        let limits = self.shared.limits;
        let mut ending = self.shared.ending.clone();
        loop {
            let notice = ending.borrow_and_update().clone();
            if let Some(notice) = notice {
                self.answer_if_ready().await;
                write(
                    &mut self.writer,
                    &ServerMessage::Ending {
                        reason: &notice.reason,
                        update: notice.update.as_ref(),
                    },
                    limits.write,
                    Delivery::Final,
                )
                .await;
                return;
            }
            let idle = (self.subscription.is_none() && self.pending.is_none())
                .then(|| self.last_received + limits.idle);
            let heartbeat = self
                .subscription
                .as_ref()
                .map(|subscription| subscription.last_push + limits.heartbeat);
            let due = self
                .subscription
                .as_ref()
                .and_then(|subscription| subscription.due);
            let lapses =
                self.subscription
                    .as_ref()
                    .and_then(|subscription| match &subscription.feed {
                        Feed::Github { renewed, .. } => Some(*renewed + limits.renewal),
                        Feed::State(_) => None,
                    });
            let proceed = tokio::select! {
                biased;
                _ = ending.changed() => true,
                answer = answered(&mut self.pending) => self.respond(answer).await,
                () = changed(&mut self.subscription) => {
                    self.schedule_event();
                    true
                }
                () = at(due) => self.push_event().await,
                () = at(heartbeat) => self.push_heartbeat().await,
                () = at(lapses) => {
                    // the connection stays; the next `renew` is told the
                    // subscription ended
                    self.subscription = None;
                    self.last_received = Instant::now();
                    true
                }
                () = at(idle) => false,
                line = self.lines.recv(), if self.pending.is_none() => match line {
                    Some(Read::Line(line)) => {
                        self.last_received = Instant::now();
                        self.handle(&line).await
                    }
                    _ => false,
                },
            };
            if !proceed {
                return;
            }
        }
    }

    /// answers the pending request when its answer is already there, so the answer to
    /// a `quit` is not lost to the `ending` that follows it.
    async fn answer_if_ready(&mut self) {
        let ready = match &mut self.pending {
            Some((_, receiver)) => receiver.try_recv().ok(),
            None => None,
        };
        if let (Some(answer), Some((id, _))) = (ready, self.pending.take()) {
            self.answer(id, &answer).await;
        }
    }

    fn schedule_event(&mut self) {
        let coalesce = self.shared.limits.coalesce;
        if let Some(subscription) = &mut self.subscription
            && subscription.due.is_none()
        {
            subscription.due = Some((subscription.last_push + coalesce).max(Instant::now()));
        }
    }

    async fn push_event(&mut self) -> bool {
        let Some(subscription) = &mut self.subscription else {
            return true;
        };
        subscription.due = None;
        subscription.seq += 1;
        subscription.last_push = Instant::now();
        let seq = subscription.seq;
        match &mut subscription.feed {
            Feed::State(snapshots) => {
                let snapshot = Arc::clone(&snapshots.borrow_and_update());
                self.send(&ServerMessage::Event {
                    topic: Topic::State.word(),
                    seq,
                    value: EventValue::State(&snapshot),
                })
                .await
            }
            Feed::Github { values, .. } => {
                let value = Arc::clone(&values.borrow_and_update());
                self.send(&ServerMessage::Event {
                    topic: Topic::Github.word(),
                    seq,
                    value: EventValue::Github(&value),
                })
                .await
            }
        }
    }

    async fn push_heartbeat(&mut self) -> bool {
        let Some(subscription) = &mut self.subscription else {
            return true;
        };
        subscription.last_push = Instant::now();
        let seq = subscription.seq;
        self.send(&ServerMessage::Heartbeat { seq }).await
    }

    async fn respond(&mut self, answer: Result<ApiResult, oneshot::error::RecvError>) -> bool {
        let Some((id, _)) = self.pending.take() else {
            return true;
        };
        let answer = answer
            .unwrap_or_else(|_| Err(ApiError::new(ErrorCode::Refused, "the agent is ending")));
        self.answer(id, &answer).await
    }

    async fn answer(&mut self, id: u64, answer: &ApiResult) -> bool {
        let body = answer.as_ref().err().map(ApiError::body);
        self.send(&ServerMessage::Response {
            id,
            result: answer.as_ref().ok(),
            error: body.as_ref(),
        })
        .await
    }

    async fn handle(&mut self, line: &[u8]) -> bool {
        let Ok(ClientMessage::Request { id, method, params }) =
            serde_json::from_slice::<ClientMessage>(line)
        else {
            let body = error(ErrorCode::InvalidRequest, "the message is not a request");
            // a request that is malformed but names its id is answered under that id,
            // so the client that waits for it is not left waiting
            return match readable_request_id(line) {
                Some(id) => {
                    self.answer(
                        id,
                        &Err(ApiError::new(
                            ErrorCode::InvalidRequest,
                            "the request is malformed",
                        )),
                    )
                    .await
                }
                None => self.send(&ServerMessage::Error { error: &body }).await,
            };
        };
        match parse(&method, params) {
            Err(refusal) => self.answer(id, &Err(refusal)).await,
            Ok(Parsed::Subscribe(topic)) => {
                let answer = self.subscribe(topic);
                let sent = self.answer(id, &answer).await;
                self.schedule_event();
                sent
            }
            Ok(Parsed::Renew(topic)) => {
                let answer = self.renew(topic);
                self.answer(id, &answer).await
            }
            Ok(Parsed::Call(mut method)) => {
                if let Method::DrainForUpdate { requester, .. } = &mut method {
                    let (alive, closed) = oneshot::channel();
                    self.drain_requests.retain(|sender| !sender.is_closed());
                    self.drain_requests.push(alive);
                    *requester = Some(closed);
                }
                let (reply, answer) = oneshot::channel();
                if self.shared.calls.send(Call { method, reply }).is_err() {
                    let gone = Err(ApiError::new(ErrorCode::Refused, "the agent is ending"));
                    return self.answer(id, &gone).await;
                }
                self.pending = Some((id, answer));
                true
            }
        }
    }

    fn renew(&mut self, topic: Topic) -> ApiResult {
        match self
            .subscription
            .as_mut()
            .map(|subscription| &mut subscription.feed)
        {
            Some(Feed::Github { renewed, .. }) if topic == Topic::Github => {
                *renewed = Instant::now();
                Ok(serde_json::json!({ "topic": topic.word() }))
            }
            _ => Err(ApiError::new(
                ErrorCode::NotSubscribed,
                format!(
                    "this connection holds no {} subscription; subscribe again",
                    topic.word()
                ),
            )),
        }
    }

    fn subscribe(&mut self, topic: Topic) -> ApiResult {
        if let Some(subscription) = &mut self.subscription {
            let held = subscription.feed.topic();
            if held != topic {
                return Err(ApiError::new(
                    ErrorCode::InvalidRequest,
                    format!(
                        "this connection already holds a {} subscription; open another connection",
                        held.word()
                    ),
                ));
            }
            if let Feed::Github { renewed, .. } = &mut subscription.feed {
                *renewed = Instant::now();
            }
        } else {
            let limits = &self.shared.limits;
            let Some(slot) = self.shared.counts.subscribe(limits) else {
                return Err(ApiError::new(
                    ErrorCode::SubscriptionLimit,
                    format!(
                        "the agent serves {} subscriptions at once",
                        limits.subscriptions
                    ),
                ));
            };
            let feed = match topic {
                Topic::State => {
                    let mut snapshots = self.shared.snapshots.clone();
                    snapshots.borrow_and_update();
                    Feed::State(snapshots)
                }
                Topic::Github => {
                    let mut values = self.shared.github.clone();
                    values.borrow_and_update();
                    Feed::Github {
                        values,
                        renewed: Instant::now(),
                        _watching: self.shared.watchers.join(),
                    }
                }
            };
            self.subscription = Some(Subscription {
                feed,
                seq: 0,
                last_push: Instant::now()
                    .checked_sub(limits.coalesce)
                    .unwrap_or_else(Instant::now),
                due: None,
                _slot: slot,
            });
        }
        Ok(serde_json::json!({ "topic": topic.word() }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_limits_are_the_documented_ones() {
        let limits = Limits::default();
        assert_eq!(limits.handshake, Duration::from_secs(5));
        assert_eq!(limits.idle, Duration::from_secs(60));
        assert_eq!(limits.write, Duration::from_secs(5));
        assert_eq!(limits.heartbeat, Duration::from_secs(15));
        assert_eq!(limits.coalesce, Duration::from_millis(250));
        assert_eq!(limits.connections, 16);
        assert_eq!(limits.subscriptions, 4);
        assert_eq!(limits.renewal, Duration::from_secs(90));
    }
}
