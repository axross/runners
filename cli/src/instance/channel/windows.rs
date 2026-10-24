//! The control pipe.

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
use tokio::sync::{mpsc, oneshot};

use super::super::protocol::{Incoming, MAX_REQUEST_BYTES, Request, Response, read_line_limited};
use super::super::roots::Roots;
use super::super::windows::{current_user_sid, name_is_taken, only_user_descriptor};
use super::{CONNECT_TIMEOUT, ChannelError, ClientError, MAX_CONNECTIONS};

type Stream = DuplexPipeStream<pipe_mode::Bytes>;

/// How long closing the server waits to connect to its own pipe.
const WAKE_TIMEOUT: Duration = Duration::from_millis(500);

/// The listening pipe. Dropping it stops the accepting thread, and once that thread
/// has ended no new client can reach the pipe.
#[derive(Debug)]
pub struct Server {
    closing: Arc<AtomicBool>,
    name: String,
    accepting: Option<JoinHandle<()>>,
}

impl Drop for Server {
    fn drop(&mut self) {
        self.closing.store(true, Ordering::SeqCst);
        // accept() returns only when a client connects, and it skips a client that is
        // already gone, so the waking connection stays open until the thread ends
        let wake = Stream::connect_by_path_with_wait_mode(
            OsStr::new(&self.name),
            ConnectWaitMode::Timeout(WAKE_TIMEOUT),
        );
        if wake.is_ok()
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
        let flag = Arc::clone(&closing);
        let accepting = thread::spawn(move || accept(&listener, &requests, &flag));
        Ok(Self {
            closing,
            name,
            accepting: Some(accepting),
        })
    }
}

fn accept(
    listener: &PipeListener<pipe_mode::Bytes, pipe_mode::Bytes>,
    requests: &mpsc::UnboundedSender<Incoming>,
    closing: &AtomicBool,
) {
    let active = Arc::new(AtomicUsize::new(0));
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
        let active = Arc::clone(&active);
        thread::spawn(move || {
            serve(&stream, &requests);
            active.fetch_sub(1, Ordering::SeqCst);
        });
    }
}

/// Serves one connection. The pipe crate's blocking calls have no timeout, so a
/// client that connects and sends nothing holds this thread until it disconnects.
fn serve(stream: &Stream, requests: &mpsc::UnboundedSender<Incoming>) {
    let mut reader = BufReader::new(stream);
    let Ok(Some(line)) = read_line_limited(&mut reader, MAX_REQUEST_BYTES) else {
        return;
    };
    let response = match Request::parse(&line) {
        Ok(request) => dispatch(request, requests),
        Err(error) => Response::refused(&error.to_string()),
    };
    let mut writer = stream;
    let _ = writer
        .write_all(response.encode().as_bytes())
        .and_then(|()| writer.flush());
}

fn dispatch(request: Request, requests: &mpsc::UnboundedSender<Incoming>) -> Response {
    let (reply, answer) = oneshot::channel();
    if requests.send(Incoming { request, reply }).is_err() {
        return Response::refused("the instance is ending");
    }
    answer
        .blocking_recv()
        .unwrap_or_else(|_| Response::refused("the instance is ending"))
}

/// Connects to this user's pipe.
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
