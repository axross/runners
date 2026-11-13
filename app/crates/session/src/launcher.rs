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

/// the Windows flag that detaches the agent from the app's console, if the app has one:
/// the agent then has no console at all, and so no console window.
#[cfg(any(windows, test))]
const DETACHED_PROCESS: u32 = 0x0000_0008;
/// the Windows flag that gives the agent a process group of its own, so ending the app
/// takes nothing else with it.
#[cfg(any(windows, test))]
const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;

/// the process creation flags of the agent on Windows. `CREATE_NO_WINDOW` is left out on
/// purpose: Windows ignores it beside `DETACHED_PROCESS`, which already leaves the agent
/// without a console. The `docker` clients and the exporter the agent starts get
/// `CREATE_NO_WINDOW` from the library instead, since each of them is a console program.
#[cfg(any(windows, test))]
const AGENT_CREATION_FLAGS: u32 = DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP;

/// the agent's `PATH` on macOS. An app opened from the Finder inherits launchd's short
/// `PATH`, without Homebrew's directory, and the agent passes its own on to Tart, which
/// finds `softnet` on it.
#[cfg(target_os = "macos")]
pub const AGENT_PATH: &str = "/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin";

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

    /// whether the file is still the one the app started from; a missing or changed file,
    /// or one the app could not read when it started, is never started.
    fn check(&self) -> Result<(), String> {
        let word = |what: &str| format!("the executable {} {what}", self.path.display());
        match &self.started_from {
            Some(file) => match file.look() {
                Look::Same => Ok(()),
                Look::Replaced => Err(word("has changed since the app started")),
                Look::Unreadable => Err(word("cannot be read")),
            },
            None if self.path.is_file() => Err(word("could not be read when the app started")),
            None => Err(word("is missing")),
        }
    }
}

impl OwnExecutable {
    /// the command that starts the agent: no input or output, on macOS the agent's
    /// `PATH`, the agent's variables, and the flags that detach it.
    fn agent_command(&self, launch: Launch) -> Command {
        let mut command = Command::new(&self.path);
        #[cfg(target_os = "macos")]
        command.env("PATH", AGENT_PATH);
        command
            .args(marshal_modes::agent_arguments(launch))
            .envs(self.environment.iter().map(|(name, value)| (name, value)))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        detach(&mut command);
        command
    }
}

impl Launcher for OwnExecutable {
    fn start_agent(&self, launch: Launch) -> Result<(), String> {
        self.check()?;
        let mut child = self
            .agent_command(launch)
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

/// detaches the agent from the app: its own process group on every platform, and on
/// Windows no console. A test build records the Windows flags on every platform,
/// because a command does not report the flags it carries.
fn detach(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(AGENT_CREATION_FLAGS);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(test)]
    applied::record(AGENT_CREATION_FLAGS);
}

/// the creation flags `detach` applied on this thread, for a test that cannot read
/// them back from a command.
#[cfg(test)]
mod applied {
    use std::cell::Cell;

    thread_local! {
        static LAST: Cell<Option<u32>> = const { Cell::new(None) };
    }

    pub(super) fn record(flags: u32) {
        LAST.with(|last| last.set(Some(flags)));
    }

    pub(super) fn take() -> Option<u32> {
        LAST.with(Cell::take)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use marshal_cli::agent::Launch;

    use super::{OwnExecutable, applied};
    use crate::seam::Log;

    struct NoLog;
    impl Log for NoLog {
        fn info(&self, _: &str) {}
        fn warn(&self, _: &str) {}
    }

    #[test]
    fn on_windows_the_agent_has_no_console_and_a_process_group_of_its_own() {
        let launcher = OwnExecutable::at(PathBuf::from("marshal-app"), Arc::new(NoLog));
        applied::take();
        let _ = launcher.agent_command(Launch::Running);
        assert_eq!(
            applied::take(),
            Some(0x0000_0008 | 0x0000_0200),
            "DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP, and not CREATE_NO_WINDOW, which Windows ignores beside DETACHED_PROCESS"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn on_macos_the_agent_finds_homebrew_on_its_path() {
        let launcher = OwnExecutable::at(PathBuf::from("marshal-app"), Arc::new(NoLog));
        let command = launcher.agent_command(Launch::Running);
        let path = command
            .get_envs()
            .find(|(name, _)| *name == "PATH")
            .and_then(|(_, value)| value);
        assert_eq!(
            path.and_then(|value| value.to_str()),
            Some("/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin")
        );
    }
}
