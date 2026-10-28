//! the control pipe.

use std::ffi::OsStr;
use std::io::{self, BufReader, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use interprocess::ConnectWaitMode;
use interprocess::os::windows::named_pipe::{
    DuplexPipeStream, PipeListener, PipeListenerOptions, PipeMode, pipe_mode,
};
use tokio::sync::mpsc;

use super::super::protocol::{Incoming, MAX_REQUEST_BYTES, Response, read_line_limited, submit};
use super::super::roots::Roots;
use super::super::windows::{current_user_sid, name_is_taken, only_user_descriptor};
use super::{CONNECT_TIMEOUT, ChannelError, ClientError, MAX_CONNECTIONS};

type Stream = DuplexPipeStream<pipe_mode::Bytes>;

/// how long closing the server waits to connect to its own pipe, each time.
const WAKE_TIMEOUT: Duration = Duration::from_millis(500);
/// how often closing the server tries to connect to its own pipe.
const WAKE_ATTEMPTS: usize = 2;
/// how often [`Server::close`] checks whether every connection has ended.
const CLOSE_POLL: Duration = Duration::from_millis(10);

/// the listening pipe; serving threads are never joined, so a client that sends
/// nothing holds its thread, and that thread's clone of the request sender, until it
/// disconnects.
#[derive(Debug)]
pub struct Server {
    closing: Arc<AtomicBool>,
    name: String,
    accepting: Option<JoinHandle<()>>,
    /// the connections being served.
    active: Arc<AtomicUsize>,
}

impl Drop for Server {
    fn drop(&mut self) {
        self.closing.store(true, Ordering::SeqCst);
        // accept() returns only when a client connects, and it skips a client that is
        // already gone, so the waking connection stays open until the thread ends
        let wake = (0..WAKE_ATTEMPTS).find_map(|_| {
            Stream::connect_by_path_with_wait_mode(
                OsStr::new(&self.name),
                ConnectWaitMode::Timeout(WAKE_TIMEOUT),
            )
            .ok()
        });
        // without a wake the join could wait for a client that never comes, so the
        // thread is left to end at the next client or with the process
        if wake.is_some()
            && let Some(accepting) = self.accepting.take()
        {
            let _ = accepting.join();
        }
        drop(wake);
    }
}

impl Server {
    pub fn address(&self) -> String {
        self.name.clone()
    }

    pub fn open(
        roots: &Roots,
        requests: mpsc::UnboundedSender<Incoming>,
    ) -> Result<Self, ChannelError> {
        let sid = current_user_sid().map_err(|error| ChannelError::Io(error.to_string()))?;
        let descriptor =
            only_user_descriptor(&sid).map_err(|error| ChannelError::Io(error.to_string()))?;
        let name = roots.control_pipe_name(&sid);
        // PipeListenerOptions rejects remote clients unless told otherwise, and
        // creates the first instance of the name.
        let listener = PipeListenerOptions::new()
            .path(OsStr::new(&name))
            .mode(PipeMode::Bytes)
            .security_descriptor(Some(descriptor))
            .create_duplex::<pipe_mode::Bytes>()
            .map_err(|error| {
                if name_is_taken(&error) {
                    ChannelError::NameTaken(name.clone())
                } else {
                    ChannelError::Io(format!("{name}: {error}"))
                }
            })?;
        let closing = Arc::new(AtomicBool::new(false));
        let active = Arc::new(AtomicUsize::new(0));
        let (flag, count) = (Arc::clone(&closing), Arc::clone(&active));
        let accepting = thread::spawn(move || accept(&listener, &requests, &flag, &count));
        Ok(Self {
            closing,
            name,
            accepting: Some(accepting),
            active,
        })
    }

    pub async fn close(self, within: Duration) {
        self.closing.store(true, Ordering::SeqCst);
        let deadline = tokio::time::Instant::now() + within;
        while self.active.load(Ordering::SeqCst) > 0 && tokio::time::Instant::now() < deadline {
            tokio::time::sleep(CLOSE_POLL).await;
        }
    }
}

fn accept(
    listener: &PipeListener<pipe_mode::Bytes, pipe_mode::Bytes>,
    requests: &mpsc::UnboundedSender<Incoming>,
    closing: &AtomicBool,
    active: &Arc<AtomicUsize>,
) {
    while !closing.load(Ordering::SeqCst) {
        let Ok(stream) = listener.accept() else {
            thread::sleep(Duration::from_millis(100));
            continue;
        };
        if closing.load(Ordering::SeqCst) {
            return;
        }
        if active.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
            active.fetch_sub(1, Ordering::SeqCst);
            continue;
        }
        let requests = requests.clone();
        let active = Arc::clone(active);
        thread::spawn(move || {
            serve(&stream, &requests);
            active.fetch_sub(1, Ordering::SeqCst);
        });
    }
}

/// serves one connection; the pipe crate's blocking calls have no timeout, so a
/// client that sends nothing holds this thread until it disconnects.
fn serve(stream: &Stream, requests: &mpsc::UnboundedSender<Incoming>) {
    let mut reader = BufReader::new(stream);
    let Ok(Some(line)) = read_line_limited(&mut reader, MAX_REQUEST_BYTES) else {
        return;
    };
    let response = match submit(&line, requests) {
        Ok(answer) => answer
            .blocking_recv()
            .unwrap_or_else(|_| Response::ending()),
        Err(refusal) => refusal,
    };
    let mut writer = stream;
    let _ = writer
        .write_all(response.encode().as_bytes())
        .and_then(|()| writer.flush());
}

/// connects to this user's pipe.
pub fn connect(roots: &Roots) -> Result<Stream, ClientError> {
    let sid = current_user_sid().map_err(|error| ClientError::Failed(error.to_string()))?;
    let name = roots.control_pipe_name(&sid);
    Stream::connect_by_path_with_wait_mode(
        OsStr::new(&name),
        ConnectWaitMode::Timeout(CONNECT_TIMEOUT),
    )
    .map_err(|error| match error.kind() {
        io::ErrorKind::NotFound | io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock => {
            ClientError::NoInstance
        }
        _ => ClientError::Failed(error.to_string()),
    })
}
