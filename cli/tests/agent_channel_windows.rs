//! the agent's pipe on Windows is created as the first instance of its name, so a name
//! another process already holds is detected and not shared. Windows only.
#![cfg(windows)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod support;

use std::sync::Arc;

use marshal_cli::agent::AgentChannel;
use marshal_cli::agent::server::Limits;
use marshal_cli::agent::snapshot::Snapshot;
use marshal_cli::instance::channel::ChannelError;
use marshal_cli::instance::lock::InstanceLock;
use marshal_cli::instance::roots::Roots;
use marshal_cli::instance::windows::control_pipe_options;
use support::TempDir;
use tokio::sync::{mpsc, watch};

#[tokio::test(flavor = "current_thread")]
async fn a_pipe_name_another_listener_holds_is_a_startup_failure() {
    let root = TempDir::new();
    let roots = Roots::for_tests(root.path());
    let lock = InstanceLock::acquire(&roots).unwrap();
    // a squatter that created the user's pipe name first, with the same options
    let (options, _) = control_pipe_options(&roots).unwrap();
    let _squatter = options
        .create_tokio_duplex::<interprocess::os::windows::named_pipe::pipe_mode::Bytes>()
        .unwrap();

    let (calls, _requests) = mpsc::unbounded_channel();
    let (_sender, snapshots) = watch::channel(Arc::new(Snapshot::default()));
    let error = AgentChannel::open(&lock, &roots, calls, snapshots, Limits::default())
        .err()
        .expect("the channel opened over a name another listener holds");
    assert!(matches!(error, ChannelError::NameTaken(_)), "{error}");
}

#[tokio::test(flavor = "current_thread")]
async fn a_second_agent_channel_on_the_same_name_is_a_startup_failure() {
    let root = TempDir::new();
    let roots = Roots::for_tests(root.path());
    let lock = InstanceLock::acquire(&roots).unwrap();
    let (calls, _requests) = mpsc::unbounded_channel();
    let (_sender, snapshots) = watch::channel(Arc::new(Snapshot::default()));
    let _first =
        AgentChannel::open(&lock, &roots, calls, snapshots.clone(), Limits::default()).unwrap();
    let (calls, _more) = mpsc::unbounded_channel();
    let error = AgentChannel::open(&lock, &roots, calls, snapshots, Limits::default())
        .err()
        .expect("a second channel opened on one name");
    assert!(matches!(error, ChannelError::NameTaken(_)), "{error}");
}

#[tokio::test(flavor = "current_thread")]
async fn a_client_connecting_to_a_real_agent_passes_the_check_of_the_server_s_user() {
    let root = TempDir::new();
    let roots = Roots::for_tests(root.path());
    let lock = InstanceLock::acquire(&roots).unwrap();
    let (calls, _requests) = mpsc::unbounded_channel();
    let (_sender, snapshots) = watch::channel(Arc::new(Snapshot::default()));
    let _channel = AgentChannel::open(&lock, &roots, calls, snapshots, Limits::default()).unwrap();
    // the server is this process, which runs as this user, so the check lets it through
    let (client, _pushes) =
        marshal_cli::agent::connect(&roots, &marshal_cli::agent::wire::ClientInfo::default())
            .await
            .expect("the client refused the agent of its own user");
    assert_eq!(client.welcome.version, 2);
}
