//! the modes the one executable runs in. Its first argument decides the mode before
//! the app starts: the resident agent, the diagnostics exporter the agent starts, the
//! shim on Unix, or otherwise the app. The hidden modes run the `marshal-cli` library's
//! entry points, have no interface and promise no compatibility.

use std::ffi::OsString;
use std::process::ExitCode;

use marshal_cli::agent::{AgentOptions, Launch};

/// the first argument of the agent's mode, followed by [`RUNNING`] or [`STOPPED`].
pub const AGENT_ARGUMENT: &str = "agent";

/// the agent runs the pools as soon as it starts.
pub const RUNNING: &str = "running";

/// the agent waits to be told to run the pools.
pub const STOPPED: &str = "stopped";

const EXIT_USAGE: u8 = 2;

/// the arguments that start the agent from this executable.
#[must_use]
pub fn agent_arguments(launch: Launch) -> [&'static str; 2] {
    match launch {
        Launch::Running => [AGENT_ARGUMENT, RUNNING],
        Launch::Stopped => [AGENT_ARGUMENT, STOPPED],
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    App,
    Agent(Launch),
    /// a hidden command of the library, with the program name first.
    Command(Vec<OsString>),
    /// arguments that name a hidden mode but not a valid one.
    Refused,
}

impl Mode {
    /// the mode for a process's arguments, the program name first.
    pub fn of(args: impl IntoIterator<Item = OsString>) -> Self {
        let args: Vec<OsString> = args.into_iter().collect();
        let mut rest = args.iter().skip(1);
        let Some(first) = rest.next() else {
            return Self::App;
        };
        if first == AGENT_ARGUMENT {
            return match (rest.next(), rest.next()) {
                (Some(word), None) if word == RUNNING => Self::Agent(Launch::Running),
                (Some(word), None) if word == STOPPED => Self::Agent(Launch::Stopped),
                _ => Self::Refused,
            };
        }
        if is_command(first) {
            return Self::Command(args);
        }
        Self::App
    }
}

fn is_command(word: &OsString) -> bool {
    #[cfg(unix)]
    if word == marshal_cli::shim::SHIM_COMMAND {
        return true;
    }
    word == marshal_cli::diagnostics::EXPORT_COMMAND
}

/// runs a hidden mode and returns the process's exit code; the app mode is not run
/// here.
#[must_use]
pub fn run(mode: Mode) -> ExitCode {
    match mode {
        Mode::Agent(launch) => ExitCode::from(marshal_cli::agent::run(
            AgentOptions::new(launch),
            &mut std::io::stderr(),
        )),
        Mode::Command(args) => {
            marshal_cli::command::run(args, &mut std::io::stdout(), &mut std::io::stderr())
        }
        Mode::App | Mode::Refused => ExitCode::from(EXIT_USAGE),
    }
}
