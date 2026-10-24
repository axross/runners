//! The control channel: requests reach the instance and answers come back, on a
//! Unix socket or a Windows pipe. Only debug builds can move the channel's root.
#![cfg(debug_assertions)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod support;

use std::time::Duration;

use runners_cli::instance::channel::{ChannelServer, ClientError, exchange};
use runners_cli::instance::fake::FakeCore;
use runners_cli::instance::lifecycle::{Instance, Settings, Source};
use runners_cli::instance::lock::InstanceLock;
use runners_cli::instance::protocol::{Incoming, Outcome, Request, Response};
use runners_cli::instance::roots::Roots;
use runners_cli::instance::source::FileSource;
use runners_cli::profile::Profile;
use runners_cli::runtime::fake::{FakeBackend, FakeClock, MemoryLog};
use support::TempDir;
use support::instance::{CONFIG, START};
use tokio::sync::mpsc;

const PATIENCE: Duration = Duration::from_secs(10);

/// An open channel whose requests arrive at `requests`.
struct Open {
    roots: Roots,
    _server: ChannelServer,
    _lock: InstanceLock,
}

fn open(root: &TempDir) -> (Open, mpsc::UnboundedReceiver<Incoming>) {
    let roots = Roots::for_tests(root.path());
    let lock = InstanceLock::acquire(&roots).unwrap();
    let (sender, requests) = mpsc::unbounded_channel();
    let server = ChannelServer::open(&lock, &roots, sender).unwrap();
    let channel = Open {
        roots,
        _server: server,
        _lock: lock,
    };
    (channel, requests)
}

/// Sends a request from a thread of its own, as the client process would.
async fn ask(roots: &Roots, request: Request) -> Result<Response, ClientError> {
    let roots = roots.clone();
    tokio::task::spawn_blocking(move || exchange(&roots, &request, PATIENCE))
        .await
        .unwrap()
}

/// Answers every request with `answer` and records what was asked.
fn answer_all(
    mut requests: mpsc::UnboundedReceiver<Incoming>,
    answer: Response,
) -> tokio::task::JoinHandle<Vec<Request>> {
    tokio::spawn(async move {
        let mut asked = Vec::new();
        while let Some(incoming) = requests.recv().await {
            asked.push(incoming.request);
            let _ = incoming.reply.send(answer.clone());
        }
        asked
    })
}

