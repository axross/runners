//! the host prerequisite checks: Tart's version, and Softnet's file and version. Each
//! returns a pass, a warning or a refusal with its message. The caller decides what a
//! refusal does: at `start` and `build` every refusal ends the command, and after
//! `start` a missing or unanswering `tart` means Tart is unavailable, while a refused
//! `softnet` holds its slot. `docs/operations/cli.md` describes the checks.

use std::ffi::OsStr;
use std::time::Duration;

use crate::runtime::scrub::scrub_error_text;
use crate::runtime::tart::args::version_arguments;
use crate::runtime::tart::client::{Captured, MAX_OUTPUT_BYTES, TartClient, TartError};

use super::softnet::{
    FileKind, OwnerMode, SetupProblem, capture_version, find_on_path,
    owner_mode_override_from_environment, read_file_facts, setup_problem,
};
use super::version::{
    Standing, TESTED_SOFTNET, TESTED_TART, Version, read_softnet_output, read_tart_output, standing,
};

/// how long each version read gets.
pub const VERSION_BUDGET: Duration = Duration::from_secs(5);

/// the host document the messages send the operator to.
pub const HOST_DOCUMENT: &str = "macOS Runner Host";

/// the host document's section on installing Tart and Softnet at the tested versions.
pub const INSTALL_SECTION: &str = "Installing Tart and Softnet";

/// the host document's section on Softnet's SUID bit, owner and permissions.
pub const SOFTNET_SECTION: &str = "Setting Up Softnet";

/// the host document's section on the signed-in session, which a boot failure names.
pub const SIGNED_IN_SECTION: &str = "The Signed-In Session";

/// why a check refused; the caller maps each to what it does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cause {
    /// the program is not on `PATH` or cannot be started.
    Missing,
    /// the program did not answer `--version` within its bound.
    NotAnswering,
    /// the program answered, and its version is not one that runs.
    Version,
    /// Softnet's file is not set up as the host document says.
    Setup,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    pub cause: Cause,
    pub message: String,
}

/// the result of one check. `version` is the text to record in the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Check {
    Pass { version: String },
    Warn { version: String, message: String },
    Refuse(Refusal),
}

impl Check {
    /// the version and the warning, if any, of a program that runs, or the refusal;
    /// each caller keeps its own policy for the refusals it can wait out.
    pub fn into_result(self) -> Result<(String, Option<String>), Refusal> {
        match self {
            Self::Pass { version } => Ok((version, None)),
            Self::Warn { version, message } => Ok((version, Some(message))),
            Self::Refuse(refusal) => Err(refusal),
        }
    }
}

fn refuse(cause: Cause, message: String) -> Check {
    Check::Refuse(Refusal { cause, message })
}

pub(crate) fn section_of_document(title: &str) -> String {
    format!("the \"{title}\" section of the {HOST_DOCUMENT} document")
}

struct Read {
    number: Version,
    recorded: String,
}

struct Tool {
    name: &'static str,
    command: &'static str,
    tested: Version,
    form: &'static str,
    read: fn(&str) -> Option<Read>,
    after_no_answer: Option<&'static str>,
}

const TART: Tool = Tool {
    name: "Tart",
    command: "tart",
    tested: TESTED_TART,
    form: "exactly three dot-separated numbers",
    read: |output| {
        read_tart_output(output).map(|number| Read {
            number,
            recorded: number.to_string(),
        })
    },
    after_no_answer: None,
};

const SOFTNET: Tool = Tool {
    name: "Softnet",
    command: "softnet",
    tested: TESTED_SOFTNET,
    form: "\"softnet \" and exactly three dot-separated numbers, a hyphen and a short commit",
    read: |output| {
        read_softnet_output(output).map(|version| Read {
            number: version.number,
            recorded: version.to_string(),
        })
    },
    after_no_answer: Some(SOFTNET_SECTION),
};

impl Tool {
    fn install_advice(&self) -> String {
        format!(
            "Install {} {} as {} describes.",
            self.name,
            self.tested,
            section_of_document(INSTALL_SECTION)
        )
    }

    fn judge(&self, answer: Result<Captured, TartError>, budget: Duration) -> Check {
        let captured = match answer {
            Ok(captured) => captured,
            Err(TartError::TimedOut(_)) => return self.not_answering(budget),
            Err(error) => {
                return refuse(
                    Cause::Missing,
                    format!(
                        "{} could not be started ({}). {}",
                        self.name,
                        scrub_error_text(&error.to_string(), ""),
                        self.install_advice()
                    ),
                );
            }
        };
        if !captured.succeeded() {
            return refuse(
                Cause::Version,
                format!(
                    "\"{} --version\" failed: {}. {}",
                    self.command,
                    captured.error_text(),
                    self.install_advice()
                ),
            );
        }
        let read = if captured.stdout_cut {
            None
        } else {
            (self.read)(&captured.stdout)
        };
        let Some(read) = read else {
            return refuse(Cause::Version, self.unreadable(&captured));
        };
        self.sort(&read)
    }

