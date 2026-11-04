//! the agent's pipe listener: today's name and today's security descriptor, which
//! grants all access to the current user and to no one else, with remote clients
//! rejected, created as the first instance of the name so a squatted name is
//! detected; the pipe crate's asynchronous listener runs it on the agent's runtime.

use std::ffi::OsStr;
use std::io;

use interprocess::os::windows::named_pipe::tokio::{DuplexPipeStream, PipeListener};
use interprocess::os::windows::named_pipe::{PipeListenerOptions, PipeMode, pipe_mode};

use crate::instance::channel::ChannelError;
use crate::instance::roots::Roots;
use crate::instance::windows::{current_user_sid, name_is_taken, only_user_descriptor};

pub type Stream = DuplexPipeStream<pipe_mode::Bytes>;

pub struct Listener {
    listener: PipeListener<pipe_mode::Bytes, pipe_mode::Bytes>,
    name: String,
}

impl Listener {
    pub fn open(roots: &Roots) -> Result<Self, ChannelError> {
        let sid = current_user_sid().map_err(|error| ChannelError::Io(error.to_string()))?;
        let descriptor =
            only_user_descriptor(&sid).map_err(|error| ChannelError::Io(error.to_string()))?;
        let name = roots.control_pipe_name(&sid);
        // the options reject remote clients unless told otherwise, and create the
        // first instance of the name
        let listener = PipeListenerOptions::new()
            .path(OsStr::new(&name))
            .mode(PipeMode::Bytes)
            .security_descriptor(Some(descriptor))
            .create_tokio_duplex::<pipe_mode::Bytes>()
            .map_err(|error| {
                if name_is_taken(&error) {
                    ChannelError::NameTaken(name.clone())
                } else {
                    ChannelError::Io(format!("{name}: {error}"))
                }
            })?;
        Ok(Self { listener, name })
    }

    pub fn address(&self) -> String {
        self.name.clone()
    }

    pub async fn accept(&mut self) -> io::Result<Stream> {
        self.listener.accept().await
    }
}
