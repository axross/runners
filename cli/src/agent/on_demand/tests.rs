#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;
use tokio::time::{Instant, timeout};

use crate::runtime::clock::SystemClock;
use crate::runtime::fake::MemoryLog;
use crate::runtime::registration::{HttpLimits, JitRequest, Registrar};
use crate::runtime::secret::Secret;
use crate::runtime::token::TokenSource;
use crate::scaleset::{Client, JobEvent, JobEventKind, Message, Statistics};

use super::demand::Demand;
use super::*;

const RUNNER: &str = "example-linux-1-20260102030405006";

fn statistics(assigned: u64) -> Statistics {
    Statistics {
        total_assigned_jobs: assigned,
        ..Statistics::default()
    }
}

fn offers(ids: &[u64]) -> Message {
    Message {
        id: 7,
        statistics: statistics(0),
        jobs: ids
            .iter()
            .map(|&request_id| JobEvent {
                kind: JobEventKind::Available,
                request_id,
            })
            .collect(),
    }
}

#[test]
fn initial_assigned_demand_and_failed_start_survive_without_events() {
    let mut demand = Demand::default();
    demand.initial(statistics(2));
    let first = demand.reserve(1, 1).unwrap();
    assert!(demand.reserve(2, 1).is_none());
    demand.finish(first, false);
    let retry = demand.reserve(1, 1).unwrap();
    demand.finish(retry, true);
    let second = demand.reserve(1, 1).unwrap();
    demand.finish(second, true);
    assert!(demand.reserve(1, 1).is_none());
    assert_eq!(demand.statistics.total_assigned_jobs, 2);
}

#[test]
fn statistics_before_cleanup_do_not_consume_waiting_demand_twice() {
    for recreated in [false, true] {
        let mut demand = Demand::default();
        demand.initial(statistics(2));
        let first = demand.reserve(1, 1).unwrap();
        if recreated {
            demand.initial(statistics(1));
        } else {
            demand.update_statistics(statistics(1));
        }
        demand.finish(first, true);
        let second = demand.reserve(1, 1).unwrap();
        demand.finish(second, true);
        assert!(demand.reserve(1, 1).is_none());
    }
}

#[test]
fn cleanup_before_statistics_does_not_credit_the_next_completion() {
    let mut demand = Demand::default();
    demand.initial(statistics(3));
    let first = demand.reserve(1, 1).unwrap();
    demand.finish(first, true);
    demand.update_statistics(statistics(2));
    let second = demand.reserve(1, 1).unwrap();
    demand.finish(second, true);
    let third = demand.reserve(1, 1).unwrap();
    demand.finish(third, true);
    assert!(demand.reserve(1, 1).is_none());
}

#[test]
fn overflow_is_not_acknowledgeable_and_duplicate_delivery_does_not_start_twice() {
    let mut demand = Demand::default();
    let message = offers(&[11, 12]);
    demand.receive(&message);
    let first = demand.acquire(1);
    assert_eq!(first, vec![11]);
    assert_eq!(demand.occupied(), 1);
    assert!(demand.reserve(1, 1).is_none());
    demand.acquired(&first, &first);
    let lease = demand.reserve(1, 1).unwrap();
    demand.receive(&message);
    assert!(!demand.processed());
    assert!(demand.acquire(1).is_empty());
    assert!(demand.reserve(1, 1).is_none());
    demand.finish(lease, true);
    let second = demand.acquire(1);
    assert_eq!(second, vec![12]);
    demand.acquired(&second, &second);
    assert!(demand.processed());
    let lease = demand.reserve(1, 1).unwrap();
    demand.finish(lease, true);
    demand.receive(&message);
    assert!(demand.reserve(1, 1).is_none());
}

#[test]
fn response_unknown_acquire_retries_only_the_same_held_ids() {
    let mut demand = Demand::default();
    demand.receive(&offers(&[11, 12, 13]));
    assert_eq!(demand.acquire(2), vec![11, 12]);
    assert_eq!(demand.acquire(2), vec![11, 12]);
    assert_eq!(demand.occupied(), 2);
    demand.acquired(&[11, 12], &[]);
    let mut assigned = offers(&[11, 12, 13]);
    assigned.statistics = statistics(2);
    demand.receive(&assigned);
    assert!(demand.acquire(2).is_empty());
    let a = demand.reserve(1, 2).unwrap();
    let b = demand.reserve(2, 2).unwrap();
    assert!(demand.reserve(3, 2).is_none());
    demand.finish(a, true);
    demand.finish(b, true);
    assert_eq!(demand.acquire(2), vec![13]);
}

#[test]
fn acquired_demand_is_not_counted_again_when_statistics_catch_up() {
    let mut demand = Demand::default();
    demand.receive(&offers(&[11]));
    demand.acquire(2);
    demand.acquired(&[11], &[11]);
    let lease = demand.reserve(1, 2).unwrap();
    let mut assigned = offers(&[11]);
    assigned.statistics = statistics(1);
    demand.receive(&assigned);
    assert!(demand.reserve(2, 2).is_none());
    demand.finish(lease, true);
    demand.receive(&assigned);
    assert!(demand.reserve(1, 2).is_none());
}

#[test]
fn recreation_and_capacity_reduction_never_release_active_or_uncertain_seats() {
    let mut demand = Demand::default();
    demand.initial(statistics(1));
    let lease = demand.reserve(1, 2).unwrap();
    demand.receive(&Message {
        statistics: statistics(1),
        ..offers(&[11])
    });
    assert_eq!(demand.acquire(2), vec![11]);
    demand.initial(statistics(2));
    assert_eq!(demand.occupied(), 2);
    assert!(demand.reserve(2, 1).is_none());
    demand.finish(lease, false);
    assert_eq!(demand.occupied(), 1);
    assert!(demand.reserve(1, 1).is_none());
}

struct Tokens(RefCell<Result<String, String>>);

impl Tokens {
    fn new() -> Self {
        Self(RefCell::new(Ok("<pat-placeholder>".to_owned())))
    }
}

impl TokenSource for Tokens {
    fn token(&self, _: &str, _: &str, _: Option<&Path>) -> Result<Secret, String> {
        self.0.borrow().clone().map(Secret::new)
    }
}

struct Channels {
    stop: watch::Sender<bool>,
    drain: watch::Sender<bool>,
    capacity: watch::Sender<u32>,
}

impl Channels {
    fn new(capacity: u32) -> Self {
        Self {
            stop: watch::Sender::new(false),
            drain: watch::Sender::new(false),
            capacity: watch::Sender::new(capacity),
        }
    }
    fn controls(&self) -> Controls {
        Controls {
            stop: self.stop.subscribe(),
            drain: self.drain.subscribe(),
            capacity: self.capacity.subscribe(),
        }
    }
}