    fn not_answering(&self, budget: Duration) -> Check {
        let mut message = format!(
            "\"{} --version\" did not answer within {} s.",
            self.command,
            budget.as_secs()
        );
        if let Some(section) = self.after_no_answer {
            message.push_str(&format!(
                " Check {} as {} describes.",
                self.name,
                section_of_document(section)
            ));
        }
        refuse(Cause::NotAnswering, message)
    }

    fn unreadable(&self, captured: &Captured) -> String {
        let printed = scrub_error_text(&captured.stdout, "");
        let printed = if printed.is_empty() {
            "nothing".to_string()
        } else if captured.stdout_cut {
            format!(
                "more than {} bytes, beginning \"{printed}\"",
                MAX_OUTPUT_BYTES
            )
        } else {
            format!("\"{printed}\"")
        };
        format!(
            "\"{} --version\" printed {printed}, which is not {}. {}",
            self.command,
            self.form,
            self.install_advice()
        )
    }

    fn sort(&self, read: &Read) -> Check {
        let (name, found, tested) = (self.name, read.number, self.tested);
        let version = read.recorded.clone();
        match standing(found, tested) {
            Standing::Tested => Check::Pass { version },
            Standing::NewerPatch => Check::Warn {
                version,
                message: format!(
                    "{name} {found} is a newer patch of the tested version {tested}; it runs, but only {tested} was tested."
                ),
            },
            Standing::OlderPatch => refuse(
                Cause::Version,
                format!(
                    "{name} {found} is older than the tested version {tested}. {}",
                    self.install_advice()
                ),
            ),
            Standing::OtherRelease => refuse(
                Cause::Version,
                format!(
                    "{name} {found} is not the tested version {tested} or a newer patch of {}.{}. {}",
                    tested.major,
                    tested.minor,
                    self.install_advice()
                ),
            ),
        }
    }
}

/// reads `tart --version` through `client`, within [`VERSION_BUDGET`].
pub async fn check_tart(client: &TartClient) -> Check {
    check_tart_within(client, VERSION_BUDGET).await
}

pub async fn check_tart_within(client: &TartClient, budget: Duration) -> Check {
    let answer = client.capture(&version_arguments(), None, budget).await;
    TART.judge(answer, budget)
}

/// checks the `softnet` that Tart will run, which is the first on `path`: its file,
/// then its version within [`VERSION_BUDGET`]. A debug build takes the file's owner
/// and mode from `MARSHAL_TEST_SOFTNET_OWNER_MODE` when it is set.
pub async fn check_softnet(path: Option<&OsStr>) -> Check {
    match owner_mode_override_from_environment() {
        Ok(owner_mode) => check_softnet_with(path, owner_mode, VERSION_BUDGET).await,
        Err(message) => refuse(Cause::Setup, message),
    }
}

/// [`check_softnet`] with the owner and mode override and the bound named. The file is
/// judged before the program runs, so a file that fails the check never starts.
pub async fn check_softnet_with(
    path: Option<&OsStr>,
    owner_mode_override: Option<OwnerMode>,
    budget: Duration,
) -> Check {
    let Some(found) = find_on_path(path) else {
        return refuse(
            Cause::Missing,
            format!(
                "Softnet was not found on PATH. {}",
                SOFTNET.install_advice()
            ),
        );
    };
    let shown = found.display();
    let setup = section_of_document(SOFTNET_SECTION);
    let facts = match read_file_facts(&found, owner_mode_override) {
        Ok(facts) => facts,
        Err(error) => {
            return refuse(
                Cause::Setup,
                format!(
                    "Softnet at {shown} cannot be read ({error}). Set it up as {setup} describes."
                ),
            );
        }
    };
    if let Some(problem) = setup_problem(facts) {
        return refuse(
            Cause::Setup,
            format!(
                "Softnet at {shown} {}. Set it up as {setup} describes.",
                problem_text(problem)
            ),
        );
    }
    SOFTNET.judge(capture_version(&found, path, budget).await, budget)
}

fn problem_text(problem: SetupProblem) -> String {
    match problem {
        SetupProblem::NotRegular(FileKind::Directory) => {
            "is a directory, not a regular file".to_string()
        }
        SetupProblem::NotRegular(_) => "is not a regular file".to_string(),
        SetupProblem::NotOwnedByRoot { uid } => {
            format!("is owned by user {uid}, not root, so it cannot run with root privilege")
        }
        SetupProblem::NoSuidBit => {
            "does not have the SUID bit set, so it cannot run with root privilege".to_string()
        }
        SetupProblem::GroupWritable => "can be written by its group".to_string(),
        SetupProblem::OtherWritable => "can be written by others".to_string(),
    }
}
