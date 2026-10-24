//! A loopback stand-in for the GitHub API, one scripted reply per connection.
#![allow(
    dead_code,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

/// What the stub does with one connection.
pub enum Reply {
    /// A status line, extra headers, and a body.
    Respond {
        status: &'static str,
        headers: Vec<(&'static str, String)>,
        body: Vec<u8>,
    },
    /// Reads the request and never answers.
    Stall,
    /// Answers 200 with a chunked body of this many bytes.
    Chunked(usize),
    /// Redirects to another path on the stub itself, so a client that follows it
    /// reaches the stub's next reply.
    RedirectToSelf,
}

impl Reply {
    pub fn json(body: &str) -> Self {
        Self::Respond {
            status: "200 OK",
            headers: vec![("Content-Type", "application/json".to_string())],
            body: body.as_bytes().to_vec(),
        }
    }

    pub fn status(status: &'static str, body: &str) -> Self {
        Self::Respond {
            status,
            headers: Vec::new(),
            body: body.as_bytes().to_vec(),
        }
    }
}

/// One request the stub read.
#[derive(Debug, Clone)]
pub struct Recorded {
    pub method: String,
    pub path: String,
    /// Header names in lower case.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Recorded {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(have, _)| have == name)
            .map(|(_, value)| value.as_str())
    }
}

pub struct Stub {
    pub address: String,
    requests: Arc<Mutex<Vec<Recorded>>>,
    done: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Stub {
    pub fn start(replies: Vec<Reply>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let done = Arc::new(AtomicBool::new(false));
        let thread = {
            let (requests, done) = (requests.clone(), done.clone());
            let own = address.clone();
            std::thread::spawn(move || serve(&listener, &own, replies, &requests, &done))
        };
        Self {
            address,
            requests,
            done,
            thread: Some(thread),
        }
    }

    pub fn requests(&self) -> Vec<Recorded> {
        self.requests.lock().unwrap().clone()
    }
}

impl Drop for Stub {
    fn drop(&mut self) {
        self.done.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn serve(
    listener: &TcpListener,
    own_address: &str,
    replies: Vec<Reply>,
    requests: &Mutex<Vec<Recorded>>,
    done: &AtomicBool,
) {
    let mut held: Vec<TcpStream> = Vec::new();
    for reply in replies {
        let Some(mut stream) = accept(listener, done) else {
            return;
        };
        stream.set_nonblocking(false).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let Some(recorded) = read_request(&mut stream) else {
            continue;
        };
        requests.lock().unwrap().push(recorded);
        match reply {
            Reply::Stall => held.push(stream),
            Reply::RedirectToSelf => {
                let head = format!(
                    "HTTP/1.1 307 Temporary Redirect\r\nLocation: {own_address}/followed\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                );
                let _ = stream.write_all(head.as_bytes());
            }
            Reply::Respond {
                status,
                headers,
                body,
            } => {
                let mut head = format!(
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n",
                    body.len()
                );
                for (name, value) in headers {
                    head.push_str(&format!("{name}: {value}\r\n"));
                }
                head.push_str("\r\n");
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(&body);
            }
            Reply::Chunked(total) => {
                let _ = stream.write_all(
                    b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n",
                );
                let chunk = vec![b'a'; 16 * 1024];
                let mut sent = 0;
                while sent < total {
                    let size = chunk.len().min(total - sent);
                    let head = format!("{size:x}\r\n");
                    if stream.write_all(head.as_bytes()).is_err()
                        || stream.write_all(&chunk[..size]).is_err()
                        || stream.write_all(b"\r\n").is_err()
                    {
                        break;
                    }
                    sent += size;
                }
                let _ = stream.write_all(b"0\r\n\r\n");
            }
        }
    }
    while !done.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(10));
    }
    drop(held);
}

fn accept(listener: &TcpListener, done: &AtomicBool) -> Option<TcpStream> {
    loop {
        match listener.accept() {
            Ok((stream, _)) => return Some(stream),
            Err(_) if done.load(Ordering::SeqCst) => return None,
            Err(_) => std::thread::sleep(Duration::from_millis(5)),
        }
    }
}

fn read_request(stream: &mut TcpStream) -> Option<Recorded> {
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 4096];
    let head_end = loop {
        let read = stream.read(&mut buffer).ok()?;
        if read == 0 {
            return None;
        }
        bytes.extend_from_slice(&buffer[..read]);
        if let Some(position) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break position + 4;
        }
    };
    let head = String::from_utf8_lossy(&bytes[..head_end]).into_owned();
    let mut lines = head.split("\r\n");
    let mut request_line = lines.next()?.split(' ');
    let method = request_line.next()?.to_string();
    let path = request_line.next()?.to_string();
    let headers: Vec<(String, String)> = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_string()))
        .collect();
    let length: usize = headers
        .iter()
        .find(|(name, _)| name == "content-length")
        .and_then(|(_, value)| value.parse().ok())
        .unwrap_or(0);
    let mut body = bytes[head_end..].to_vec();
    while body.len() < length {
        let read = stream.read(&mut buffer).ok()?;
        if read == 0 {
            break;
        }
        body.extend_from_slice(&buffer[..read]);
    }
    Some(Recorded {
        method,
        path,
        headers,
        body,
    })
}