#[tokio::test(flavor = "current_thread")]
async fn a_status_request_reaches_the_instance_and_its_answer_comes_back() {
    let root = TempDir::new();
    let (channel, mut requests) = open(&root);
    let roots = channel.roots.clone();
    let instance = tokio::spawn(async move {
        let incoming = requests.recv().await.unwrap();
        assert_eq!(incoming.request, Request::Status);
        let _ = incoming
            .reply
            .send(Response::ok("running").with("state", "running"));
    });

    let response = ask(&roots, Request::Status).await.unwrap();

    assert_eq!(response.outcome, Outcome::Ok);
    assert_eq!(response.message, "running");
    assert_eq!(response.field("state"), Some("running"));
    instance.await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn every_kind_of_request_arrives_as_it_was_sent() {
    let root = TempDir::new();
    let (channel, requests) = open(&root);
    let roots = channel.roots.clone();
    let instance = answer_all(requests, Response::ok("done"));
    let resume = Request::resume(
        Profile::Axmc,
        std::path::Path::new("/Users/me/with space/host.json"),
    )
    .unwrap();

    for request in [
        Request::Status,
        Request::Stop,
        Request::Restart,
        resume.clone(),
    ] {
        let response = ask(&roots, request).await.unwrap();
        assert_eq!(response.outcome, Outcome::Ok);
    }

    drop(channel);
    let asked = tokio::time::timeout(PATIENCE, instance)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        asked,
        vec![Request::Status, Request::Stop, Request::Restart, resume]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_refusal_comes_back_as_a_refusal() {
    let root = TempDir::new();
    let (channel, requests) = open(&root);
    let roots = channel.roots.clone();
    answer_all(requests, Response::refused("an instance is stopping"));

    let response = ask(&roots, Request::Restart).await.unwrap();

    assert_eq!(response.outcome, Outcome::Refused);
    assert_eq!(response.message, "an instance is stopping");
}

#[tokio::test(flavor = "current_thread")]
async fn a_client_finds_no_instance_when_nothing_has_opened_the_channel() {
    let root = TempDir::new();
    let roots = Roots::for_tests(root.path());

    assert_eq!(
        ask(&roots, Request::Status).await,
        Err(ClientError::NoInstance)
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_client_finds_no_instance_once_the_channel_is_closed() {
    let root = TempDir::new();
    let (channel, _requests) = open(&root);
    let roots = channel.roots.clone();
    drop(channel);

    assert_eq!(
        ask(&roots, Request::Status).await,
        Err(ClientError::NoInstance)
    );
}

#[tokio::test(flavor = "current_thread")]
async fn several_clients_are_served_at_the_same_time() {
    let root = TempDir::new();
    let (channel, mut requests) = open(&root);
    let roots = channel.roots.clone();
    let clients: Vec<_> = (0..4)
        .map(|_| {
            let roots = roots.clone();
            tokio::spawn(async move { ask(&roots, Request::Status).await })
        })
        .collect();

    // none is answered until all four have arrived, so all four were connected at once
    let mut waiting = Vec::new();
    for _ in 0..4 {
        waiting.push(
            tokio::time::timeout(PATIENCE, requests.recv())
                .await
                .unwrap()
                .unwrap(),
        );
    }
    for incoming in waiting {
        let _ = incoming.reply.send(Response::ok("running"));
    }

    for client in clients {
        assert_eq!(client.await.unwrap().unwrap().message, "running");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_request_that_is_never_answered_ends_as_no_answer_at_the_clients_limit() {
    let root = TempDir::new();
    let (channel, mut requests) = open(&root);
    let roots = channel.roots.clone();
    let silent = tokio::spawn(async move {
        let incoming = requests.recv().await.unwrap();
        tokio::time::sleep(PATIENCE).await;
        drop(incoming);
    });

    let thread_roots = roots.clone();
    let outcome = tokio::task::spawn_blocking(move || {
        exchange(&thread_roots, &Request::Status, Duration::from_millis(300))
    })
    .await
    .unwrap();

    assert_eq!(outcome, Err(ClientError::NoAnswer));
    silent.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn an_instance_that_drops_the_reply_is_answered_as_ending() {
    let root = TempDir::new();
    let (channel, mut requests) = open(&root);
    let roots = channel.roots.clone();
    tokio::spawn(async move {
        while let Some(incoming) = requests.recv().await {
            drop(incoming.reply);
        }
    });

    let response = ask(&roots, Request::Stop).await.unwrap();

    assert_eq!(response.outcome, Outcome::Refused);
    assert!(response.message.contains("ending"), "{}", response.message);
}

#[tokio::test(flavor = "current_thread")]
async fn a_request_after_the_instance_stopped_receiving_is_answered_as_ending() {
    let root = TempDir::new();
    let (channel, requests) = open(&root);
    let roots = channel.roots.clone();
    drop(requests);

    let response = ask(&roots, Request::Status).await.unwrap();

    assert_eq!(response.outcome, Outcome::Refused);
    assert!(response.message.contains("ending"), "{}", response.message);
}

#[tokio::test(flavor = "current_thread")]
async fn no_request_makes_the_instance_read_a_configuration_path_other_than_its_own() {
    let root = TempDir::new();
    let own = std::fs::canonicalize(root.write("own/host.json", CONFIG.as_bytes())).unwrap();
    let other = std::fs::canonicalize(root.write("other/host.json", CONFIG.as_bytes())).unwrap();
    let own_text = own.to_str().unwrap().to_string();
    let source = FileSource::new(own.clone(), Profile::Axpc);
    let plan = source.load().await.unwrap();
    let (channel, mut requests) = open(&root);
    let roots = channel.roots.clone();
    let (backend, clock, core, log) = (
        FakeBackend::default(),
        FakeClock::at(START),
        FakeCore::default(),
        MemoryLog::default(),
    );
    let settings = Settings {
        config_path: own_text.clone(),
        profile: Profile::Axpc,
    };
    let instance = Instance::new(settings, &backend, &clock, &core, &source, &log);

    let script = async {
        // the instance's own file becomes invalid while the other stays valid
        std::fs::write(&own, b"{}").unwrap();
        let restart = ask(&roots, Request::Restart).await.unwrap();
        assert_eq!(restart.outcome, Outcome::Refused, "{restart:?}");
        assert_eq!(restart.field("state"), Some("stopped"));

        let elsewhere = Request::resume(Profile::Axpc, &other).unwrap();
        let resume = ask(&roots, elsewhere).await.unwrap();
        assert_eq!(resume.outcome, Outcome::Refused, "{resume:?}");
        assert!(resume.message.contains("another configuration path"));

        let status = ask(&roots, Request::Status).await.unwrap();
        assert_eq!(status.field("state"), Some("stopped"));
        assert_eq!(status.field("config"), Some(own_text.as_str()));
        assert!(
            status.field("reason").unwrap().contains("not valid"),
            "{status:?}"
        );
        assert_eq!(
            ask(&roots, Request::Stop).await.unwrap().outcome,
            Outcome::Ok
        );
    };
    let run = instance.run(plan, &mut requests);
    tokio::time::timeout(PATIENCE, async { tokio::join!(run, script) })
        .await
        .unwrap();
    assert_eq!(core.runs.get(), 1, "the core ran only with the first plan");
}

#[cfg(unix)]
mod socket {
    use std::fs;
    use std::io::{Read, Write};
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::Path;

    use runners_cli::instance::channel::ChannelError;
    use runners_cli::instance::protocol::MAX_REQUEST_BYTES;
    use runners_cli::private::{PrivateError, current_uid};

    use super::*;

    fn mode(path: &Path) -> u32 {
        fs::symlink_metadata(path).unwrap().mode() & 0o7777
    }

    fn open_error(root: &TempDir) -> ChannelError {
        let roots = Roots::for_tests(root.path());
        let lock = InstanceLock::acquire(&roots).unwrap();
        let (sender, _requests) = mpsc::unbounded_channel();
        ChannelServer::open(&lock, &roots, sender).unwrap_err()
    }

    /// Sends `bytes` and reads until the server closes. A server that drops the peer
    /// first can fail the write with a broken pipe instead of ending the read, so a
    /// failed write reads as an empty answer.
    fn raw_exchange(roots: &Roots, bytes: &[u8]) -> String {
        let mut stream = UnixStream::connect(roots.socket_path()).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        if stream.write_all(bytes).is_err() {
            return String::new();
        }
        let mut answer = String::new();
        let _ = stream.read_to_string(&mut answer);
        answer
    }

    #[tokio::test(flavor = "current_thread")]
    async fn the_socket_is_mode_0600_in_a_directory_created_0700() {
        let root = TempDir::new();
        let (open, _requests) = open(&root);

        assert_eq!(mode(&open.roots.socket_path()), 0o600);
        assert_eq!(mode(open.roots.channel_directory()), 0o700);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn the_directory_is_created_private_whatever_the_umask() {
        let root = TempDir::new();
        let roots = Roots::for_tests(root.path());
        let lock = InstanceLock::acquire(&roots).unwrap();
        let (sender, _requests) = mpsc::unbounded_channel();

        let _server = ChannelServer::open(&lock, &roots, sender).unwrap();

        assert_eq!(mode(roots.channel_directory()), 0o700);
        let uid = current_uid().unwrap();
        assert_eq!(fs::metadata(roots.channel_directory()).unwrap().uid(), uid);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_directory_open_to_others_is_a_startup_failure_and_is_left_as_it_was() {
        let root = TempDir::new();
        let directory = root.path().join("channel");
        fs::create_dir(&directory).unwrap();
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o755)).unwrap();

        let error = open_error(&root);

        assert!(
            matches!(
                error,
                ChannelError::Directory(PrivateError::WrongMode { found: 0o755, .. })
            ),
            "{error}"
        );
        assert_eq!(mode(&directory), 0o755);
        assert!(!directory.join("control.sock").exists());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_link_in_place_of_the_directory_is_a_startup_failure() {
        let root = TempDir::new();
        let real = root.path().join("real");
        fs::create_dir(&real).unwrap();
        fs::set_permissions(&real, fs::Permissions::from_mode(0o700)).unwrap();
        std::os::unix::fs::symlink(&real, root.path().join("channel")).unwrap();

        let error = open_error(&root);

        assert!(
            matches!(
                error,
                ChannelError::Directory(PrivateError::NotADirectory(_))
            ),
            "{error}"
        );
        assert!(!real.join("control.sock").exists());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_socket_path_the_system_cannot_hold_is_a_startup_failure() {
        let outer = TempDir::new();
        let long = outer.path().join("d".repeat(150));
        fs::create_dir(&long).unwrap();
        let roots = Roots::for_tests(&long);
        let lock = InstanceLock::acquire(&roots).unwrap();
        let (sender, _requests) = mpsc::unbounded_channel();

        let error = ChannelServer::open(&lock, &roots, sender).unwrap_err();

        assert!(matches!(error, ChannelError::PathTooLong(_)), "{error}");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_socket_file_a_killed_instance_left_behind_is_replaced() {
        let root = TempDir::new();
        let roots = Roots::for_tests(root.path());
        fs::create_dir(roots.channel_directory()).unwrap();
        fs::set_permissions(roots.channel_directory(), fs::Permissions::from_mode(0o700)).unwrap();
        // binding creates the file and dropping the listener leaves it, as a kill does
        drop(UnixListener::bind(roots.socket_path()).unwrap());
        assert!(roots.socket_path().exists());
        let lock = InstanceLock::acquire(&roots).unwrap();
        let (sender, requests) = mpsc::unbounded_channel();

        let _server = ChannelServer::open(&lock, &roots, sender).unwrap();
        answer_all(requests, Response::ok("running"));

        assert_eq!(
            ask(&roots, Request::Status).await.unwrap().message,
            "running"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn something_that_is_not_a_socket_at_the_path_is_a_startup_failure_and_is_kept() {
        let root = TempDir::new();
        let roots = Roots::for_tests(root.path());
        fs::create_dir(roots.channel_directory()).unwrap();
        fs::set_permissions(roots.channel_directory(), fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(roots.socket_path(), b"precious").unwrap();

        let error = open_error(&root);

        assert!(matches!(error, ChannelError::Io(_)), "{error}");
        assert_eq!(fs::read(roots.socket_path()).unwrap(), b"precious");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn closing_the_channel_removes_the_socket() {
        let root = TempDir::new();
        let (open, _requests) = open(&root);
        let socket = open.roots.socket_path();
        assert!(socket.exists());

        drop(open);

        assert!(!socket.exists());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_peer_of_another_user_is_dropped_without_an_answer_or_a_request() {
        let root = TempDir::new();
        let roots = Roots::for_tests(root.path());
        let lock = InstanceLock::acquire(&roots).unwrap();
        let (sender, mut requests) = mpsc::unbounded_channel();
        let other = current_uid().unwrap().wrapping_add(1);
        let _server = ChannelServer::open_for_peer(&lock, &roots, sender, other).unwrap();

        let thread_roots = roots.clone();
        let answer = tokio::task::spawn_blocking(move || raw_exchange(&thread_roots, b"status\n"))
            .await
            .unwrap();

        assert_eq!(answer, "");
        assert!(requests.try_recv().is_err());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_line_that_is_not_a_request_is_refused_and_never_reaches_the_instance() {
        let root = TempDir::new();
        let (channel, mut requests) = open(&root);
        let roots = channel.roots.clone();

        let thread_roots = roots.clone();
        let answer =
            tokio::task::spawn_blocking(move || raw_exchange(&thread_roots, b"shutdown now\n"))
                .await
                .unwrap();

        assert!(answer.starts_with("refused"), "{answer}");
        assert!(answer.ends_with("end\n"), "{answer}");
        assert!(requests.try_recv().is_err());
    }

    /// Sends a line of `length` bytes, its line break included.
    async fn send_line_of(roots: &Roots, length: usize) -> String {
        let roots = roots.clone();
        tokio::task::spawn_blocking(move || {
            let mut line = vec![b'a'; length - 1];
            line.push(b'\n');
            raw_exchange(&roots, &line)
        })
        .await
        .unwrap()
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_line_longer_than_the_limit_is_closed_unanswered_and_never_reaches_the_instance() {
        let root = TempDir::new();
        let (channel, mut requests) = open(&root);

        // one byte over the limit, so the server reads the whole limit and no break
        let answer = send_line_of(&channel.roots, MAX_REQUEST_BYTES + 1).await;

        assert_eq!(answer, "");
        assert!(requests.try_recv().is_err());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_line_exactly_at_the_limit_is_answered() {
        let root = TempDir::new();
        let (channel, mut requests) = open(&root);

        let answer = send_line_of(&channel.roots, MAX_REQUEST_BYTES).await;

        assert!(answer.starts_with("refused"), "{answer}");
        assert!(answer.ends_with("end\n"), "{answer}");
        assert!(requests.try_recv().is_err());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_client_that_connects_and_sends_nothing_is_dropped_and_the_next_is_served() {
        let root = TempDir::new();
        let (channel, requests) = open(&root);
        let roots = channel.roots.clone();
        answer_all(requests, Response::ok("running"));
        let idle = UnixStream::connect(roots.socket_path()).unwrap();

        let response = ask(&roots, Request::Status).await.unwrap();

        assert_eq!(response.message, "running");
        let mut idle = idle;
        idle.set_read_timeout(Some(Duration::from_secs(15)))
            .unwrap();
        let mut rest = Vec::new();
        let read =
            tokio::task::spawn_blocking(move || idle.read_to_end(&mut rest).map(|n| (n, rest)))
                .await
                .unwrap();
        assert_eq!(
            read.unwrap().0,
            0,
            "the idle connection was closed with no answer"
        );
    }
}
