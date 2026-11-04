//! the agent's channel keeps today's access rules: the user's own processes only, a
//! socket private to the user, and a startup failure over a channel it cannot make safe.
#![cfg(unix)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod support;

use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::sync::Arc;
use std::time::Duration;

use marshal_cli::agent::AgentChannel;
use marshal_cli::agent::machine::Launch;
use marshal_cli::agent::server::Limits;
use marshal_cli::agent::snapshot::Snapshot;
use marshal_cli::instance::channel::ChannelError;
use marshal_cli::instance::lock::InstanceLock;
use marshal_cli::instance::roots::Roots;
use marshal_cli::private::current_uid;
use support::TempDir;
use support::agent::{ONE_POOL, Rig};
use tokio::sync::{mpsc, watch};

fn mode(path: &std::path::Path) -> u32 {
    fs::symlink_metadata(path).unwrap().mode_bits()
}

trait Bits {
    fn mode_bits(&self) -> u32;
}

impl Bits for fs::Metadata {
    fn mode_bits(&self) -> u32 {
        use std::os::unix::fs::MetadataExt;
        self.mode() & 0o7777
    }
}

#[tokio::test(flavor = "current_thread")]
async fn the_socket_is_mode_0600_in_a_directory_created_0700_and_goes_with_the_agent() {
    let rig = Rig::new(ONE_POOL);
    let rig = &rig;
    let socket = rig.roots.socket_path();
    let directory = rig.roots.channel_directory().to_path_buf();
    let (_, ()) = rig
        .drive(Launch::Stopped, |conn| async move {
            assert_eq!(mode(&socket), 0o600);
            assert_eq!(mode(&directory), 0o700);
            conn.ok("quit").await;
        })
        .await;
    assert!(
        !rig.roots.socket_path().exists(),
        "the socket outlived the agent"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_peer_of_another_user_is_dropped_without_an_answer() {
    let root = TempDir::new();
    let roots = Roots::for_tests(root.path());
    let lock = InstanceLock::acquire(&roots).unwrap();
    let (calls, mut requests) = mpsc::unbounded_channel();
    let (_sender, snapshots) = watch::channel(Arc::new(Snapshot::default()));
    let other = current_uid().unwrap().wrapping_add(1);
    let _channel =
        AgentChannel::open_for_peer(&lock, &roots, calls, snapshots, Limits::default(), other)
            .unwrap();
    let socket = roots.socket_path();
    let answer = tokio::task::spawn_blocking(move || {
        let mut stream = UnixStream::connect(socket).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let _ = stream.write_all(b"{\"type\":\"hello\",\"versions\":[2]}\n");
        let mut answer = String::new();
        let _ = stream.read_to_string(&mut answer);
        answer
    })
    .await
    .unwrap();
    assert_eq!(answer, "");
    assert!(requests.try_recv().is_err());
}

#[tokio::test(flavor = "current_thread")]
async fn a_directory_open_to_others_is_a_startup_failure_and_is_left_as_it_was() {
    let root = TempDir::new();
    let roots = Roots::for_tests(root.path());
    let lock = InstanceLock::acquire(&roots).unwrap();
    let directory = roots.channel_directory().to_path_buf();
    fs::create_dir_all(&directory).unwrap();
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o755)).unwrap();
    let (calls, _requests) = mpsc::unbounded_channel();
    let (_sender, snapshots) = watch::channel(Arc::new(Snapshot::default()));
    let error = AgentChannel::open(&lock, &roots, calls, snapshots, Limits::default())
        .err()
        .expect("the channel opened over an open directory");
    assert!(matches!(error, ChannelError::Directory(_)), "{error}");
    assert_eq!(mode(&directory), 0o755);
}
