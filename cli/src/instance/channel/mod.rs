//! The control channel: how `stop`, `restart`, `status` and a resuming `start` in
//! another terminal talk to the instance.
//!
//! It is per user and carries the text protocol in [`protocol`](super::protocol).
//! Only the instance's own user can reach it:
//!
//! - On Windows it is the named pipe `\\.\pipe\runners-control-<user SID>`, created
//!   as the first instance with the descriptor `D:P(A;;GA;;;<user SID>)` and with
//!   remote clients rejected. The pipe crate's blocking calls run on threads of
//!   their own, which hand each request to the runtime.
//! - On Unix it is the socket `control.sock`, mode `0600`, in a directory that must
//!   be `0700`, owned by the current user, and not a link. The server also checks
//!   that each peer's user ID is its own.
//!
//! The server can be opened only by the holder of the instance lock, so a leftover
//! socket file proves no other instance owns it and is replaced.

mod client;
#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

use std::fmt;

use tokio::sync::mpsc;

use super::lock::InstanceLock;
use super::protocol::Incoming;
use super::roots::Roots;

pub use client::{CONNECT_TIMEOUT, ClientError, exchange};

#[cfg(unix)]
use unix::connect as platform_connect;
#[cfg(windows)]
use windows::connect as platform_connect;

/// How many connections the server serves at once; more wait for a turn.
const MAX_CONNECTIONS: usize = 16;

/// Why the channel could not be opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChannelError {
    /// The socket's path is longer than the operating system allows.
    PathTooLong(String),
    /// The socket directory is not private to the current user.
    #[cfg(unix)]
    Directory(crate::private::PrivateError),
    /// Another process already created the channel's name.
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

/// The open channel. Dropping it closes it.
#[derive(Debug)]
pub struct ChannelServer {
    inner: Inner,
}

#[cfg(unix)]
use unix::Server as Inner;
#[cfg(windows)]
use windows::Server as Inner;

impl ChannelServer {
    /// Opens the channel and sends every request it receives to `requests`, with the
    /// sender its answer goes back through. The lock proves no other instance owns
    /// the name. On Unix this must run inside a Tokio runtime.
    ///
    /// # Errors
    ///
    /// See [`ChannelError`]. Every one of them is a startup failure.
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

    /// Where clients reach the channel: the socket's path, or the pipe's name.
    #[must_use]
    pub fn address(&self) -> String {
        self.inner.address()
    }

    /// Like [`open`](Self::open), but accepts only peers whose user ID is `peer_uid`.
    /// A test uses it to show that a peer of another user is turned away; a debug
    /// build only.
    ///
    /// # Errors
    ///
    /// See [`ChannelError`].
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
