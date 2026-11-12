//! another process that holds the agent's channel: it accepts connections and either
//! says nothing or says something that is not protocol version 2.
#![allow(dead_code)]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use marshal_cli::instance::roots::Roots;
use tokio::io::AsyncWriteExt;
use tokio::task::JoinHandle;

#[derive(Debug, Clone, Copy)]
pub enum Behaviour {
    /// accepts and never answers.
    Silent,
    /// answers with a line that is not the agent's welcome.
    NotProtocol,
}

const NOT_A_WELCOME: &[u8] = b"220 another program says hello\n";

pub struct Squatter {
    accepted: Arc<AtomicUsize>,
    task: JoinHandle<()>,
}

impl Squatter {
    /// how many connections have been accepted.
    pub fn accepted(&self) -> usize {
        self.accepted.load(Ordering::SeqCst)
    }
}

impl Drop for Squatter {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[cfg(unix)]
pub fn squat(roots: &Roots, behaviour: Behaviour) -> Squatter {
    let uid = marshal_cli::private::current_uid().unwrap();
    marshal_cli::private::ensure_private_directory(roots.channel_directory(), uid).unwrap();
    let listener = tokio::net::UnixListener::bind(roots.socket_path()).unwrap();
    let accepted = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&accepted);
    let task = tokio::spawn(async move {
        let mut held = Vec::new();
        while let Ok((mut stream, _)) = listener.accept().await {
            counted.fetch_add(1, Ordering::SeqCst);
            if matches!(behaviour, Behaviour::NotProtocol) {
                let _ = stream.write_all(NOT_A_WELCOME).await;
            }
            held.push(stream);
        }
    });
    Squatter { accepted, task }
}

#[cfg(windows)]
pub fn squat(roots: &Roots, behaviour: Behaviour) -> Squatter {
    use interprocess::os::windows::named_pipe::pipe_mode;

    let (options, _) = marshal_cli::instance::windows::control_pipe_options(roots).unwrap();
    let listener = options.create_tokio_duplex::<pipe_mode::Bytes>().unwrap();
    let accepted = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&accepted);
    let task = tokio::spawn(async move {
        let mut held = Vec::new();
        while let Ok(mut stream) = listener.accept().await {
            counted.fetch_add(1, Ordering::SeqCst);
            if matches!(behaviour, Behaviour::NotProtocol) {
                let _ = stream.write_all(NOT_A_WELCOME).await;
            }
            held.push(stream);
        }
    });
    Squatter { accepted, task }
}
