//! the keep-awake hold: a `caffeinate -i -w <this process's ID>` child that stops the
//! Mac from idle sleep while it runs. `-w` ends the child when this process ends,
//! even when it is killed hard.

use std::cell::RefCell;
use std::ffi::OsString;
use std::time::{Duration, Instant};

use tokio::process::Child;

use super::probe::{resolve_program, system_command};

/// the program a release build runs, by absolute path.
pub const CAFFEINATE_PROGRAM: &str = "/usr/bin/caffeinate";

/// the variable that names `caffeinate` in a debug build; a release build ignores it.
pub const TEST_CAFFEINATE_VARIABLE: &str = "MARSHAL_TEST_CAFFEINATE";

/// how long after a failed start, or an early end, the hold is tried again.
pub const RETRY_INTERVAL: Duration = Duration::from_secs(10);

/// what a start, or a check, found out about the hold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HoldEvent {
    Unchanged,
    /// the hold was lost or could not be taken: the text is the warning for the log
    /// and `status`, once per episode.
    Warned(String),
    /// the hold is held again after a warning.
    Recovered,
}

/// how to reach `caffeinate`.
#[derive(Debug, Clone)]
pub struct KeepAwakeConfig {
    pub program: OsString,
    pub environment: Vec<(OsString, OsString)>,
    pub retry_interval: Duration,
}

impl Default for KeepAwakeConfig {
    fn default() -> Self {
        Self {
            program: OsString::from(CAFFEINATE_PROGRAM),
            environment: Vec::new(),
            retry_interval: RETRY_INTERVAL,
        }
    }
}

impl KeepAwakeConfig {
    /// the `MARSHAL_TEST_CAFFEINATE` program in a debug build, else the system's.
    #[must_use]
    pub fn from_environment() -> Self {
        Self {
            program: resolve_program(
                std::env::var_os(TEST_CAFFEINATE_VARIABLE).as_deref(),
                CAFFEINATE_PROGRAM,
            ),
            ..Self::default()
        }
    }
}

enum Hold {
    Released,
    Held(Child),
    /// a retried `caffeinate` that has not yet lived to the next check, so it is not
    /// yet reported as holding.
    Retrying {
        child: Child,
        warning: String,
    },
    Failed {
        warning: String,
        retry_at: Instant,
    },
}

/// the hold. The instance takes it on entering running and keeps it through stopping,
/// polls it while it is wanted, and releases it on entering stopped.
pub struct KeepAwake {
    config: KeepAwakeConfig,
    hold: RefCell<Hold>,
}

impl KeepAwake {
    #[must_use]
    pub fn new(config: KeepAwakeConfig) -> Self {
        Self {
            config,
            hold: RefCell::new(Hold::Released),
        }
    }

    fn start(&self) -> Result<Child, String> {
        let arguments = [
            "-i".to_string(),
            "-w".to_string(),
            std::process::id().to_string(),
        ];
        system_command(&self.config.program, &self.config.environment, &arguments)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|error| warning_text(&format!("caffeinate could not start: {error}")))
    }

    fn failed(&self, warning: String) -> Hold {
        Hold::Failed {
            warning,
            retry_at: Instant::now() + self.config.retry_interval,
        }
    }

    /// takes the hold; it does nothing when the hold is wanted already. A start
    /// that fails warns and is retried by [`KeepAwake::poll`].
    pub fn hold(&self) -> HoldEvent {
        let mut hold = self.hold.borrow_mut();
        if !matches!(*hold, Hold::Released) {
            return HoldEvent::Unchanged;
        }
        match self.start() {
            Ok(child) => {
                *hold = Hold::Held(child);
                HoldEvent::Unchanged
            }
            Err(text) => {
                *hold = self.failed(text.clone());
                HoldEvent::Warned(text)
            }
        }
    }

    /// ends the hold and stops retrying it.
    pub fn release(&self) {
        *self.hold.borrow_mut() = Hold::Released;
    }

    /// checks that the hold still holds. A lost hold is retried once its retry
    /// interval has passed, and counts as recovered at the first check after the
    /// retry that finds its `caffeinate` still running.
    pub fn poll(&self) -> HoldEvent {
        let mut hold = self.hold.borrow_mut();
        let current = std::mem::replace(&mut *hold, Hold::Released);
        let (next, event) = self.advance(current);
        *hold = next;
        event
    }

    fn advance(&self, current: Hold) -> (Hold, HoldEvent) {
        match current {
            Hold::Released => (Hold::Released, HoldEvent::Unchanged),
            Hold::Held(mut child) => match child.try_wait() {
                Ok(None) => (Hold::Held(child), HoldEvent::Unchanged),
                Ok(Some(status)) => self.lost(&format!("caffeinate ended early ({status})")),
                Err(error) => self.lost(&format!("caffeinate could not be checked: {error}")),
            },
            Hold::Retrying { mut child, warning } => match child.try_wait() {
                Ok(None) => (Hold::Held(child), HoldEvent::Recovered),
                Ok(Some(status)) => (
                    self.failed(warning_text(&format!("caffeinate ended early ({status})"))),
                    HoldEvent::Unchanged,
                ),
                Err(_) => (self.failed(warning), HoldEvent::Unchanged),
            },
            Hold::Failed { warning, retry_at } if Instant::now() < retry_at => {
                (Hold::Failed { warning, retry_at }, HoldEvent::Unchanged)
            }
            Hold::Failed { warning, .. } => match self.start() {
                Ok(child) => (Hold::Retrying { child, warning }, HoldEvent::Unchanged),
                Err(text) => (self.failed(text), HoldEvent::Unchanged),
            },
        }
    }

    fn lost(&self, cause: &str) -> (Hold, HoldEvent) {
        let text = warning_text(cause);
        (self.failed(text.clone()), HoldEvent::Warned(text))
    }

    /// the warning while the hold is wanted and not held.
    #[must_use]
    pub fn warning(&self) -> Option<String> {
        match &*self.hold.borrow() {
            Hold::Failed { warning, .. } | Hold::Retrying { warning, .. } => Some(warning.clone()),
            _ => None,
        }
    }
}

fn warning_text(cause: &str) -> String {
    format!("keep-awake is not held ({cause}), so the Mac may sleep when idle; trying again")
}
