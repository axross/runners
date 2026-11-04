//! the agent's socket listener.

use std::io;
use std::path::PathBuf;

use tokio::io::{ReadHalf, WriteHalf};
use tokio::net::{UnixListener, UnixStream};

use crate::instance::channel::{ChannelError, bind_private};
use crate::instance::roots::Roots;
use crate::private;

/// the listening socket, which accepts only the current user's processes; dropping
/// it removes the socket file.
pub struct Listener {
    listener: UnixListener,
    socket: PathBuf,
    uid: u32,
}

impl Drop for Listener {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.socket);
    }
}

impl Listener {
    pub fn open(roots: &Roots) -> Result<Self, ChannelError> {
        let uid = private::current_uid()
            .map_err(|error| ChannelError::Io(format!("finding the user: {error}")))?;
        Self::open_accepting(roots, uid)
    }

    /// opens the socket for the peers whose user ID is `uid`.
    pub fn open_accepting(roots: &Roots, uid: u32) -> Result<Self, ChannelError> {
        let (listener, socket) = bind_private(roots)?;
        Ok(Self {
            listener,
            socket,
            uid,
        })
    }

    pub fn address(&self) -> String {
        self.socket.display().to_string()
    }

    /// the next connection whose peer is the current user; any other peer is closed
    /// unanswered.
    pub async fn accept(&mut self) -> io::Result<UnixStream> {
        loop {
            let (stream, _) = self.listener.accept().await?;
            if stream
                .peer_cred()
                .is_ok_and(|credentials| credentials.uid() == self.uid)
            {
                return Ok(stream);
            }
        }
    }
}

/// the two halves of an accepted connection.
pub fn split(stream: UnixStream) -> (ReadHalf<UnixStream>, WriteHalf<UnixStream>) {
    tokio::io::split(stream)
}
