//! what runs inside a booted VM through its guest agent. `tart exec`
//! commands run as the agent's user, `admin`, whose `docker` group the golden VM
//! gives it, so `docker` needs no `sudo`; every captured answer is cut at
//! [`MAX_OUTPUT_BYTES`](super::MAX_OUTPUT_BYTES) and read as data.

use std::time::Duration;

use tokio::process::Child;

use crate::golden::pins::LAUNCHER_PATH;
use crate::runtime::scrub::scrub_error_text;

use super::client::{TartClient, TartError};

/// the guest agent of one VM.
#[derive(Debug, Clone, Copy)]
pub struct Guest<'a> {
    client: &'a TartClient,
    vm: &'a str,
}

impl<'a> Guest<'a> {
    #[must_use]
    pub fn new(client: &'a TartClient, vm: &'a str) -> Self {
        Self { client, vm }
    }

    /// whether the agent runs a command and answers within `budget`; a VM whose agent
    /// is not up yet fails the call, which is no answer, and the error is Tart's text
    /// on one line, cut.
    pub async fn ask(&self, budget: Duration) -> Result<(), String> {
        let command = ["true".to_string()];
        let answer = self
            .client
            .exec(self.vm, &command, None, budget)
            .await
            .map_err(|error| scrub_error_text(&error.to_string(), ""))?;
        if answer.succeeded() {
            return Ok(());
        }
        Err(answer.error_text())
    }

    /// starts the launcher in the VM with the arguments of a `docker run`, for a
    /// command that lasts as long as the job; the JIT configuration goes to the
    /// returned child's standard input, which the caller then closes, and is never
    /// an argument.
    pub fn launch(&self, arguments: &[String]) -> Result<Child, TartError> {
        let mut command = vec![LAUNCHER_PATH.to_string()];
        command.extend(arguments.iter().cloned());
        self.client.spawn_exec(self.vm, &command)
    }
}
