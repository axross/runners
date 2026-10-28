//! the control channel, reachable only by the instance's own user; only the holder
//! of the instance lock can open the server, so a leftover socket file proves no
//! other instance owns it and is replaced; `docs/operations/cli.md` describes the
//! pipe and the socket.

mod client;
#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

use std::fmt;
use std::time::Duration;

use tokio::sync::mpsc;

use super::lock::InstanceLock;
use super::protocol::Incoming;
use super::roots::Roots;

pub use client::{CONNECT_TIMEOUT, ClientError, exchange};

#[cfg(unix)]
use unix::connect as platform_connect;
#[cfg(windows)]
use windows::connect as platform_connect;

/// connections served at once; on Unix a further one waits for a turn, on Windows
/// it is closed unanswered.
const MAX_CONNECTIONS: usize = 16;

/// [`MAX_CONNECTIONS`], for the test that fills every connection; a debug build only.
#[cfg(debug_assertions)]
#[doc(hidden)]
pub const MAX_CONNECTIONS_FOR_TESTS: usize = MAX_CONNECTIONS;

/// why the channel could not be opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChannelError {
    /// the socket's path is longer than the operating system allows.
    PathTooLong(String),
    /// the socket directory is not private to the current user.
    #[cfg(unix)]
    Directory(crate::private::PrivateError),
    /// another process already created the channel's name.
    NameTaken(String),
    Io(String),
}

impl fmt::Display for ChannelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PathTooLong(path) => write!(
                f,
                "the control socket path is longer than the operating system allows: {path}"
            ),
            #[cfg(unix)]
            Self::Directory(error) => {
                write!(f, "the control socket directory is not private: {error}")
            }
            Self::NameTaken(name) => write!(
                f,
                "another process already created the control channel {name}"
            ),
            Self::Io(detail) => write!(f, "the control channel cannot be opened: {detail}"),
        }
    }
}

impl std::error::Error for ChannelError {}

/// the open channel; dropping it closes it.
#[derive(Debug)]
pub struct ChannelServer {
    inner: Inner,
}

#[cfg(unix)]
use unix::Server as Inner;
#[cfg(windows)]
use windows::Server as Inner;

impl ChannelServer {
    /// opens the channel and sends every request it receives to `requests`, with the
    /// sender its answer goes back through; every error is a startup failure, and on
    /// Unix this must run inside a Tokio runtime.
    pub fn open(
        lock: &InstanceLock,
        roots: &Roots,
        requests: mpsc::UnboundedSender<Incoming>,
    ) -> Result<Self, ChannelError> {
        let _ = lock;
        Ok(Self {
            inner: Inner::open(roots, requests)?,
        })
    }

    /// stops accepting, then waits up to `within` for accepted connections to end, so
    /// an answer already given reaches its client before the process exits; drop the
    /// request receiver first, so a request still waiting gets its "ending" answer at
    /// once.
    pub async fn close(self, within: Duration) {
        self.inner.close(within).await;
    }

    /// where clients reach the channel: the socket's path, or the pipe's name.
    #[must_use]
    pub fn address(&self) -> String {
        self.inner.address()
    }

    /// like [`open`](Self::open), but accepts only peers whose user ID is `peer_uid`,
    /// to show that a peer of another user is turned away; a debug build only.
    #[cfg(all(unix, debug_assertions))]
    #[doc(hidden)]
    pub fn open_for_peer(
        lock: &InstanceLock,
        roots: &Roots,
        requests: mpsc::UnboundedSender<Incoming>,
        peer_uid: u32,
    ) -> Result<Self, ChannelError> {
        let _ = lock;
        Ok(Self {
            inner: Inner::open_accepting(roots, requests, peer_uid)?,
        })
    }
}
