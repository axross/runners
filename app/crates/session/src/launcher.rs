//! starting the agent: the app's own executable, by its absolute path, in agent mode, as a
//! plain detached child with no console window, so it outlives the app. Nothing goes
//! through Tauri's shell plugin.

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Arc;

use marshal_cli::agent::Launch;
use marshal_cli::agent::executable::{Executable, Look};

use crate::seam::{Launcher, Log};

/// the process creation flags of the agent on Windows: detached from the app's console,
/// if it has one, with no console window of its own, and in a process group of its own,
/// so ending the app takes nothing else with it.
pub const DETACHED_PROCESS: u32 = 0x0000_0008;
pub const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;
pub const AGENT_CREATION_FLAGS: u32 =
    DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW;

/// the app's own executable, as it was when the app started.
pub struct OwnExecutable {
    path: PathBuf,
    /// the file as the app started from it; `None` when it could not be read then.
    started_from: Option<Executable>,
    log: Arc<dyn Log>,
    /// variables the child is given besides this process's own; only a test sets any.
    environment: Vec<(OsString, OsString)>,
}

impl OwnExecutable {
    /// the executable this process runs from.
    pub fn current(log: Arc<dyn Log>) -> Result<Self, String> {
        let path = std::env::current_exe()
            .map_err(|error| format!("the app's own executable cannot be found: {error}"))?;
        Ok(Self::at(path, log))
    }

    #[must_use]
    pub fn at(path: PathBuf, log: Arc<dyn Log>) -> Self {
        Self {
            started_from: Executable::at(&path),
            path,
            log,
            environment: Vec::new(),
        }
    }

    /// gives the agent these variables as well, for a test that moves it to a scratch
    /// host.
    #[must_use]
    pub fn with_environment(mut self, variables: Vec<(OsString, OsString)>) -> Self {
        self.environment = variables;
        self
    }

    /// whether the file is still the one the app started from; a missing or changed file
    /// is never started.
    fn check(&self) -> Result<(), String> {
        let word = |what: &str| format!("the executable {} {what}", self.path.display());
        match &self.started_from {
            Some(file) => match file.look() {
                Look::Same => Ok(()),
                Look::Replaced => Err(word("has changed since the app started")),
                Look::Unreadable => Err(word("cannot be read")),
            },
            None if self.path.is_file() => Ok(()),
            None => Err(word("is missing")),
        }
    }
}

impl Launcher for OwnExecutable {
    fn start_agent(&self, launch: Launch) -> Result<(), String> {
        self.check()?;
        let mut command = Command::new(&self.path);
        command
            .args(marshal_modes::agent_arguments(launch))
            .envs(self.environment.iter().map(|(name, value)| (name, value)))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        detach(&mut command);
        let mut child = command
            .spawn()
            .map_err(|error| format!("the process could not be started: {error}"))?;
        let log = Arc::clone(&self.log);
        // the agent's end is logged, and the process is reaped
        std::thread::spawn(move || match child.wait() {
            Ok(status) => log.info(&format!("The agent process ended: {status}.")),
            Err(error) => log.warn(&format!(
                "The agent process could not be waited for: {error}"
            )),
        });
        Ok(())
    }
}

#[cfg(windows)]
fn detach(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    command.creation_flags(AGENT_CREATION_FLAGS);
}

#[cfg(unix)]
fn detach(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
}
