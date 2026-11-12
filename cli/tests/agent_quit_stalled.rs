//! the agent as a process ends, and frees its lock, within about a second of a
//! `quit` while a subscribed client is still open and has read nothing. On Windows
//! the pipe crate flushes on a blocking thread that waits for the peer to read, which
//! once held the process and the lock; this runs on every platform.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod support;

use std::time::{Duration, Instant};

use marshal_cli::instance::lock::InstanceLock;
use marshal_cli::instance::roots::Roots;
use serde_json::json;
use support::TempDir;
use support::agent::{Conn, HELPER, Raw};

#[tokio::test(flavor = "current_thread")]
async fn quit_ends_the_process_and_frees_the_lock_while_a_subscribed_client_reads_nothing() {
    let root = TempDir::new();
    let mut child = std::process::Command::new(HELPER)
        .args(["agent", "--root"])
        .arg(root.path())
        .arg("--stopped")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let roots = Roots::for_tests(root.path());

    let mut stalled = Raw::hello(&roots).await;
    stalled
        .request(1, "subscribe", json!({"topic": "state"}))
        .await;
    // the first event and the response are now unread on the stalled connection
    let conn = Conn::open(&roots).await;
    conn.status().await;

    let started = Instant::now();
    conn.ok("quit").await;
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        assert!(
            started.elapsed() < Duration::from_millis(2500),
            "the agent did not end within a second or so of quit"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    assert!(status.success(), "{status:?}");
    assert!(InstanceLock::acquire(&roots).is_ok(), "the lock is held");
    // the stalled client was open throughout
    drop(stalled);
}
