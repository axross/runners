#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use marshal_cli::runtime::registration::{GITHUB_API, HttpLimits};
use marshal_cli::scaleset::Client;

#[test]
fn the_profile_is_the_one_ci_asked_for() {
    let debug_profile = cfg!(debug_assertions);
    match std::env::var("MARSHAL_EXPECT_RELEASE").as_deref() {
        Ok("1") => assert!(!debug_profile, "this test was built in the debug profile"),
        Ok("0") => assert!(debug_profile, "this test was built in the release profile"),
        _ => {}
    }
}

#[test]
fn release_ignores_overrides_and_debug_accepts_only_loopback() {
    for requested in ["http://127.0.0.1:34567", "https://example.invalid", ""] {
        let client = Client::new(Some(requested), HttpLimits::default());
        if cfg!(debug_assertions) {
            assert_eq!(client.is_ok(), requested == "http://127.0.0.1:34567");
        } else {
            assert_eq!(client.unwrap().base_url(), GITHUB_API);
        }
    }
}

#[cfg(debug_assertions)]
mod contract {
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, SystemTime};

    use marshal_cli::runtime::secret::Secret;
    use marshal_cli::scaleset::{Connection, ErrorKind, JobEventKind, Session};
    use serde_json::{Value, json};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::task::JoinHandle;

    use super::*;

    const PAT: &str = "<pat-placeholder>";
    const REGISTRATION: &str = "<registration-placeholder>";
    const QUEUE: &str = "<queue-placeholder>";
    const JIT: &str = "<jit-placeholder>";
    const SESSION: &str = "01234567-89ab-cdef-0123-456789abcdef";
    const NAME: &str = "example-linux";
    const RUNNER: &str = "example-linux-1-20260102030405006";
    const ROUTE: &str = "/tenant/_apis/runtime/runnerscalesets";

    fn admin() -> String {
        // an unsigned, fake JWT with exp = 4102444800, not a usable credential.
        "fake.eyJleHAiOjQxMDI0NDQ4MDB9.signature".to_owned()
    }

    fn stats(assigned: u64, running: u64, idle: u64) -> Value {
        json!({"totalAvailableJobs": 9, "totalAcquiredJobs": 8, "totalAssignedJobs": assigned, "totalRunningJobs": running, "totalRegisteredRunners": 7, "totalBusyRunners": 6, "totalIdleRunners": idle})
    }

    fn scale(labels: Value) -> Value {
        json!({"id": 23, "name": NAME, "runnerGroupId": 1, "labels": labels})
    }

    fn found() -> Reply {
        Reply::json(json!({"count": 1, "value": [scale(json!([{"name": "EXAMPLE-linux"}]))]}))
    }

    fn session_with(queue_url: &str, assigned: u64) -> Reply {
        Reply::json(
            json!({"sessionId": SESSION, "messageQueueUrl": queue_url, "messageQueueAccessToken": QUEUE, "statistics": stats(assigned, 2, 3)}),
        )
    }

    fn session() -> Reply {
        session_with("{{base}}/queue", 5)
    }

    fn connection_replies() -> Vec<Reply> {
        vec![
            Reply::status(201, json!({"token": REGISTRATION})),
            Reply::json(json!({"url": "{{base}}/tenant/", "token": admin()})),
        ]
    }

    struct Reply {
        status: u16,
        body: String,
        headers: Vec<(&'static str, &'static str)>,
        delay: Duration,
        chunked: bool,
    }

    impl Reply {
        fn json(body: Value) -> Self {
            Self::status(200, body)
        }
        fn status(status: u16, body: Value) -> Self {
            Self {
                status,
                body: body.to_string(),
                headers: Vec::new(),
                delay: Duration::ZERO,
                chunked: false,
            }
        }
        fn empty(status: u16) -> Self {
            Self {
                body: String::new(),
                ..Self::status(status, Value::Null)
            }
        }
    }

    #[derive(Debug, Clone)]
    struct Request {
        method: String,
        path: String,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
    }

    impl Request {
        fn header(&self, name: &str) -> Option<&str> {
            self.headers
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.as_str())
        }
        fn json(&self) -> Value {
            serde_json::from_slice(&self.body).unwrap()
        }
        fn count(&self, name: &str) -> usize {
            self.headers.iter().filter(|(key, _)| key == name).count()
        }
        /// the sorted header names, without the framing the HTTP client adds on its own.
        fn names(&self) -> Vec<&str> {
            let mut names: Vec<_> = self
                .headers
                .iter()
                .map(|(key, _)| key.as_str())
                .filter(|key| !["host", "content-length"].contains(key))
                .collect();
            names.sort_unstable();
            names
        }
    }

    struct Fake {
        base: String,
        requests: Arc<Mutex<Vec<Request>>>,
        task: JoinHandle<()>,
    }

    impl Fake {
        async fn start(replies: Vec<Reply>) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let base = format!("http://{}", listener.local_addr().unwrap());
            let requests = Arc::new(Mutex::new(Vec::new()));
            let (own, recorded) = (base.clone(), requests.clone());
            let task = tokio::spawn(async move {
                for reply in replies {
                    let (mut stream, _) = listener.accept().await.unwrap();
                    let mut bytes = Vec::new();
                    let mut buffer = [0; 4096];
                    let end = loop {
                        let count = stream.read(&mut buffer).await.unwrap();
                        if count == 0 {
                            return;
                        }
                        bytes.extend_from_slice(&buffer[..count]);
                        assert!(bytes.len() < 1024 * 1024);
                        if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                            break end + 4;
                        }
                    };
                    let head = String::from_utf8_lossy(&bytes[..end]);
                    let mut lines = head.split("\r\n");
                    let mut first = lines.next().unwrap().split(' ');
                    let (method, path) = (
                        first.next().unwrap().to_owned(),
                        first.next().unwrap().to_owned(),
                    );
                    let headers: Vec<_> = lines
                        .filter_map(|line| line.split_once(':'))
                        .map(|(key, value)| (key.to_ascii_lowercase(), value.trim().to_owned()))
                        .collect();
                    let length = headers
                        .iter()
                        .find(|(key, _)| key == "content-length")
                        .map_or(0, |(_, value)| value.parse::<usize>().unwrap());
                    let mut body = bytes[end..].to_vec();
                    while body.len() < length {
                        let count = stream.read(&mut buffer).await.unwrap();
                        if count == 0 {
                            return;
                        }
                        body.extend_from_slice(&buffer[..count]);
                    }
                    recorded.lock().unwrap().push(Request {
                        method,
                        path,
                        headers,
                        body,
                    });
                    tokio::time::sleep(reply.delay).await;
                    let body = reply.body.replace("{{base}}", &own);
                    let framing = if reply.chunked {
                        "Transfer-Encoding: chunked\r\n".to_owned()
                    } else {
                        format!("Content-Length: {}\r\n", body.len())
                    };
                    let mut head = format!(
                        "HTTP/1.1 {} Reply\r\n{framing}Connection: close\r\n",
                        reply.status
                    );
                    for (key, value) in reply.headers {
                        head.push_str(&format!("{key}: {value}\r\n"));
                    }
                    head.push_str("\r\n");
                    let body = if reply.chunked {
                        format!("{:x}\r\n{body}\r\n0\r\n\r\n", body.len())
                    } else {
                        body
                    };
                    let _ = stream.write_all(head.as_bytes()).await;
                    let _ = stream.write_all(body.as_bytes()).await;
                }
            });
            Self {
                base,
                requests,
                task,
            }
        }

        fn client(&self) -> Client {
            Client::new(Some(&self.base), HttpLimits::default())
                .unwrap()
                .with_exchange_retry_delay(Duration::from_millis(1))
        }
        fn requests(&self) -> Vec<Request> {
            self.requests.lock().unwrap().clone()
        }
        async fn connection(&self, client: &Client) -> Connection {
            client
                .connect(
                    "example-owner",
                    "example-repo",
                    &Secret::new(PAT.to_owned()),
                )
                .await
                .unwrap()
        }
    }

    impl Drop for Fake {
        fn drop(&mut self) {
            self.task.abort();
        }
    }

    async fn listening(extra: Vec<Reply>) -> (Fake, Client, Connection, Session) {
        let mut replies = connection_replies();
        replies.extend([found(), session()]);
        replies.extend(extra);
        let fake = Fake::start(replies).await;
        let client = fake.client();
        let connection = fake.connection(&client).await;
        let set = client.find_or_create(&connection, NAME).await.unwrap();
        let session = client
            .create_session(&connection, &set, "example-host")
            .await
            .unwrap();
        (fake, client, connection, session)
    }

    #[tokio::test]
    async fn protocol_routes_payloads_statistics_progress_and_ownership_are_asymmetric() {
        let mut replies = connection_replies();
        replies.extend([
            Reply::json(json!({"count": 0, "value": []})),
            Reply::json(scale(json!([{"name": NAME}]))),
            session(),
            Reply::empty(202),
            Reply::json(json!({"messageId": 71, "messageType": "RunnerScaleSetJobMessages", "statistics": stats(4, 1, 2), "body": json!([
                {"messageType": "JobAvailable", "runnerRequestId": 501, "acquireJobUrl": "https://example.invalid/not-used"},
                {"messageType": "JobAssigned", "runnerRequestId": 502},
                {"messageType": "JobStarted", "runnerRequestId": 503},
                {"messageType": "JobCompleted", "runnerRequestId": 504}
            ]).to_string()})),
            Reply::json(json!({"count": 1, "value": [501]})),
            Reply::empty(204),
            Reply::json(json!({"runner": {"id": 901, "name": RUNNER, "runnerScaleSetId": 23}, "encodedJITConfig": JIT})),
            Reply::json(json!({"total_count": 2, "runners": [{"id": 900, "name": format!("{RUNNER}-other"), "status": "offline", "busy": false}]})),
            Reply::json(json!({"total_count": 2, "runners": [{"id": 901, "name": RUNNER, "status": "online", "busy": true}]})),
            Reply::json(json!({"id": 901, "name": RUNNER, "status": "offline", "busy": false})),
            Reply::empty(204),
            Reply::empty(204),
        ]);
        let fake = Fake::start(replies).await;
        let client = fake.client();
        let connection = fake.connection(&client).await;
        assert!(!connection.needs_renewal(&Secret::new(PAT.to_owned()), SystemTime::now()));
        let set = client.find_or_create(&connection, NAME).await.unwrap();
        assert_eq!(set.id(), 23);
        let session = client
            .create_session(&connection, &set, "example-host")
            .await
            .unwrap();
        assert_eq!(session.statistics.total_assigned_jobs, 5);
        assert_eq!(session.statistics.total_running_jobs, 2);
        assert_eq!(session.statistics.total_idle_runners, 3);
        assert!(client.poll(&session, None, 3).await.unwrap().is_none());
        assert_eq!(session.statistics.total_assigned_jobs, 5);
        let message = client.poll(&session, Some(17), 3).await.unwrap().unwrap();
        assert_eq!(message.id, 71);
        assert_eq!(message.statistics.total_assigned_jobs, 4);
        assert_eq!(
            message
                .jobs
                .iter()
                .map(|event| (event.kind, event.request_id))
                .collect::<Vec<_>>(),
            vec![
                (JobEventKind::Available, 501),
                (JobEventKind::Assigned, 502),
                (JobEventKind::Started, 503),
                (JobEventKind::Completed, 504)
            ]
        );
        assert_eq!(
            client
                .acquire_jobs(&connection, &session, &[501, 505])
                .await
                .unwrap(),
            vec![501]
        );
        client.acknowledge(&session, message.id).await.unwrap();
        let jit = client
            .request_jit(
                &connection,
                &set,
                RUNNER,
                &Secret::new(PAT.to_owned()),
                SystemTime::now(),
            )
            .await
            .unwrap();
        assert_eq!(
            (jit.id, jit.name.as_str(), jit.config.expose()),
            (901, RUNNER, JIT)
        );
        assert!(!format!("{connection:?} {session:?} {jit:?}").contains(JIT));
        assert_eq!(
            client
                .find_runner(
                    "example-owner",
                    "example-repo",
                    RUNNER,
                    &Secret::new(PAT.to_owned())
                )
                .await
                .unwrap()
                .unwrap()
                .id,
            901
        );
        client
            .delete_runner(
                "example-owner",
                "example-repo",
                901,
                RUNNER,
                &Secret::new(PAT.to_owned()),
            )
            .await
            .unwrap();
        client.close_session(&connection, &session).await.unwrap();
        let requests = fake.requests();
        assert_eq!(requests.len(), 15);
        assert_eq!(
            requests[0].path,
            "/repos/example-owner/example-repo/actions/runners/registration-token"
        );
        assert_eq!(
            requests[0].header("authorization"),
            Some(format!("Bearer {PAT}").as_str())
        );
        assert_eq!(
            requests[0].header("x-github-api-version"),
            Some("2022-11-28")
        );
        assert_eq!(requests[1].path, "/actions/runner-registration");
        assert_eq!(
            requests[1].header("authorization"),
            Some(format!("RemoteAuth {REGISTRATION}").as_str())
        );
        assert_eq!(
            requests[1].json(),
            json!({"runner_event": "register", "url": "https://github.com/example-owner/example-repo"})
        );
        assert_eq!(
            requests[2].path,
            format!("{ROUTE}?api-version=6.0-preview&runnerGroupId=1&name={NAME}")
        );
        assert_eq!(
            requests[3].json(),
            json!({"name": NAME, "runnerGroupId": 1, "labels": [{"name": NAME, "type": "System"}], "RunnerSetting": {"disableUpdate": true}})
        );
        assert_eq!(requests[4].json(), json!({"ownerName": "example-host"}));
        assert_eq!(requests[5].path, "/queue");
        assert_eq!(requests[6].path, "/queue?lastMessageId=17");
        assert_eq!(requests[6].header("x-scalesetmaxcapacity"), Some("3"));
        assert_eq!(
            requests[6].header("accept"),
            Some("application/json; api-version=6.0-preview")
        );
        assert_eq!(
            requests[7].path,
            format!("{ROUTE}/23/acquirejobs?api-version=6.0-preview")
        );
        assert_eq!(requests[7].json(), json!([501, 505]));
        assert_eq!(
            requests[7].header("authorization"),
            Some(format!("Bearer {QUEUE}").as_str())
        );
        assert_eq!(
            (requests[8].method.as_str(), requests[8].path.as_str()),
            ("DELETE", "/queue/71")
        );
        assert_eq!(
            requests[9].json(),
            json!({"name": RUNNER, "workFolder": "_work"})
        );
        assert_eq!(
            requests[9].path,
            format!("{ROUTE}/23/generatejitconfig?api-version=6.0-preview")
        );
        assert_eq!(
            requests[9].header("authorization"),
            Some(format!("Bearer {}", admin()).as_str())
        );
        assert!(requests[11].path.ends_with("per_page=100&page=2"));
        assert_eq!(requests[12].method, "GET");
        assert_eq!(requests[13].method, "DELETE");
        assert_eq!(
            requests[14].path,
            format!("{ROUTE}/23/sessions/{SESSION}?api-version=6.0-preview")
        );
    }

    #[tokio::test]
    async fn processing_and_ack_failures_do_not_advance_poll_progress_or_acknowledge_implicitly() {
        let batch = || {
            Reply::json(
                json!({"messageId": 71, "messageType": "RunnerScaleSetJobMessages", "statistics": stats(3, 1, 2), "body": "[{\"messageType\":\"JobAvailable\",\"runnerRequestId\":501}]"}),
            )
        };
        let (fake, client, connection, session) = listening(vec![
            batch(),
            Reply::empty(503),
            batch(),
            Reply::json(json!({"count": 1, "value": [501]})),
            Reply::empty(503),
            batch(),
            Reply::json(json!({"count": 0, "value": []})),
            Reply::empty(204),
            Reply::empty(202),
        ])
        .await;
        assert_eq!(
            client
                .poll(&session, Some(17), 1)
                .await
                .unwrap()
                .unwrap()
                .id,
            71
        );
        assert_eq!(fake.requests().len(), 5);
        assert_eq!(
            client
                .acquire_jobs(&connection, &session, &[501])
                .await
                .unwrap_err()
                .kind,
            ErrorKind::Service
        );
        let again = client.poll(&session, Some(17), 1).await.unwrap().unwrap();
        assert_eq!(again.id, 71);
        assert_eq!(
            client
                .acquire_jobs(&connection, &session, &[501])
                .await
                .unwrap(),
            vec![501]
        );
        assert_eq!(
            client.acknowledge(&session, 71).await.unwrap_err().kind,
            ErrorKind::Service
        );
        assert_eq!(
            client
                .poll(&session, Some(17), 1)
                .await
                .unwrap()
                .unwrap()
                .id,
            71
        );
        assert!(
            client
                .acquire_jobs(&connection, &session, &[501])
                .await
                .unwrap()
                .is_empty()
        );
        client.acknowledge(&session, 71).await.unwrap();
        assert!(client.poll(&session, Some(71), 1).await.unwrap().is_none());
        let polls: Vec<_> = fake
            .requests()
            .into_iter()
            .filter(|request| request.method == "GET" && request.path.starts_with("/queue"))
            .map(|request| request.path)
            .collect();
        assert_eq!(
            polls,
            vec![
                "/queue?lastMessageId=17",
                "/queue?lastMessageId=17",
                "/queue?lastMessageId=17",
                "/queue?lastMessageId=71"
            ]
        );
    }

    #[tokio::test]
    async fn queue_query_is_preserved_but_progress_parameter_is_owned_by_the_listener() {
        let mut replies = connection_replies();
        replies.extend([
            found(),
            session_with("{{base}}/queue?channel=example&lastMessageId=999", 13),
            Reply::empty(202),
            Reply::empty(204),
        ]);
        let fake = Fake::start(replies).await;
        let client = fake.client();
        let connection = fake.connection(&client).await;
        let set = client.find_or_create(&connection, NAME).await.unwrap();
        let session = client
            .create_session(&connection, &set, "example-host")
            .await
            .unwrap();
        assert_eq!(session.statistics.total_assigned_jobs, 13);
        assert!(client.poll(&session, Some(17), 1).await.unwrap().is_none());
        client.acknowledge(&session, 71).await.unwrap();
        let requests = fake.requests();
        assert_eq!(requests[4].path, "/queue?channel=example&lastMessageId=17");
        assert_eq!(
            requests[5].path,
            "/queue/71?channel=example&lastMessageId=999"
        );
    }

    #[tokio::test]
    async fn missing_initial_statistics_prevent_a_session_from_becoming_usable() {
        let mut replies = connection_replies();
        replies.extend([found(), Reply::json(json!({"sessionId": SESSION, "messageQueueUrl": "{{base}}/queue", "messageQueueAccessToken": QUEUE}))]);
        let fake = Fake::start(replies).await;
        let client = fake.client();
        let connection = fake.connection(&client).await;
        let set = client.find_or_create(&connection, NAME).await.unwrap();
        assert_eq!(
            client
                .create_session(&connection, &set, "example-host")
                .await
                .unwrap_err()
                .kind,
            ErrorKind::Protocol
        );
        assert_eq!(fake.requests().len(), 4);
    }

    #[tokio::test]
    async fn lookup_and_creation_reject_every_incompatible_label_set_without_side_effects() {
        for labels in [
            json!([]),
            json!([{"name": NAME}, {"name": "self-hosted"}]),
            json!([{"name": "other"}]),
        ] {
            for create in [false, true] {
                let mut replies = connection_replies();
                if create {
                    replies.extend([
                        Reply::json(json!({"count": 0, "value": []})),
                        Reply::json(scale(labels.clone())),
                    ]);
                } else {
                    replies.push(Reply::json(
                        json!({"count": 1, "value": [scale(labels.clone())]}),
                    ));
                }
                let fake = Fake::start(replies).await;
                let client = fake.client();
                let connection = fake.connection(&client).await;
                let error = client.find_or_create(&connection, NAME).await.unwrap_err();
                assert_eq!(error.kind, ErrorKind::LabelMismatch);
                assert_eq!(error.status, None);
                assert_eq!(error.to_string(), "scale-set label mismatch");
                assert_eq!(fake.requests().len(), if create { 4 } else { 3 });
            }
        }
    }

    #[tokio::test]
    async fn statuses_and_rate_limit_signals_are_typed_and_never_echo_any_response_body() {
        let rows = [
            (401, vec![], "permission", ErrorKind::Rejected),
            (403, vec![], "permission", ErrorKind::Rejected),
            (404, vec![], "missing", ErrorKind::NotFound),
            (409, vec![], "conflict", ErrorKind::Service),
            (429, vec![], "wait", ErrorKind::RateLimited),
            (
                403,
                vec![("Retry-After", "30"), ("x-ratelimit-remaining", "4000")],
                "permission",
                ErrorKind::RateLimited,
            ),
            (
                403,
                vec![
                    ("x-ratelimit-remaining", "0"),
                    ("x-ratelimit-reset", "4102444800"),
                ],
                "quota",
                ErrorKind::RateLimited,
            ),
            (403, vec![], "secondary rate limit", ErrorKind::RateLimited),
            (500, vec![], "server", ErrorKind::Service),
        ];
        for (status, headers, text, kind) in rows {
            let body = json!({"message": format!("{text} {PAT} {REGISTRATION} {} {QUEUE} {JIT}", admin())});
            let fake = Fake::start(vec![Reply {
                headers,
                ..Reply::status(status, body)
            }])
            .await;
            let client = fake.client();
            let error = client
                .connect(
                    "example-owner",
                    "example-repo",
                    &Secret::new(PAT.to_owned()),
                )
                .await
                .unwrap_err();
            assert_eq!(error.kind, kind, "{status} {text}");
            assert_eq!(error.status, Some(status));
            if text == "permission" && kind == ErrorKind::RateLimited {
                assert_eq!(error.retry_after, Some(Duration::from_secs(30)));
            }
            let display = format!("{error} {error:?}");
            for secret in [PAT, REGISTRATION, &admin(), QUEUE, JIT] {
                assert!(!display.contains(secret));
            }
            assert_eq!(fake.requests().len(), 1);
        }
    }

    #[tokio::test]
    async fn a_404_is_returned_for_the_listener_to_recreate_once_and_a_409_opens_no_runner() {
        let mut replies = connection_replies();
        replies.extend([
            found(),
            Reply::empty(404),
            found(),
            session(),
            Reply::empty(409),
        ]);
        let fake = Fake::start(replies).await;
        let client = fake.client();
        let connection = fake.connection(&client).await;
        let set = client.find_or_create(&connection, NAME).await.unwrap();
        assert_eq!(
            client
                .create_session(&connection, &set, "example-host")
                .await
                .unwrap_err()
                .kind,
            ErrorKind::NotFound
        );
        let set = client.find_or_create(&connection, NAME).await.unwrap();
        let _session = client
            .create_session(&connection, &set, "example-host")
            .await
            .unwrap();
        assert_eq!(
            client
                .create_session(&connection, &set, "example-host")
                .await
                .unwrap_err()
                .kind,
            ErrorKind::Conflict
        );
        assert_eq!(fake.requests().len(), 7);
    }

    #[tokio::test]
    async fn replacement_requires_connection_renewal_even_when_old_credentials_are_valid() {
        let mut replies = connection_replies();
        replies.push(found());
        replies.extend(connection_replies());
        replies.push(Reply::json(json!({"runner": {"id": 88, "name": RUNNER, "runnerScaleSetId": 23}, "encodedJITConfig": JIT})));
        let fake = Fake::start(replies).await;
        let client = fake.client();
        let connection = fake.connection(&client).await;
        let set = client.find_or_create(&connection, NAME).await.unwrap();
        let replacement = Secret::new("<replacement-pat-placeholder>".to_owned());
        assert!(connection.needs_renewal(&replacement, SystemTime::now()));
        assert!(connection.needs_renewal(&Secret::new(PAT.to_owned()), connection.expires_at()));
        assert_eq!(
            client
                .request_jit(&connection, &set, RUNNER, &replacement, SystemTime::now())
                .await
                .unwrap_err()
                .kind,
            ErrorKind::ConnectionRenewalRequired
        );
        assert_eq!(fake.requests().len(), 3);
        let renewed = client
            .connect("example-owner", "example-repo", &replacement)
            .await
            .unwrap();
        assert_eq!(
            client
                .request_jit(&renewed, &set, RUNNER, &replacement, SystemTime::now())
                .await
                .unwrap()
                .id,
            88
        );
        assert_eq!(
            fake.requests()[3].header("authorization"),
            Some("Bearer <replacement-pat-placeholder>")
        );
    }

    #[tokio::test]
    async fn session_refresh_returns_new_initial_statistics_and_queue_credential() {
        let refreshed = Reply::json(
            json!({"sessionId": SESSION, "messageQueueUrl": "{{base}}/new-queue", "messageQueueAccessToken": "<refreshed-queue-placeholder>", "statistics": stats(11, 4, 2)}),
        );
        let (fake, client, connection, session) =
            listening(vec![Reply::empty(401), refreshed, Reply::empty(202)]).await;
        assert_eq!(
            client.poll(&session, Some(71), 1).await.unwrap_err().kind,
            ErrorKind::Rejected
        );
        let session = client.refresh_session(&connection, &session).await.unwrap();
        assert_eq!(session.statistics.total_assigned_jobs, 11);
        assert!(client.poll(&session, Some(71), 1).await.unwrap().is_none());
        let requests = fake.requests();
        assert_eq!(requests[5].method, "PATCH");
        assert_eq!(requests[6].path, "/new-queue?lastMessageId=71");
        assert_eq!(
            requests[6].header("authorization"),
            Some("Bearer <refreshed-queue-placeholder>")
        );
    }

    #[tokio::test]
    async fn malformed_credentials_statistics_messages_and_jit_fail_without_echo() {
        for token in [
            "",
            "not-a-jwt",
            "fake.invalid.signature",
            "fake.eyJleHAiOiJub3Qtbm9tYmVyIn0.signature",
        ] {
            let fake = Fake::start(vec![
                Reply::status(201, json!({"token": REGISTRATION})),
                Reply::json(json!({"url": "{{base}}/tenant", "token": token})),
            ])
            .await;
            assert_eq!(
                fake.client()
                    .connect(
                        "example-owner",
                        "example-repo",
                        &Secret::new(PAT.to_owned())
                    )
                    .await
                    .unwrap_err()
                    .kind,
                ErrorKind::Protocol
            );
        }
        for body in [
            json!({"sessionId": SESSION, "messageQueueUrl": "{{base}}/queue", "messageQueueAccessToken": QUEUE}),
            json!({"messageId": 71, "messageType": JIT, "statistics": stats(1, 2, 3), "body": ""}),
            json!({"messageId": 71, "messageType": "RunnerScaleSetJobMessages", "statistics": stats(1, 2, 3), "body": JIT}),
        ] {
            let (_fake, client, _connection, session) = listening(vec![Reply::json(body)]).await;
            let error = client.poll(&session, None, 1).await.unwrap_err();
            assert_eq!(error.kind, ErrorKind::Protocol);
            assert!(!format!("{error:?} {error}").contains(JIT));
        }
        for runner in [
            json!({"id": 88, "name": "other", "runnerScaleSetId": 23}),
            json!({"id": 88, "name": RUNNER, "runnerScaleSetId": 24}),
        ] {
            let mut replies = connection_replies();
            replies.extend([
                found(),
                Reply::json(json!({"runner": runner, "encodedJITConfig": JIT})),
            ]);
            let fake = Fake::start(replies).await;
            let client = fake.client();
            let connection = fake.connection(&client).await;
            let set = client.find_or_create(&connection, NAME).await.unwrap();
            assert_eq!(
                client
                    .request_jit(
                        &connection,
                        &set,
                        RUNNER,
                        &Secret::new(PAT.to_owned()),
                        SystemTime::now(),
                    )
                    .await
                    .unwrap_err()
                    .kind,
                ErrorKind::Protocol
            );
        }
    }

    #[tokio::test]
    async fn refused_service_and_queue_urls_never_receive_a_request() {
        for url in [
            "https://xactions.githubusercontent.com/tenant",
            "https://actions.githubusercontent.com.example.invalid/tenant",
            "http://actions.githubusercontent.com/tenant",
            "https://actions.githubusercontent.com@evil.invalid/tenant",
            "https://example.invalid/tenant",
            "https://user:password@actions.githubusercontent.com/tenant",
            "https://actions.githubusercontent.com/tenant#fragment",
        ] {
            let mut replies = connection_replies();
            replies[1] = Reply::json(json!({"url": url, "token": admin()}));
            let fake = Fake::start(replies).await;
            assert_eq!(
                fake.client()
                    .connect(
                        "example-owner",
                        "example-repo",
                        &Secret::new(PAT.to_owned())
                    )
                    .await
                    .unwrap_err()
                    .kind,
                ErrorKind::RefusedHost
            );
            assert_eq!(fake.requests().len(), 2);
            let mut replies = connection_replies();
            replies.extend([found(), session_with(url, 1)]);
            let fake = Fake::start(replies).await;
            let client = fake.client();
            let connection = fake.connection(&client).await;
            let set = client.find_or_create(&connection, NAME).await.unwrap();
            assert_eq!(
                client
                    .create_session(&connection, &set, "example-host")
                    .await
                    .unwrap_err()
                    .kind,
                ErrorKind::RefusedHost
            );
            assert_eq!(fake.requests().len(), 4);
        }
        for url in [
            "https://actions.githubusercontent.com/tenant",
            "https://broker.actions.githubusercontent.com/tenant",
        ] {
            let fake = Fake::start(vec![
                Reply::status(201, json!({"token": REGISTRATION})),
                Reply::json(json!({"url": url, "token": admin()})),
            ])
            .await;
            assert!(
                fake.client()
                    .connect(
                        "example-owner",
                        "example-repo",
                        &Secret::new(PAT.to_owned())
                    )
                    .await
                    .is_ok()
            );
        }
    }

    #[tokio::test]
    async fn redirects_declared_and_streamed_caps_and_network_timeouts_are_bounded() {
        for reply in [
            Reply {
                headers: vec![("Location", "https://example.invalid/")],
                ..Reply::empty(307)
            },
            Reply {
                body: "x".repeat(1025),
                ..Reply::empty(201)
            },
            Reply {
                body: "x".repeat(1025),
                chunked: true,
                ..Reply::empty(201)
            },
        ] {
            let expected = if reply.status == 307 {
                ErrorKind::Redirect
            } else {
                ErrorKind::ResponseTooLarge
            };
            let fake = Fake::start(vec![reply]).await;
            let client = Client::new(
                Some(&fake.base),
                HttpLimits {
                    max_response_bytes: 1024,
                    ..HttpLimits::default()
                },
            )
            .unwrap();
            assert_eq!(
                client
                    .connect(
                        "example-owner",
                        "example-repo",
                        &Secret::new(PAT.to_owned())
                    )
                    .await
                    .unwrap_err()
                    .kind,
                expected
            );
            assert_eq!(fake.requests().len(), 1);
        }
        let fake = Fake::start(vec![Reply {
            delay: Duration::from_secs(1),
            ..Reply::empty(201)
        }])
        .await;
        let client = Client::new(
            Some(&fake.base),
            HttpLimits {
                total_timeout: Duration::from_millis(50),
                ..HttpLimits::default()
            },
        )
        .unwrap();
        let error = client
            .connect(
                "example-owner",
                "example-repo",
                &Secret::new(PAT.to_owned()),
            )
            .await
            .unwrap_err();
        assert!(matches!(error.kind, ErrorKind::Unreachable(_)));
    }

    #[tokio::test]
    async fn polling_has_its_own_timeout_instead_of_the_short_rest_timeout() {
        let (fake, _client, _connection, session) = listening(vec![Reply {
            delay: Duration::from_millis(150),
            ..Reply::empty(202)
        }])
        .await;
        let client = Client::new(
            Some(&fake.base),
            HttpLimits {
                total_timeout: Duration::from_millis(40),
                ..HttpLimits::default()
            },
        )
        .unwrap();
        assert!(client.poll(&session, None, 1).await.unwrap().is_none());
        assert!(marshal_cli::scaleset::POLL_TIMEOUT > Duration::from_secs(50));
    }

    #[tokio::test]
    async fn session_close_settles_404_but_not_service_failure_and_keeps_the_exact_route() {
        let (fake, client, connection, session) = listening(vec![
            Reply::empty(404),
            Reply::empty(503),
            Reply::empty(204),
        ])
        .await;
        client.close_session(&connection, &session).await.unwrap();
        assert_eq!(
            client
                .close_session(&connection, &session)
                .await
                .unwrap_err()
                .kind,
            ErrorKind::Service
        );
        client.close_session(&connection, &session).await.unwrap();
        for request in fake.requests().iter().skip(4) {
            assert_eq!(request.method, "DELETE");
            assert_eq!(
                request.path,
                format!("{ROUTE}/23/sessions/{SESSION}?api-version=6.0-preview")
            );
        }
        assert_eq!(fake.requests().len(), 7);
    }

    #[tokio::test]
    async fn cleanup_requires_exact_id_and_name_and_absence_is_settled_but_server_failure_is_not() {
        let fake = Fake::start(vec![
            Reply::json(json!({"id": 44, "name": "other", "status": "offline", "busy": false})),
            Reply::empty(404),
            Reply::json(json!({"id": 44, "name": RUNNER, "status": "offline", "busy": false})),
            Reply::empty(404),
            Reply::json(json!({"id": 44, "name": RUNNER, "status": "offline", "busy": false})),
            Reply::empty(503),
        ])
        .await;
        let client = fake.client();
        let pat = Secret::new(PAT.to_owned());
        assert_eq!(
            client
                .delete_runner("example-owner", "example-repo", 44, RUNNER, &pat)
                .await
                .unwrap_err()
                .kind,
            ErrorKind::OwnershipMismatch
        );
        assert_eq!(fake.requests().len(), 1);
        client
            .delete_runner("example-owner", "example-repo", 44, RUNNER, &pat)
            .await
            .unwrap();
        client
            .delete_runner("example-owner", "example-repo", 44, RUNNER, &pat)
            .await
            .unwrap();
        assert_eq!(
            client
                .delete_runner("example-owner", "example-repo", 44, RUNNER, &pat)
                .await
                .unwrap_err()
                .kind,
            ErrorKind::Service
        );
        assert_eq!(fake.requests().len(), 6);
    }

    /// reply headers are `'static`, so a computed value is leaked for the life of the test run.
    fn leaked(text: String) -> &'static str {
        Box::leak(text.into_boxed_str())
    }

    fn pat() -> Secret {
        Secret::new(PAT.to_owned())
    }

    async fn connect_error(fake: &Fake) -> marshal_cli::scaleset::Error {
        fake.client()
            .connect("example-owner", "example-repo", &pat())
            .await
            .unwrap_err()
    }

    #[tokio::test]
    async fn the_registration_exchange_alone_retries_a_401_or_403_then_succeeds() {
        for status in [401, 403] {
            let fake = Fake::start(vec![
                Reply::status(201, json!({"token": REGISTRATION})),
                Reply::empty(status),
                Reply::empty(status),
                Reply::json(json!({"url": "{{base}}/tenant/", "token": admin()})),
            ])
            .await;
            fake.connection(&fake.client()).await;
            let requests = fake.requests();
            assert_eq!(requests.len(), 4);
            for request in &requests[1..] {
                assert_eq!(request.path, "/actions/runner-registration");
                assert_eq!(
                    request.header("authorization"),
                    Some(format!("RemoteAuth {REGISTRATION}").as_str())
                );
            }
        }
    }

    #[tokio::test]
    async fn a_persistent_exchange_rejection_stays_rejected_after_bounded_waits() {
        let fake = Fake::start(vec![
            Reply::status(201, json!({"token": REGISTRATION})),
            Reply::empty(401),
            Reply::empty(401),
            Reply::empty(401),
            Reply::empty(200),
        ])
        .await;
        let client = Client::new(Some(&fake.base), HttpLimits::default())
            .unwrap()
            .with_exchange_retry_delay(Duration::from_millis(60));
        let started = std::time::Instant::now();
        let error = client
            .connect("example-owner", "example-repo", &pat())
            .await
            .unwrap_err();
        assert_eq!(error.kind, ErrorKind::Rejected);
        assert_eq!(error.status, Some(401));
        assert!(started.elapsed() >= Duration::from_millis(120));
        assert_eq!(fake.requests().len(), 4);
    }

    #[tokio::test]
    async fn no_other_call_and_no_rate_limit_answer_is_retried_as_a_rejection() {
        let fake = Fake::start(vec![Reply::empty(401), Reply::empty(201)]).await;
        assert_eq!(connect_error(&fake).await.kind, ErrorKind::Rejected);
        assert_eq!(fake.requests().len(), 1);
        let fake = Fake::start(vec![
            Reply::status(201, json!({"token": REGISTRATION})),
            Reply::empty(429),
            Reply::empty(200),
        ])
        .await;
        assert_eq!(connect_error(&fake).await.kind, ErrorKind::RateLimited);
        assert_eq!(fake.requests().len(), 2);
        let mut replies = connection_replies();
        replies.extend([found(), Reply::empty(403), Reply::empty(403)]);
        let fake = Fake::start(replies).await;
        let client = fake.client();
        let connection = fake.connection(&client).await;
        let set = client.find_or_create(&connection, NAME).await.unwrap();
        let error = client
            .create_session(&connection, &set, "example-host")
            .await
            .unwrap_err();
        assert_eq!(error.kind, ErrorKind::Rejected);
        assert_eq!(fake.requests().len(), 4);
    }

    #[tokio::test]
    async fn a_409_names_another_listener_only_for_session_creation() {
        let mut replies = connection_replies();
        replies.extend([
            Reply::json(json!({"count": 0, "value": []})),
            Reply::empty(409),
            found(),
            Reply::empty(409),
            Reply::empty(409),
        ]);
        let fake = Fake::start(replies).await;
        let client = fake.client();
        let connection = fake.connection(&client).await;
        let error = client.find_or_create(&connection, NAME).await.unwrap_err();
        assert_eq!((error.kind, error.status), (ErrorKind::Service, Some(409)));
        let set = client.find_or_create(&connection, NAME).await.unwrap();
        let error = client
            .request_jit(&connection, &set, RUNNER, &pat(), SystemTime::now())
            .await
            .unwrap_err();
        assert_eq!(
            (&error.kind, error.status),
            (&ErrorKind::Service, Some(409))
        );
        assert!(!error.to_string().contains("another listener"));
        let error = client
            .create_session(&connection, &set, "example-host")
            .await
            .unwrap_err();
        assert_eq!(
            (&error.kind, error.status),
            (&ErrorKind::Conflict, Some(409))
        );
        assert_eq!(error.to_string(), "another listener (409)");
    }

    #[tokio::test]
    async fn each_request_sends_one_accept_and_a_content_type_only_with_a_body() {
        let (fake, client, connection, session) = listening(vec![Reply::empty(202)]).await;
        client.poll(&session, None, 1).await.unwrap();
        let requests = fake.requests();
        let rest = &requests[0];
        assert_eq!(
            rest.names(),
            [
                "accept",
                "authorization",
                "user-agent",
                "x-github-api-version"
            ]
        );
        assert_eq!(rest.header("accept"), Some("application/vnd.github+json"));
        let exchange = &requests[1];
        assert_eq!(
            exchange.names(),
            ["accept", "authorization", "content-type", "user-agent"]
        );
        assert_eq!(exchange.header("accept"), Some("*/*"));
        let lookup = &requests[2];
        assert_eq!(lookup.names(), ["accept", "authorization", "user-agent"]);
        assert_eq!(
            lookup.header("accept"),
            Some("application/json; api-version=6.0-preview")
        );
        let created = &requests[3];
        assert_eq!(
            created.names(),
            ["accept", "authorization", "content-type", "user-agent"]
        );
        assert_eq!(created.header("content-type"), Some("application/json"));
        let poll = &requests[4];
        assert_eq!(
            poll.names(),
            [
                "accept",
                "authorization",
                "user-agent",
                "x-scalesetmaxcapacity"
            ]
        );
        for request in &requests {
            assert_eq!(request.count("accept"), 1, "{}", request.path);
        }
        drop((connection, session));
    }

    #[tokio::test]
    async fn a_rate_reset_far_in_the_future_is_held_to_one_window_and_retry_after_to_an_hour() {
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let soon = leaked((now + 60).to_string());
        for (reset, expected) in [("4102444800", now + 3600), (soon, now + 60)] {
            let fake = Fake::start(vec![Reply {
                headers: vec![("x-ratelimit-remaining", "0"), ("x-ratelimit-reset", reset)],
                ..Reply::empty(403)
            }])
            .await;
            let error = connect_error(&fake).await;
            assert_eq!(error.kind, ErrorKind::RateLimited);
            let held = error.rate_reset.unwrap();
            assert!(held.abs_diff(expected) <= 5, "{held} vs {expected}");
        }
        let fake = Fake::start(vec![Reply {
            headers: vec![("retry-after", "99999")],
            ..Reply::empty(429)
        }])
        .await;
        assert_eq!(
            connect_error(&fake).await.retry_after,
            Some(Duration::from_secs(3600))
        );
    }

    #[tokio::test]
    async fn an_oversized_error_body_is_discarded_and_the_status_still_decides_the_kind() {
        for chunked in [false, true] {
            for (status, headers, kind) in [
                (401, vec![], ErrorKind::Rejected),
                (403, vec![], ErrorKind::Rejected),
                (403, vec![("retry-after", "7")], ErrorKind::RateLimited),
                (429, vec![], ErrorKind::RateLimited),
                (500, vec![], ErrorKind::Service),
            ] {
                let fake = Fake::start(vec![Reply {
                    body: "x".repeat(1025),
                    headers,
                    chunked,
                    ..Reply::empty(status)
                }])
                .await;
                let client = Client::new(
                    Some(&fake.base),
                    HttpLimits {
                        max_response_bytes: 1024,
                        ..HttpLimits::default()
                    },
                )
                .unwrap();
                let error = client
                    .connect("example-owner", "example-repo", &pat())
                    .await
                    .unwrap_err();
                assert_eq!((error.kind, error.status), (kind, Some(status)));
            }
        }
    }

    #[tokio::test]
    async fn a_refused_connection_reports_a_cause_without_the_url() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        drop(listener);
        let client = Client::new(Some(&base), HttpLimits::default()).unwrap();
        let error = client
            .connect("example-owner", "example-repo", &pat())
            .await
            .unwrap_err();
        let ErrorKind::Unreachable(cause) = &error.kind else {
            panic!("expected an unreachable error, got {error:?}");
        };
        assert!(!cause.is_empty());
        let shown = error.to_string();
        assert!(shown.starts_with("scale-set request failed or timed out"));
        assert!(shown.ends_with(cause.as_str()));
        for hidden in [base.as_str(), "http://", "actions/runners", PAT] {
            assert!(!shown.contains(hidden), "{shown}");
        }
    }

    #[tokio::test]
    async fn runner_listing_refuses_an_empty_page_before_the_total_and_stops_at_100_pages() {
        let fake = Fake::start(vec![
            Reply::json(
                json!({"total_count": 3, "runners": [{"id": 1, "name": "a", "status": "online", "busy": false}]}),
            ),
            Reply::json(json!({"total_count": 3, "runners": []})),
        ])
        .await;
        let error = fake
            .client()
            .list_runners("example-owner", "example-repo", &pat())
            .await
            .unwrap_err();
        assert_eq!(error.kind, ErrorKind::Protocol);
        assert_eq!(fake.requests().len(), 2);
        let page = || {
            Reply::json(
                json!({"total_count": 1000, "runners": [{"id": 1, "name": "a", "status": "online", "busy": false}]}),
            )
        };
        let fake = Fake::start((0..101).map(|_| page()).collect()).await;
        let error = fake
            .client()
            .list_runners("example-owner", "example-repo", &pat())
            .await
            .unwrap_err();
        assert_eq!(error.kind, ErrorKind::Protocol);
        let requests = fake.requests();
        assert_eq!(requests.len(), 100);
        assert!(requests[99].path.ends_with("per_page=100&page=100"));
    }

    #[tokio::test]
    async fn two_runners_with_the_exact_name_are_an_ownership_mismatch() {
        let runner = |id: u64, name: &str| json!({"id": id, "name": name, "status": "online", "busy": false});
        for (runners, expected) in [
            (vec![runner(1, RUNNER), runner(2, RUNNER)], Err(())),
            (vec![runner(1, RUNNER), runner(2, "other")], Ok(Some(1))),
            (vec![runner(2, "other")], Ok(None)),
        ] {
            let fake = Fake::start(vec![Reply::json(
                json!({"total_count": runners.len(), "runners": runners}),
            )])
            .await;
            let found = fake
                .client()
                .find_runner("example-owner", "example-repo", RUNNER, &pat())
                .await;
            match expected {
                Err(()) => assert_eq!(found.unwrap_err().kind, ErrorKind::OwnershipMismatch),
                Ok(id) => assert_eq!(found.unwrap().map(|runner| runner.id), id),
            }
        }
    }

    #[tokio::test]
    async fn scale_set_lookup_refuses_inconsistent_counts_and_invalid_names_before_any_request() {
        for body in [
            json!({"count": 2, "value": [scale(json!([{"name": NAME}]))]}),
            json!({"count": 2, "value": [scale(json!([{"name": NAME}])), scale(json!([{"name": NAME}]))]}),
            json!({"count": 0, "value": [scale(json!([{"name": NAME}]))]}),
        ] {
            let mut replies = connection_replies();
            replies.push(Reply::json(body));
            let fake = Fake::start(replies).await;
            let client = fake.client();
            let connection = fake.connection(&client).await;
            let error = client.find_or_create(&connection, NAME).await.unwrap_err();
            assert_eq!(error.kind, ErrorKind::Protocol);
            assert_eq!(fake.requests().len(), 3);
        }
        let fake = Fake::start(connection_replies()).await;
        let client = fake.client();
        let connection = fake.connection(&client).await;
        for name in ["", "has space", "slash/name", &"x".repeat(46)] {
            let error = client.find_or_create(&connection, name).await.unwrap_err();
            assert_eq!(error.kind, ErrorKind::InvalidInput, "{name:?}");
        }
        assert_eq!(fake.requests().len(), 2);
    }

    #[tokio::test]
    async fn an_acquire_answer_naming_an_unrequested_job_or_miscounting_is_a_protocol_error() {
        let (_fake, client, connection, session) = listening(vec![
            Reply::json(json!({"count": 1, "value": [999]})),
            Reply::json(json!({"count": 2, "value": [501]})),
        ])
        .await;
        for _ in 0..2 {
            let error = client
                .acquire_jobs(&connection, &session, &[501])
                .await
                .unwrap_err();
            assert_eq!(error.kind, ErrorKind::Protocol);
        }
    }
}
