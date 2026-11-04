//! the agent's pipe listener: today's name and today's security descriptor, which
//! grants all access to the current user and to no one else, with remote clients
//! rejected, created as the first instance of the name so a squatted name is
//! detected; the pipe crate's asynchronous listener runs it on the agent's runtime.

use std::io;

use interprocess::os::windows::named_pipe::pipe_mode;
use interprocess::os::windows::named_pipe::tokio::{DuplexPipeStream, PipeListener};

use crate::instance::channel::ChannelError;
use crate::instance::roots::Roots;
use crate::instance::windows::{control_pipe_options, creation_error};

pub type Stream = DuplexPipeStream<pipe_mode::Bytes>;

pub struct Listener {
    listener: PipeListener<pipe_mode::Bytes, pipe_mode::Bytes>,
    name: String,
}

impl Listener {
    pub fn open(roots: &Roots) -> Result<Self, ChannelError> {
        let (options, name) = control_pipe_options(roots)?;
        let listener = options
            .create_tokio_duplex::<pipe_mode::Bytes>()
            .map_err(|error| creation_error(&name, &error))?;
        Ok(Self { listener, name })
    }

    pub fn address(&self) -> String {
        self.name.clone()
    }

    pub async fn accept(&mut self) -> io::Result<Stream> {
        self.listener.accept().await
    }
}
