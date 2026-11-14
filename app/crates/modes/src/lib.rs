//! the modes the one executable runs in. Its first argument decides the mode before
//! the app starts: the resident agent, the diagnostics exporter the agent starts, the
//! shims on Unix, or otherwise the app. The hidden modes run the `marshal-cli` library's
//! entry points, have no interface and promise no compatibility.

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;

use marshal_cli::agent::{AgentOptions, Launch};

/// the first argument of the agent's mode, followed by [`RUNNING`] or [`STOPPED`].
pub const AGENT_ARGUMENT: &str = "agent";

/// the agent runs the pools as soon as it starts.
pub const RUNNING: &str = "running";

/// the agent waits to be told to run the pools.
pub const STOPPED: &str = "stopped";

const EXIT_USAGE: u8 = 2;

/// the variables a debug build's agent reads to run against a scratch host, which are
/// the ones the command line's `start` reads.
pub use marshal_cli::command::{
    TEST_DOCKER_VARIABLE, TEST_GITHUB_API_VARIABLE, TEST_ROOT_VARIABLE,
};

/// what a test moves in the agent: where its lock, channel, log and configuration are,
/// which `docker` it runs and which GitHub address it calls. Only a debug build reads
/// them, as the library's agent options do.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Overrides {
    pub root: Option<PathBuf>,
    pub docker: Option<OsString>,
    pub github_api: Option<String>,
}

impl Overrides {
    /// the overrides in this process's environment; none in a release build.
    #[must_use]
    pub fn from_environment() -> Self {
        Self::from_lookup(|name| {
            if cfg!(debug_assertions) {
                std::env::var_os(name)
            } else {
                None
            }
        })
    }

    #[must_use]
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<OsString>) -> Self {
        Self {
            root: lookup(TEST_ROOT_VARIABLE).map(PathBuf::from),
            docker: lookup(TEST_DOCKER_VARIABLE),
            github_api: lookup(TEST_GITHUB_API_VARIABLE).and_then(|value| value.into_string().ok()),
        }
    }

    fn options(&self, launch: Launch) -> AgentOptions<'_> {
        let mut options = AgentOptions::new(launch);
        options.test_root = self.root.as_deref();
        options.docker = self.docker.as_deref();
        options.github_api = self.github_api.as_deref();
        options
    }
}

/// the argument the sign-in item starts the app with, which hides its window.
pub use marshal_cli::windows_host::run_key::SIGN_IN_ARGUMENT;

/// whether a process's arguments, the program name first, start the app from the
/// sign-in item, with its icon and without its window.
pub fn is_sign_in(args: impl IntoIterator<Item = OsString>) -> bool {
    args.into_iter()
        .skip(1)
        .any(|argument| argument == SIGN_IN_ARGUMENT)
}

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
    if word == marshal_cli::shim::SHIM_COMMAND || word == marshal_cli::vm_shim::VM_SHIM_COMMAND {
        return true;
    }
    word == marshal_cli::diagnostics::EXPORT_COMMAND
}

/// runs a hidden mode and returns the process's exit code; the app mode is not run
/// here.
#[must_use]
pub fn run(mode: Mode) -> ExitCode {
    match mode {
        Mode::Agent(launch) => {
            let overrides = Overrides::from_environment();
            let mut options = overrides.options(launch);
            options.keep_awake = keep_awake_request();
            ExitCode::from(marshal_cli::agent::run(options, &mut std::io::stderr()))
        }
        Mode::Command(args) => {
            marshal_cli::command::run(args, &mut std::io::stdout(), &mut std::io::stderr())
        }
        Mode::App | Mode::Refused => ExitCode::from(EXIT_USAGE),
    }
}

/// asks Windows to keep the system, not the display, awake until the returned guard
/// is dropped.
#[cfg(windows)]
fn keep_awake_request() -> Option<marshal_cli::windows_host::keep_awake::Request> {
    Some(Box::new(|| {
        keepawake::Builder::default()
            .idle(true)
            .reason("Marshal's runners are running")
            .app_name("Marshal")
            .create()
            .map(|held| Box::new(held) as Box<dyn std::any::Any>)
            .map_err(|error| error.to_string())
    }))
}

#[cfg(not(windows))]
fn keep_awake_request() -> Option<marshal_cli::windows_host::keep_awake::Request> {
    None
}