fn pool() -> Pool {
    Pool {
        owner: "example-owner".to_owned(),
        repository: "example-repository".to_owned(),
        logical_name: "logical-pool".to_owned(),
        scale_set: "example-linux".to_owned(),
        session_owner: "<host-placeholder>".to_owned(),
    }
}

#[test]
fn errors_keep_rate_limit_retry_hints_and_rejection_details_without_secrets() {
    let client = Client::new(None, HttpLimits::default()).unwrap();
    let tokens = Tokens::new();
    let log = MemoryLog::default();
    let channels = Channels::new(1);
    let coordinator = Coordinator::new(
        pool(),
        &client,
        &tokens,
        &SystemClock,
        &log,
        channels.controls(),
    );
    let mut error = super::protocol_error();
    error.kind = ErrorKind::RateLimited;
    error.status = Some(403);
    error.retry_after = Some(Duration::from_secs(30));
    coordinator.failed(&error, Duration::from_secs(5));
    assert_eq!(coordinator.view().state, ListenerState::BackingOff);
    assert!(
        coordinator
            .state
            .borrow()
            .retry_at
            .unwrap()
            .duration_since(SystemTime::now())
            .unwrap()
            > Duration::from_secs(29)
    );
    error.kind = ErrorKind::Conflict;
    error.status = Some(409);
    coordinator.failed(&error, Duration::from_secs(5));
    assert_eq!(coordinator.view().state, ListenerState::AnotherListener);
    error.kind = ErrorKind::LabelMismatch;
    error.status = None;
    coordinator.failed(&error, Duration::from_secs(5));
    assert_eq!(coordinator.view().state, ListenerState::Rejected);
    assert_eq!(
        coordinator.view().reason.as_deref(),
        Some("scale-set label mismatch")
    );
    assert_eq!(coordinator.view().status, None);
    assert!(coordinator.state.borrow().fatal);
    assert!(!log.lines().join("\n").contains("<pat-placeholder>"));
}

#[test]
fn drain_toggled_during_a_request_invalidates_its_admission_and_stop_is_pool_local() {
    let client = Client::new(None, HttpLimits::default()).unwrap();
    let tokens = Tokens::new();
    let log = MemoryLog::default();
    let channels = Channels::new(1);
    let coordinator = Coordinator::new(
        pool(),
        &client,
        &tokens,
        &SystemClock,
        &log,
        channels.controls(),
    );
    coordinator.listening();
    let epoch = coordinator.admission_epoch();
    channels.drain.send_replace(true);
    assert!(!coordinator.admitting());
    channels.drain.send_replace(false);
    assert!(epoch.changed());
    coordinator.stop(Instant::now() + Duration::from_secs(1));
    assert!(!coordinator.admitting());
    assert!(!*channels.stop.borrow());
}

#[cfg(debug_assertions)]
mod http {
    use super::*;

    struct Reply {
        method: &'static str,
        path: &'static str,
        status: u16,
        body: Value,
        headers: &'static str,
        signal: Option<watch::Sender<bool>>,
        lose_response: bool,
    }

    impl Reply {
        fn json(method: &'static str, path: &'static str, status: u16, body: Value) -> Self {
            Self {
                method,
                path,
                status,
                body,
                headers: "",
                signal: None,
                lose_response: false,
            }
        }
        fn empty(method: &'static str, path: &'static str) -> Self {
            Self::json(method, path, 204, Value::Null)
        }
    }

    /// reads one request and returns its method, path and body.
    async fn read_request(stream: &mut TcpStream) -> (String, String, Vec<u8>) {
        try_read_request(stream).await.unwrap()
    }

