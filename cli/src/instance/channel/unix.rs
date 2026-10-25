//! The control socket.

use std::fs;
use std::io;
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::os::unix::net::UnixStream as StdUnixStream;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{Semaphore, mpsc};
use tokio::task::JoinHandle;

use crate::private::{self, FILE_MODE};

use super::super::protocol::{Incoming, MAX_REQUEST_BYTES, Response, line_is_cut, submit};
use super::super::roots::Roots;
use super::{ChannelError, ClientError, MAX_CONNECTIONS};

/// How long a connected client has to send its request.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

/// How long the accept loop waits after the operating system refuses a connection,
/// such as when it has no file descriptors left.
const ACCEPT_BACKOFF: Duration = Duration::from_millis(100);

/// The listening socket. Dropping it stops accepting and removes the socket file.
#[derive(Debug)]
pub struct Server {
    socket: PathBuf,
    accepting: JoinHandle<()>,
    /// One permit per connection being served, so holding them all means none is.
    connections: Arc<Semaphore>,
}

impl Drop for Server {
    fn drop(&mut self) {
        self.accepting.abort();
        let _ = fs::remove_file(&self.socket);
    }
}

fn io_error(detail: &str, error: &io::Error) -> ChannelError {
    ChannelError::Io(format!("{detail}: {error}"))
}

/// Removes a socket file a hard-killed instance left behind. Anything else at the
/// path is not ours to remove.
fn remove_leftover(socket: &Path, uid: u32) -> Result<(), ChannelError> {
    match fs::symlink_metadata(socket) {
        Ok(metadata) if metadata.file_type().is_socket() && metadata.uid() == uid => {
            fs::remove_file(socket).map_err(|error| io_error("removing the old socket", &error))
        }
        Ok(_) => Err(ChannelError::Io(format!(
            "{} exists and is not a socket of the current user",
            socket.display()
        ))),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io_error("examining the old socket", &error)),
    }
}

/// Sets the socket to `0600` and checks it: a socket, owned by the user, with that
/// mode. The directory is private, so nobody else could reach it in the meantime.
fn restrict(socket: &Path, uid: u32) -> Result<(), ChannelError> {
    fs::set_permissions(socket, fs::Permissions::from_mode(FILE_MODE))
        .map_err(|error| io_error("setting the socket's mode", &error))?;
    let metadata =
        fs::symlink_metadata(socket).map_err(|error| io_error("examining the socket", &error))?;
    if !metadata.file_type().is_socket()
        || metadata.uid() != uid
        || metadata.mode() & 0o7777 != FILE_MODE
    {
        return Err(ChannelError::Io(format!(
            "{} is not a socket of the current user with mode 0600",
            socket.display()
        )));
    }
    Ok(())
}

impl Server {
    pub fn address(&self) -> String {
        self.socket.display().to_string()
    }

    pub fn open(
        roots: &Roots,
        requests: mpsc::UnboundedSender<Incoming>,
    ) -> Result<Self, ChannelError> {
        let uid = private::current_uid().map_err(|error| io_error("finding the user", &error))?;
        Self::open_accepting(roots, requests, uid)
    }

    /// Opens the socket and accepts only peers whose user ID is `peer_uid`.
    pub fn open_accepting(
        roots: &Roots,
        requests: mpsc::UnboundedSender<Incoming>,
        peer_uid: u32,
    ) -> Result<Self, ChannelError> {
        let uid = private::current_uid().map_err(|error| io_error("finding the user", &error))?;
        private::ensure_private_directory(roots.channel_directory(), uid)
            .map_err(ChannelError::Directory)?;
        let socket = roots.socket_path();
        remove_leftover(&socket, uid)?;
        let listener = UnixListener::bind(&socket).map_err(|error| {
            if error.kind() == io::ErrorKind::InvalidInput {
                ChannelError::PathTooLong(socket.display().to_string())
            } else {
                io_error("binding the socket", &error)
            }
        })?;
        restrict(&socket, uid)?;
        let connections = Arc::new(Semaphore::new(MAX_CONNECTIONS));
        let accepting = tokio::spawn(accept(
            listener,
            requests,
            peer_uid,
            Arc::clone(&connections),
        ));
        Ok(Self {
            socket,
            accepting,
            connections,
        })
    }

    pub async fn close(self, within: Duration) {
        self.accepting.abort();
        let every = u32::try_from(MAX_CONNECTIONS).unwrap_or(u32::MAX);
        let _ = tokio::time::timeout(within, self.connections.acquire_many(every)).await;
    }
}

async fn accept(
    listener: UnixListener,
    requests: mpsc::UnboundedSender<Incoming>,
    peer_uid: u32,
    permits: Arc<Semaphore>,
) {
    loop {
        let Ok(permit) = Arc::clone(&permits).acquire_owned().await else {
            return;
        };
        let stream = match listener.accept().await {
            Ok((stream, _)) => stream,
            Err(_) => {
                tokio::time::sleep(ACCEPT_BACKOFF).await;
                continue;
            }
        };
        let requests = requests.clone();
        tokio::spawn(async move {
            serve(stream, requests, peer_uid).await;
            drop(permit);
        });
    }
}

/// Serves one connection: a peer of another user, a request that does not arrive in
/// time, and a line longer than the limit are dropped without an answer.
async fn serve(stream: UnixStream, requests: mpsc::UnboundedSender<Incoming>, peer_uid: u32) {
    match stream.peer_cred() {
        Ok(credentials) if credentials.uid() == peer_uid => {}
        _ => return,
    }
    let (read, mut write) = stream.into_split();
    let mut reader = BufReader::new(read);
    let mut line = Vec::new();
    let mut limited = (&mut reader).take(MAX_REQUEST_BYTES as u64);
    let read_line = limited.read_until(b'\n', &mut line);
    match tokio::time::timeout(REQUEST_TIMEOUT, read_line).await {
        Ok(Ok(count)) if count > 0 && !line_is_cut(&line, MAX_REQUEST_BYTES) => {}
        _ => return,
    }
    let response = match submit(&line, &requests) {
        Ok(answer) => answer.await.unwrap_or_else(|_| Response::ending()),
        Err(refusal) => refusal,
    };
    if write.write_all(response.encode().as_bytes()).await.is_ok() {
        let _ = write.shutdown().await;
    }
}

/// Connects to this user's socket, after checking its directory is private.
pub fn connect(roots: &Roots) -> Result<StdUnixStream, ClientError> {
    let uid = private::current_uid().map_err(|error| ClientError::Failed(error.to_string()))?;
    match private::check_private_directory(roots.channel_directory(), uid) {
        Ok(()) => {}
        Err(private::PrivateError::Missing(_)) => return Err(ClientError::NoInstance),
        Err(error) => return Err(ClientError::Failed(error.to_string())),
    }
    StdUnixStream::connect(roots.socket_path()).map_err(|error| match error.kind() {
        io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused => ClientError::NoInstance,
        _ => ClientError::Failed(error.to_string()),
    })
}