    /// like `read_request`, but `None` for a client that went away before it finished.
    async fn try_read_request(stream: &mut TcpStream) -> Option<(String, String, Vec<u8>)> {
        let mut bytes = Vec::new();
        let mut buffer = [0; 4096];
        let end = loop {
            let count = stream
                .read(&mut buffer)
                .await
                .ok()
                .filter(|count| *count > 0)?;
            bytes.extend_from_slice(&buffer[..count]);
            assert!(bytes.len() < 1024 * 1024);
            if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                break end + 4;
            }
        };
        let head = String::from_utf8_lossy(&bytes[..end]).into_owned();
        let mut first = head.lines().next().unwrap().split(' ');
        let method = first.next().unwrap().to_owned();
        let path = first.next().unwrap().to_owned();
        let size = head
            .lines()
            .filter_map(|line| line.split_once(':'))
            .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
            .map_or(0, |(_, size)| size.trim().parse::<usize>().unwrap());
        let mut body = bytes[end..].to_vec();
        while body.len() < size {
            let count = stream
                .read(&mut buffer)
                .await
                .ok()
                .filter(|count| *count > 0)?;
            body.extend_from_slice(&buffer[..count]);
        }
        Some((method, path, body))
    }

    struct Fake {
        listener: TcpListener,
        base: String,
        requests: RefCell<Vec<(String, String)>>,
    }

    impl Fake {
        async fn new() -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            Self {
                base: format!("http://{}", listener.local_addr().unwrap()),
                listener,
                requests: RefCell::new(Vec::new()),
            }
        }

        async fn serve(&self, replies: Vec<Reply>) {
            for reply in replies {
                let (mut stream, _) = timeout(Duration::from_secs(3), self.listener.accept())
                    .await
                    .unwrap()
                    .unwrap();
                let (method, path, _) = read_request(&mut stream).await;
                assert_eq!(method, reply.method);
                assert!(path.contains(reply.path), "{path} lacks {}", reply.path);
                self.requests.borrow_mut().push((method, path));
                if let Some(signal) = reply.signal {
                    signal.send_replace(true);
                }
                if reply.lose_response {
                    continue;
                }
                let body = if reply.status == 204 || reply.status == 202 {
                    String::new()
                } else {
                    reply.body.to_string().replace("{{base}}", &self.base)
                };
                let response = format!(
                    "HTTP/1.1 {} Reply\r\n{}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    reply.status,
                    reply.headers,
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes()).await;
            }
        }

        fn count(&self, method: &str, part: &str) -> usize {
            self.requests
                .borrow()
                .iter()
                .filter(|(m, path)| m == method && path.contains(part))
                .count()
        }

        fn client(&self) -> Client {
            Client::new(
                Some(&self.base),
                HttpLimits {
                    total_timeout: Duration::from_secs(1),
                    ..HttpLimits::default()
                },
            )
            .unwrap()
        }

        /// a client that waits `delay` between the attempts of a refused registration exchange.
        fn client_retrying(&self, delay: Duration) -> Client {
            self.client().with_exchange_retry_delay(delay)
        }
    }

    fn connect() -> Vec<Reply> {
        vec![
            Reply::json(
                "POST",
                "registration-token",
                201,
                json!({"token": "<registration-placeholder>"}),
            ),
            Reply::json(
                "POST",
                "runner-registration",
                200,
                json!({"url": "{{base}}/tenant", "token": (["fake", "eyJleHAiOjQxMDI0NDQ4MDB9", "signature"].join("."))}),
            ),
            Reply::json(
                "GET",
                "runnerscalesets?",
                200,
                json!({"count": 1, "value": [{"id": 23, "name": "example-linux", "runnerGroupId": 1, "labels": [{"name": "example-linux"}]}]}),
            ),
        ]
    }

    fn session() -> Reply {
        Reply::json(
            "POST",
            "sessions",
            200,
            json!({"sessionId": "01234567-89ab-cdef-0123-456789abcdef", "messageQueueUrl": "{{base}}/queue", "messageQueueAccessToken": "<queue-placeholder>", "statistics": statistics_json(1)}),
        )
    }

    fn statistics_json(assigned: u64) -> Value {
        json!({"totalAssignedJobs": assigned, "totalRunningJobs": 0, "totalIdleRunners": 0})
    }

    struct ControlledClock {
        time: crate::runtime::fake::FakeClock,
        changed: watch::Sender<u64>,
    }

    impl ControlledClock {
        fn new() -> Self {
            Self {
                time: crate::runtime::fake::FakeClock::at(1_800_000_000_000),
                changed: watch::Sender::new(0),
            }
        }

        fn advance(&self, duration: Duration) {
            self.time.advance(duration);
            self.changed.send_modify(|version| *version += 1);
        }
    }

    impl Clock for ControlledClock {
        fn now(&self) -> SystemTime {
            self.time.now()
        }

        async fn sleep(&self, duration: Duration) {
            let until = self.now() + duration;
            let mut changed = self.changed.subscribe();
            while self.now() < until {
                changed.changed().await.unwrap();
            }
        }
    }

    /// the fake clock's long-poll hold, as the service holds an idle poll.
    const POLL_HOLD: Duration = Duration::from_secs(50);

    const SESSION_ID: &str = "01234567-89ab-cdef-0123-456789abcdef";

    /// a service that answers by path, not by script, for runs whose length the clock sets;
    /// it handles one request at a time, and an idle poll waits `POLL_HOLD` on that clock.
    struct Service<'c> {
        listener: TcpListener,
        base: String,
        clock: &'c ControlledClock,
        /// how long each admin credential it issues lives, from the clock's now.
        admin_lifetime: Duration,
        requests: RefCell<Vec<(String, String)>>,
        /// the messages the queue redelivers at once until the session acknowledges them.
        message: RefCell<Option<Value>>,
        acknowledged: std::cell::Cell<bool>,
        /// a service that hands the message back even after it was acknowledged.
        stale: std::cell::Cell<bool>,
    }

    impl<'c> Service<'c> {
        async fn new(clock: &'c ControlledClock, admin_lifetime: Duration) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            Self {
                base: format!("http://{}", listener.local_addr().unwrap()),
                listener,
                clock,
                admin_lifetime,
                requests: RefCell::new(Vec::new()),
                message: RefCell::new(None),
                acknowledged: std::cell::Cell::new(false),
                stale: std::cell::Cell::new(false),
            }
        }

        fn client(&self) -> Client {
            Client::new(Some(&self.base), HttpLimits::default())
                .unwrap()
                .with_exchange_retry_delay(Duration::from_millis(1))
        }

        /// the requests that reach `api.github.com` in production: everything under `/repos/`.
        fn rest(&self) -> Vec<(String, String)> {
            self.requests
                .borrow()
                .iter()
                .filter(|(_, path)| path.starts_with("/repos/"))
                .cloned()
                .collect()
        }

        fn count(&self, method: &str, part: &str) -> usize {
            self.requests
                .borrow()
                .iter()
                .filter(|(m, path)| m == method && path.contains(part))
                .count()
        }

        async fn serve(&self) {
            loop {
                let (mut stream, _) = self.listener.accept().await.unwrap();
                let Some((method, path, body)) = try_read_request(&mut stream).await else {
                    continue;
                };
                self.requests
                    .borrow_mut()
                    .push((method.clone(), path.clone()));
                let answered = match self.answer(&method, &path, &body) {
                    Some(answered) => answered,
                    // an idle poll is held for the hold's length, and dropped with its client
                    None => {
                        let mut byte = [0; 1];
                        tokio::select! {
                            () = self.clock.sleep(POLL_HOLD) => (202, Value::Null),
                            _ = stream.read(&mut byte) => continue,
                        }
                    }
                };
                let (status, body) = answered;
                let body = if status == 202 || status == 204 {
                    String::new()
                } else {
                    body.to_string()
                };
                let response = format!(
                    "HTTP/1.1 {status} Reply\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes()).await;
            }
        }

        /// `None` is an idle poll, which the caller holds.
        fn answer(&self, method: &str, path: &str, body: &[u8]) -> Option<(u16, Value)> {
            let route = path.split('?').next().unwrap();
            let answer = if route.ends_with("registration-token") {
                (201, json!({"token": "<registration-placeholder>"}))
            } else if route.ends_with("runner-registration") {
                let exp = (self.clock.now() + self.admin_lifetime)
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs();
                (
                    200,
                    json!({"url": format!("{}/tenant", self.base), "token": jwt(exp)}),
                )
            } else if route.starts_with("/repos/") && route.ends_with("/actions/runners") {
                (200, json!({"total_count": 0, "runners": []}))
            } else if route.ends_with("runnerscalesets") {
                (
                    200,
                    json!({"count": 1, "value": [{"id": 23, "name": "example-linux", "runnerGroupId": 1, "labels": [{"name": "example-linux"}]}]}),
                )
            } else if route.ends_with("sessions")
                || route.contains("/sessions/") && method == "PATCH"
            {
                (
                    200,
                    json!({"sessionId": SESSION_ID, "messageQueueUrl": format!("{}/queue", self.base), "messageQueueAccessToken": "<queue-placeholder>", "statistics": statistics_json(0)}),
                )
            } else if route.contains("/sessions/") && method == "DELETE" {
                (204, Value::Null)
            } else if route.ends_with("acquirejobs") {
                let ids: Vec<u64> = serde_json::from_slice(body).unwrap();
                (200, json!({"count": ids.len(), "value": ids}))
            } else if route.starts_with("/queue/") && method == "DELETE" {
                self.acknowledged.set(true);
                (204, Value::Null)
            } else if route == "/queue" {
                let message = self.message.borrow().clone();
                (
                    200,
                    message.filter(|_| self.stale.get() || !self.acknowledged.get())?,
                )
            } else {
                panic!("unexpected request {method} {path}");
            };
            Some(answer)
        }
    }

    /// an unsigned token of the shape the client reads its expiry from.
    fn jwt(exp: u64) -> String {
        const ALPHABET: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
        let claims = format!("{{\"exp\":{exp}}}");
        let mut payload = String::new();
        for chunk in claims.as_bytes().chunks(3) {
            let bits = chunk.iter().enumerate().fold(0_u32, |bits, (index, byte)| {
                bits | (u32::from(*byte) << (16 - 8 * index))
            });
            for index in 0..=chunk.len() {
                payload.push(char::from(
                    ALPHABET[((bits >> (18 - 6 * index)) & 63) as usize],
                ));
            }
        }
        format!("header.{payload}.signature")
    }

    /// moves the clock a little at a time until `horizon` has passed, letting the pool settle
    /// after each step, so the sim does not run ahead of the tasks it drives.
    async fn run_for(clock: &ControlledClock, service: &Service<'_>, horizon: Duration) {
        let end = clock.now() + horizon;
        while clock.now() < end {
            clock.advance(Duration::from_secs(10));
            // a pool that never settles is left to the caller's counts, not waited on
            for _ in 0..25 {
                let seen = service.requests.borrow().len();
                tokio::time::sleep(Duration::from_millis(4)).await;
                if service.requests.borrow().len() == seen {
                    break;
                }
            }
        }
    }

    /// the opening exchange, the crash-cleanup listing and the first session of a pool.
    fn opening() -> Vec<Reply> {
        let mut replies = connect();
        replies.push(Reply::json(
            "GET",
            "actions/runners?",
            200,
            json!({"total_count": 0, "runners": []}),
        ));
        replies.push(session());
        replies
    }

    /// the replies of a listener that closes its session, finds the set again and opens another.
    fn recreation() -> Vec<Reply> {
        let mut replies = vec![Reply::empty("DELETE", "sessions/")];
        replies.extend(connect());
        replies.push(session());
        replies
    }

    fn gone() -> Reply {
        Reply::json("GET", "queue", 404, json!({"message": "gone"}))
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_404_after_a_recovered_one_recreates_the_scale_set_again() {
        let fake = Fake::new().await;
        let client = fake.client();
        let tokens = Tokens::new();
        let channels = Channels::new(1);
        let log = MemoryLog::default();
        let coordinator = Coordinator::new(
            pool(),
            &client,
            &tokens,
            &SystemClock,
            &log,
            channels.controls(),
        );
        let mut replies = opening();
        replies.push(gone());
        replies.extend(recreation());
        replies.push(Reply::json("GET", "queue", 202, Value::Null));
        replies.push(gone());
        replies.extend(recreation());
        let mut polled = Reply::json("GET", "queue", 202, Value::Null);
        polled.signal = Some(channels.stop.clone());
        replies.push(polled);
        replies.push(Reply::empty("DELETE", "sessions/"));
        tokio::join!(fake.serve(replies), coordinator.run());
        assert_eq!(coordinator.view().state, ListenerState::Stopped);
        assert_eq!(fake.count("POST", "sessions"), 3);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_404_right_after_a_recreation_is_a_rejection() {
        let fake = Fake::new().await;
        let client = fake.client();
        let tokens = Tokens::new();
        let channels = Channels::new(1);
        let log = MemoryLog::default();
        let coordinator = Coordinator::new(
            pool(),
            &client,
            &tokens,
            &SystemClock,
            &log,
            channels.controls(),
        );
        let mut replies = opening();
        replies.push(gone());
        replies.extend(recreation());
        replies.push(gone());
        replies.push(Reply::empty("DELETE", "sessions/"));
        tokio::join!(fake.serve(replies), coordinator.run(), async {
            let mut changed = coordinator.changes();
            timeout(Duration::from_secs(2), async {
                while coordinator.view().state != ListenerState::Rejected {
                    changed.changed().await.unwrap();
                }
            })
            .await
            .unwrap();
            let view = coordinator.view();
            assert_eq!(view.status, Some(404));
            assert_eq!(fake.count("POST", "sessions"), 2);
            channels.stop.send_replace(true);
        });
        assert!(coordinator.state.borrow().fatal);
    }

    /// a registration exchange the service refuses, with `seen` told when the first attempt arrives.
    fn refused_exchange(seen: &watch::Sender<bool>) -> Vec<Reply> {
        let mut refused = Reply::json("POST", "runner-registration", 401, Value::Null);
        refused.signal = Some(seen.clone());
        vec![
            Reply::json(
                "POST",
                "registration-token",
                201,
                json!({"token": "<registration-placeholder>"}),
            ),
            refused,
        ]
    }

    /// waits until the refused exchange is in its pause between attempts, then asks for the stop.
    async fn stop_during_the_exchange_pause(
        seen: &watch::Sender<bool>,
        stop: &watch::Sender<bool>,
    ) -> Instant {
        seen.subscribe().wait_for(|seen| *seen).await.unwrap();
        tokio::time::sleep(Duration::from_millis(300)).await;
        stop.send_replace(true);
        Instant::now()
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_stop_ends_the_listener_while_its_exchange_waits_to_retry() {
        let fake = Fake::new().await;
        let client = fake.client_retrying(Duration::from_secs(1));
        let tokens = Tokens::new();
        let channels = Channels::new(1);
        let log = MemoryLog::default();
        let coordinator = Coordinator::new(
            pool(),
            &client,
            &tokens,
            &SystemClock,
            &log,
            channels.controls(),
        );
        let seen = watch::Sender::new(false);
        let (_, ended, stopped) = tokio::join!(
            fake.serve(refused_exchange(&seen)),
            async {
                coordinator.run().await;
                Instant::now()
            },
            stop_during_the_exchange_pause(&seen, &channels.stop)
        );
        assert!(
            ended.duration_since(stopped) < Duration::from_millis(500),
            "the stop waited {:?} for the connection",
            ended.duration_since(stopped)
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_stop_ends_a_registration_while_its_exchange_waits_to_retry() {
        let fake = Fake::new().await;
        let client = fake.client_retrying(Duration::from_secs(1));
        let tokens = Tokens::new();
        let channels = Channels::new(1);
        let log = MemoryLog::default();
        let coordinator = Coordinator::new(
            pool(),
            &client,
            &tokens,
            &SystemClock,
            &log,
            channels.controls(),
        );
        let seen = watch::Sender::new(false);
        let mut replies = connect();
        replies.push(session());
        replies.extend(refused_exchange(&seen));
        let (_, (outcome, ended), stopped) = tokio::join!(
            fake.serve(replies),
            async {
                seed(&coordinator).await;
                let mut lease = coordinator
                    .reserve(1, channels.stop.subscribe())
                    .await
                    .unwrap();
                // the replaced token makes the registration renew the credentials first
                *tokens.0.borrow_mut() = Ok("<replacement-pat-placeholder>".to_owned());
                let token = Secret::new("<unused-runtime-token>".to_owned());
                let outcome = lease.request_jit(&request(&token)).await;
                let ended = Instant::now();
                lease
                    .finish(false, Instant::now() + Duration::from_secs(1))
                    .await
                    .unwrap();
                (outcome, ended)
            },
            stop_during_the_exchange_pause(&seen, &channels.stop)
        );
        assert!(outcome.unwrap_err().to_string().contains("halted"));
        assert!(
            ended.duration_since(stopped) < Duration::from_millis(500),
            "the stop waited {:?} for the connection",
            ended.duration_since(stopped)
        );
        assert_eq!(fake.count("POST", "generatejitconfig"), 0);
    }

    /// what an idle pool did to the service, and where it stood, after `horizon` of the clock.
    struct IdleRun {
        rest: Vec<(String, String)>,
        registrations: usize,
        polls: usize,
        state: ListenerState,
        reason: Option<String>,
    }

    /// runs a pool with nothing to do against a service whose admin credentials live `lifetime`.
    async fn idle_pool(lifetime: Duration, horizon: Duration) -> IdleRun {
        let clock = ControlledClock::new();
        let service = Service::new(&clock, lifetime).await;
        let client = service.client();
        let tokens = Tokens::new();
        let channels = Channels::new(1);
        let log = MemoryLog::default();
        let coordinator =
            Coordinator::new(pool(), &client, &tokens, &clock, &log, channels.controls());
        let view = timeout(Duration::from_secs(60), async {
            tokio::select! {
                () = service.serve() => unreachable!(),
                view = async {
                    let (_, view) = tokio::join!(coordinator.run(), async {
                        run_for(&clock, &service, horizon).await;
                        let view = coordinator.view();
                        channels.stop.send_replace(true);
                        view
                    });
                    view
                } => view,
            }
        })
        .await
        .unwrap();
        IdleRun {
            rest: service.rest(),
            registrations: service.count("POST", "registration-token"),
            polls: service.count("GET", "/queue"),
            state: view.state,
            reason: view.reason,
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_credential_due_again_at_once_backs_off_instead_of_renewing_in_a_loop() {
        let run = idle_pool(Duration::from_secs(100), Duration::from_secs(3600)).await;
        assert_eq!(run.state, ListenerState::BackingOff);
        assert_eq!(
            run.reason.as_deref(),
            Some("scale-set connection renewal required")
        );
        assert_eq!(run.polls, 0, "a pool that cannot register polls for no job");
        // 5 s doubling to 300 s fits about seventeen attempts in an hour
        assert!(
            (2..=25).contains(&run.registrations),
            "{} registration tokens in an hour",
            run.registrations
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn two_idle_hours_reach_the_rest_api_only_for_the_start_up_listing_and_renewals() {
        // the probe's admin credential lasted 1,200 s and is renewed 120 s before it ends
        let lifetime = Duration::from_secs(1200);
        let horizon = Duration::from_secs(2 * 3600);
        let run = idle_pool(lifetime, horizon).await;
        assert_eq!(run.state, ListenerState::Listening);
        let renewals = horizon.as_secs() / (lifetime - REFRESH_BEFORE).as_secs();
        let listings = run
            .rest
            .iter()
            .filter(|(method, path)| method == "GET" && path.contains("/actions/runners?"))
            .count();
        assert_eq!(listings, 1, "one crash-cleanup listing per pool start");
        // the opening exchange, then one more at each renewal
        assert!(
            (renewals..=renewals + 2).contains(&(run.registrations as u64)),
            "{} registration tokens against {renewals} renewals",
            run.registrations
        );
        assert_eq!(
            run.rest.len(),
            listings + run.registrations,
            "an idle pool made REST requests beyond them: {:?}",
            run.rest
        );
        assert!(run.polls >= 100, "the pool did not idle on the broker");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn renewals_of_a_short_lived_credential_are_never_closer_than_the_floor() {
        let run = idle_pool(Duration::from_secs(135), Duration::from_secs(3600)).await;
        assert_eq!(run.state, ListenerState::Listening);
        // one renewal per 30 s at most, where the lifetime alone would ask for one per 15 s
        assert!(
            (20..=125).contains(&run.registrations),
            "{} registration tokens in an hour",
            run.registrations
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn listener_loop_honors_retry_after_403_and_keeps_permission_403_fatal() {
        for rate_limited in [true, false] {
            let fake = Fake::new().await;
            let client = fake.client();
            let tokens = Tokens::new();
            let channels = Channels::new(1);
            let log = MemoryLog::default();
            let clock = ControlledClock::new();
            let coordinator =
                Coordinator::new(pool(), &client, &tokens, &clock, &log, channels.controls());
            let mut replies = connect();
            replies.push(Reply::json(
                "GET",
                "/actions/runners?",
                200,
                json!({"total_count": 0, "runners": []}),
            ));
            let mut rejected =
                Reply::json("POST", "sessions", 403, json!({"message": "forbidden"}));
            if rate_limited {
                rejected.headers = "Retry-After: 30\r\nx-ratelimit-remaining: 4000\r\n";
            }
            replies.push(rejected);
            if rate_limited {
                replies.extend(connect());
                replies.push(session());
                let mut polled = Reply::json("GET", "/queue", 202, Value::Null);
                polled.signal = Some(channels.stop.clone());
                replies.push(polled);
                replies.push(Reply::empty("DELETE", "sessions/"));
            }
            let drive = async {
                let mut changed = coordinator.changes();
                let expected = if rate_limited {
                    ListenerState::BackingOff
                } else {
                    ListenerState::Rejected
                };
                timeout(Duration::from_secs(2), async {
                    while coordinator.view().state != expected {
                        changed.changed().await.unwrap();
                    }
                })
                .await
                .unwrap();
                assert_eq!(coordinator.view().status, Some(403));
                assert_eq!(fake.requests.borrow().len(), 5);
                clock.advance(Duration::from_secs(29));
                tokio::time::sleep(Duration::from_millis(30)).await;
                assert_eq!(
                    fake.requests.borrow().len(),
                    5,
                    "no HTTP retry before the complete wait"
                );
                assert_eq!(coordinator.view().state, expected);
                clock.advance(Duration::from_secs(1));
                if !rate_limited {
                    clock.advance(Duration::from_secs(3600));
                    tokio::time::sleep(Duration::from_millis(30)).await;
                    assert_eq!(fake.requests.borrow().len(), 5);
                    assert_eq!(coordinator.view().state, ListenerState::Rejected);
                    channels.stop.send_replace(true);
                }
            };
            timeout(Duration::from_secs(3), async {
                tokio::join!(fake.serve(replies), coordinator.run(), drive);
            })
            .await
            .unwrap();
            if rate_limited {
                assert!(
                    fake.requests
                        .borrow()
                        .iter()
                        .any(|(method, path)| method == "GET" && path.starts_with("/queue"))
                );
            }
            assert!(
                !fake
                    .requests
                    .borrow()
                    .iter()
                    .any(|(_, path)| path.contains("generatejitconfig"))
            );
        }
    }

    /// awaits `work` while the clock moves past each wait it reaches.
    async fn advancing<T>(clock: &ControlledClock, work: impl Future<Output = T>) -> T {
        let mut work = std::pin::pin!(work);
        loop {
            tokio::select! {
                output = &mut work => return output,
                () = tokio::time::sleep(Duration::from_millis(20)) => {
                    clock.advance(REDELIVERY_WAIT);
                }
            }
        }
    }

    async fn seed<K: Clock>(coordinator: &Coordinator<'_, Tokens, K>) {
        let pat = coordinator.tokens.token("", "", None).unwrap();
        let connection = Rc::new(
            coordinator
                .client
                .connect("example-owner", "example-repository", &pat)
                .await
                .unwrap(),
        );
        let set = coordinator
            .client
            .find_or_create(&connection, "example-linux")
            .await
            .unwrap();
        let session = Rc::new(
            coordinator
                .client
                .create_session(&connection, &set, "<host-placeholder>")
                .await
                .unwrap(),
        );
        coordinator
            .state
            .borrow_mut()
            .demand
            .initial(session.statistics);
        *coordinator.credentials.borrow_mut() = Some(Credentials {
            connection,
            scale_set: set,
            session,
        });
        coordinator.listening();
    }

    fn request<'a>(token: &'a Secret) -> JitRequest<'a> {
        JitRequest {
            owner: "example-owner",
            repository: "example-repository",
            name: RUNNER,
            labels: &[],
            token,
        }
    }

    fn jit() -> Reply {
        Reply::json(
            "POST",
            "generatejitconfig",
            200,
            json!({"runner": {"id": 99, "name": RUNNER, "runnerScaleSetId": 23}, "encodedJITConfig": "<jit-placeholder>"}),
        )
    }
    fn runner() -> Value {
        json!({"id": 99, "name": RUNNER, "status": "offline", "busy": false})
    }

    #[tokio::test(flavor = "current_thread")]
    async fn jit_followed_by_drain_is_owned_and_cleaned_without_releasing_its_seat_early() {
        let fake = Fake::new().await;
        let client = fake.client();
        let tokens = Tokens::new();
        let channels = Channels::new(1);
        let log = MemoryLog::default();
        let coordinator = Coordinator::new(
            pool(),
            &client,
            &tokens,
            &SystemClock,
            &log,
            channels.controls(),
        );
        let mut replies = connect();
        replies.push(session());
        let mut late = jit();
        late.signal = Some(channels.drain.clone());
        replies.push(late);
        replies.extend([
            Reply::json("GET", "runners/99", 200, runner()),
            Reply::empty("DELETE", "runners/99"),
        ]);
        tokio::join!(fake.serve(replies), async {
            seed(&coordinator).await;
            let mut lease = coordinator
                .reserve(1, channels.stop.subscribe())
                .await
                .unwrap();
            let token = Secret::new("<unused-runtime-token>".to_owned());
            assert!(lease.request_jit(&request(&token)).await.is_err());
            assert_eq!(coordinator.view().active_reservations, 1);
            lease
                .finish(false, Instant::now() + Duration::from_secs(2))
                .await
                .unwrap();
            assert!(coordinator.view().incomplete_registrations.is_empty());
            assert_eq!(coordinator.view().active_reservations, 0);
            assert!(!coordinator.admitting());
        });
        assert!(!log.lines().join("\n").contains("<jit-placeholder>"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn lost_jit_response_is_reconciled_exactly_and_failed_deletion_keeps_capacity() {
        let fake = Fake::new().await;
        let client = fake.client();
        let tokens = Tokens::new();
        let channels = Channels::new(1);
        let log = MemoryLog::default();
        let coordinator = Coordinator::new(
            pool(),
            &client,
            &tokens,
            &SystemClock,
            &log,
            channels.controls(),
        );
        let mut replies = connect();
        replies.push(session());
        let mut lost = jit();
        lost.lose_response = true;
        replies.push(lost);
        replies.extend([
            Reply::json(
                "GET",
                "actions/runners?",
                200,
                json!({"total_count": 1, "runners": [runner()]}),
            ),
            Reply::json("GET", "runners/99", 200, runner()),
            Reply::json("DELETE", "runners/99", 503, Value::Null),
            Reply::json("GET", "runners/99", 404, Value::Null),
        ]);
        tokio::join!(fake.serve(replies), async {
            seed(&coordinator).await;
            let mut lease = coordinator
                .reserve(1, channels.stop.subscribe())
                .await
                .unwrap();
            let token = Secret::new("<unused-runtime-token>".to_owned());
            assert!(lease.request_jit(&request(&token)).await.is_err());
            assert!(
                lease
                    .finish(false, Instant::now() + Duration::from_secs(2))
                    .await
                    .is_err()
            );
            let view = coordinator.view();
            assert_eq!(view.active_reservations, 1);
            assert_eq!(view.incomplete_registrations[0].runner_id, Some(99));
            assert_eq!(view.incomplete_registrations[0].name, RUNNER);
            lease
                .finish(false, Instant::now() + Duration::from_secs(2))
                .await
                .unwrap();
            assert_eq!(coordinator.view().active_reservations, 0);
        });
    }

    #[tokio::test(flavor = "current_thread")]
    async fn replacement_renews_before_jit_and_store_errors_cannot_use_the_cache() {
        let fake = Fake::new().await;
        let client = fake.client();
        let tokens = Tokens::new();
        let channels = Channels::new(1);
        let log = MemoryLog::default();
        let coordinator = Coordinator::new(
            pool(),
            &client,
            &tokens,
            &SystemClock,
            &log,
            channels.controls(),
        );
        let mut replies = connect();
        replies.push(session());
        replies.extend(connect());
        let mut patch = session();
        patch.method = "PATCH";
        replies.push(patch);
        replies.push(jit());
        replies.extend([
            Reply::json("GET", "runners/99", 200, runner()),
            Reply::empty("DELETE", "runners/99"),
        ]);
        tokio::join!(fake.serve(replies), async {
            seed(&coordinator).await;
            let mut lease = coordinator
                .reserve(1, channels.stop.subscribe())
                .await
                .unwrap();
            *tokens.0.borrow_mut() = Ok("<replacement-pat-placeholder>".to_owned());
            let token = Secret::new("<unused-runtime-token>".to_owned());
            assert!(lease.request_jit(&request(&token)).await.is_ok());
            lease
                .finish(false, Instant::now() + Duration::from_secs(2))
                .await
                .unwrap();
            for error in [
                "no token",
                "the token store failed: needs_user (1)",
                "the token store failed: access_refused (2)",
                "the token store failed: other (3)",
            ] {
                coordinator.listening();
                let mut lease = coordinator
                    .reserve(1, channels.stop.subscribe())
                    .await
                    .unwrap();
                *tokens.0.borrow_mut() = Err(error.to_owned());
                assert!(
                    lease
                        .request_jit(&request(&token))
                        .await
                        .unwrap_err()
                        .to_string()
                        .contains(error)
                );
                lease
                    .finish(false, Instant::now() + Duration::from_secs(2))
                    .await
                    .unwrap();
            }
        });
        assert_eq!(
            fake.requests
                .borrow()
                .iter()
                .filter(|(_, path)| path.contains("generatejitconfig"))
                .count(),
            1
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn initial_statistics_then_202_admit_a_slot_and_session_close_runs_on_stop() {
        let fake = Fake::new().await;
        let client = fake.client();
        let tokens = Tokens::new();
        let channels = Channels::new(1);
        let log = MemoryLog::default();
        let coordinator = Coordinator::new(
            pool(),
            &client,
            &tokens,
            &SystemClock,
            &log,
            channels.controls(),
        );
        let mut replies = connect();
        replies.push(Reply::json(
            "GET",
            "actions/runners?",
            200,
            json!({"total_count": 0, "runners": []}),
        ));
        replies.push(session());
        let mut idle = Reply::json("GET", "queue", 202, Value::Null);
        idle.signal = Some(channels.drain.clone());
        replies.push(idle);
        replies.push(Reply::empty("DELETE", "sessions/"));
        tokio::join!(fake.serve(replies), async {
            tokio::join!(coordinator.run(), async {
                let mut drain = channels.drain.subscribe();
                drain.wait_for(|draining| *draining).await.unwrap();
                assert_eq!(
                    coordinator.view().statistics.unwrap().total_assigned_jobs,
                    1
                );
                channels.drain.send_replace(false);
                let mut lease = coordinator
                    .reserve(1, channels.stop.subscribe())
                    .await
                    .unwrap();
                assert_eq!(coordinator.view().active_reservations, 1);
                coordinator.stop(Instant::now() + Duration::from_secs(1));
                lease
                    .finish(false, Instant::now() + Duration::from_secs(1))
                    .await
                    .unwrap();
            });
        });
        assert_eq!(coordinator.view().state, ListenerState::Stopped);
        assert!(!coordinator.view().session_cleanup_incomplete);
    }

    fn offer_body(ids: &[u64], assigned: u64) -> Value {
        let jobs: Vec<_> = ids
            .iter()
            .map(|id| json!({"messageType": "JobAvailable", "runnerRequestId": id}))
            .collect();
        let jobs = serde_json::to_string(&jobs).unwrap();
        json!({"messageId": 7, "messageType": "RunnerScaleSetJobMessages", "body": jobs, "statistics": statistics_json(assigned)})
    }

    fn offer_message(ids: &[u64], assigned: u64) -> Reply {
        Reply::json("GET", "queue", 200, offer_body(ids, assigned))
    }

    /// waits, in real time, for `condition`, which some task of the test is expected to make true.
    async fn until(what: &str, mut condition: impl FnMut() -> bool) {
        let waited = timeout(Duration::from_secs(3), async {
            while !condition() {
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
        })
        .await;
        assert!(waited.is_ok(), "timed out waiting for {what}");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_message_at_or_below_the_acknowledged_one_is_not_polled_for_in_a_loop() {
        let clock = ControlledClock::new();
        let service = Service::new(&clock, Duration::from_secs(1200)).await;
        *service.message.borrow_mut() = Some(offer_body(&[11], 0));
        service.stale.set(true);
        let client = service.client();
        let tokens = Tokens::new();
        let channels = Channels::new(1);
        let log = MemoryLog::default();
        let coordinator =
            Coordinator::new(pool(), &client, &tokens, &clock, &log, channels.controls());
        timeout(Duration::from_secs(60), async {
            tokio::select! {
                () = service.serve() => unreachable!(),
                () = async {
                    tokio::join!(coordinator.run(), async {
                        until("the acknowledgement", || service.count("DELETE", "/queue/7") == 1)
                            .await;
                        let before = service.count("GET", "/queue");
                        run_for(&clock, &service, Duration::from_secs(300)).await;
                        let polls = service.count("GET", "/queue") - before;
                        assert!((15..=45).contains(&polls), "{polls} polls in 300 s");
                        channels.stop.send_replace(true);
                    });
                } => {},
            }
        })
        .await
        .unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn an_overflow_message_the_service_redelivers_is_polled_at_a_bounded_rate_until_a_seat_frees()
     {
        let clock = ControlledClock::new();
        let service = Service::new(&clock, Duration::from_secs(1200)).await;
        *service.message.borrow_mut() = Some(offer_body(&[11, 12], 0));
        let client = service.client();
        let tokens = Tokens::new();
        let channels = Channels::new(1);
        let log = MemoryLog::default();
        let coordinator =
            Coordinator::new(pool(), &client, &tokens, &clock, &log, channels.controls());
        timeout(Duration::from_secs(60), async {
            tokio::select! {
                () = service.serve() => unreachable!(),
                () = async {
                    tokio::join!(coordinator.run(), async {
                        until("the first offer's acquisition", || {
                            service.count("POST", "acquirejobs") == 1
                        })
                        .await;
                        let mut lease = coordinator
                            .reserve(1, channels.stop.subscribe())
                            .await
                            .unwrap();
                        let before = service.count("GET", "/queue");
                        run_for(&clock, &service, Duration::from_secs(300)).await;
                        let polls = service.count("GET", "/queue") - before;
                        // one poll per wait of REDELIVERY_WAIT, where an unthrottled loop makes thousands
                        assert!((15..=45).contains(&polls), "{polls} polls in 300 s");
                        assert_eq!(service.count("POST", "acquirejobs"), 1);
                        assert_eq!(service.count("DELETE", "/queue/"), 0);
                        lease
                            .finish(true, Instant::now() + Duration::from_secs(1))
                            .await
                            .unwrap();
                        // the freed seat is used without the clock moving
                        until("the second offer's acquisition and acknowledgement", || {
                            service.count("POST", "acquirejobs") == 2
                                && service.count("DELETE", "/queue/7") == 1
                        })
                        .await;
                        channels.stop.send_replace(true);
                    });
                } => {},
            }
        })
        .await
        .unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn lost_acquire_response_refreshes_statistics_before_ack_even_when_polls_return_202() {
        for replay_acquired in [false, true] {
            let fake = Fake::new().await;
            let client = fake.client();
            let tokens = Tokens::new();
            let channels = Channels::new(2);
            let log = MemoryLog::default();
            let coordinator = Coordinator::new(
                pool(),
                &client,
                &tokens,
                &SystemClock,
                &log,
                channels.controls(),
            );
            let mut replies = connect();
            let mut initial = session();
            initial.body["statistics"] = statistics_json(0);
            replies.push(initial);
            replies.push(offer_message(&[11], 0));
            let mut lost = Reply::json(
                "POST",
                "acquirejobs",
                200,
                json!({"count": 1, "value": [11]}),
            );
            lost.lose_response = true;
            replies.push(lost);
            let mut patch = session();
            patch.method = "PATCH";
            replies.push(patch);
            replies.push(Reply::json(
                "POST",
                "acquirejobs",
                200,
                if replay_acquired {
                    json!({"count": 1, "value": [11]})
                } else {
                    json!({"count": 0, "value": []})
                },
            ));
            replies.push(Reply::empty("DELETE", "queue/7"));
            replies.push(Reply::json("GET", "lastMessageId=7", 202, Value::Null));
            tokio::join!(fake.serve(replies), async {
                seed(&coordinator).await;
                let mut pending = None;
                let mut last = None;
                let mut cleaned = true;
                let mut session_id = coordinator.credentials().unwrap().session.id().to_owned();
                coordinator
                    .cycle(&mut pending, &mut last, &mut session_id, &mut cleaned)
                    .await
                    .unwrap();
                assert!(
                    coordinator
                        .cycle(&mut pending, &mut last, &mut session_id, &mut cleaned)
                        .await
                        .is_err()
                );
                assert_eq!(last, None);
                assert_eq!(coordinator.view().occupied_capacity, 1);
                coordinator
                    .cycle(&mut pending, &mut last, &mut session_id, &mut cleaned)
                    .await
                    .unwrap();
                assert_eq!(last, Some(7));
                let mut lease = coordinator
                    .reserve(1, channels.stop.subscribe())
                    .await
                    .unwrap();
                assert!(
                    coordinator
                        .state
                        .borrow_mut()
                        .demand
                        .reserve(2, 2)
                        .is_none()
                );
                lease
                    .finish(true, Instant::now() + Duration::from_secs(1))
                    .await
                    .unwrap();
                assert!(
                    coordinator
                        .state
                        .borrow_mut()
                        .demand
                        .reserve(1, 2)
                        .is_none()
                );
            });
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn overflow_and_lost_ack_never_advance_progress_or_duplicate_a_reservation() {
        let fake = Fake::new().await;
        let client = fake.client();
        let tokens = Tokens::new();
        let channels = Channels::new(1);
        let log = MemoryLog::default();
        let clock = ControlledClock::new();
        let coordinator =
            Coordinator::new(pool(), &client, &tokens, &clock, &log, channels.controls());
        let mut replies = connect();
        let mut initial = session();
        initial.body["statistics"] = statistics_json(0);
        replies.push(initial);
        replies.push(offer_message(&[11, 12], 0));
        replies.push(Reply::json(
            "POST",
            "acquirejobs",
            200,
            json!({"count": 1, "value": [11]}),
        ));
        replies.push(offer_message(&[11, 12], 1));
        replies.push(Reply::json(
            "POST",
            "acquirejobs",
            200,
            json!({"count": 1, "value": [12]}),
        ));
        let mut lost_ack = Reply::empty("DELETE", "queue/7");
        lost_ack.lose_response = true;
        replies.push(lost_ack);
        replies.push(Reply::empty("DELETE", "queue/7"));
        replies.push(Reply::json("GET", "lastMessageId=7", 202, Value::Null));
        tokio::join!(fake.serve(replies), async {
            seed(&coordinator).await;
            let mut pending = None;
            let mut last = None;
            let mut cleaned = true;
            let mut session_id = coordinator.credentials().unwrap().session.id().to_owned();
            coordinator
                .cycle(&mut pending, &mut last, &mut session_id, &mut cleaned)
                .await
                .unwrap();
            // the overflow waits for a seat, then the poll hands the same message back
            advancing(
                &clock,
                coordinator.cycle(&mut pending, &mut last, &mut session_id, &mut cleaned),
            )
            .await
            .unwrap();
            assert_eq!(last, None);
            let mut first = coordinator
                .reserve(1, channels.stop.subscribe())
                .await
                .unwrap();
            first
                .finish(true, Instant::now() + Duration::from_secs(1))
                .await
                .unwrap();
            assert!(
                coordinator
                    .cycle(&mut pending, &mut last, &mut session_id, &mut cleaned)
                    .await
                    .is_err()
            );
            assert_eq!(last, None);
            let mut second = coordinator
                .reserve(1, channels.stop.subscribe())
                .await
                .unwrap();
            coordinator
                .cycle(&mut pending, &mut last, &mut session_id, &mut cleaned)
                .await
                .unwrap();
            assert_eq!(last, Some(7));
            assert_eq!(coordinator.view().active_reservations, 1);
            second
                .finish(true, Instant::now() + Duration::from_secs(1))
                .await
                .unwrap();
            assert!(
                coordinator
                    .state
                    .borrow_mut()
                    .demand
                    .reserve(1, 1)
                    .is_none()
            );
        });
    }
}
