//! The command line: argument parsing, reading the file, printing, and the exit code.

use std::ffi::OsString;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Arg, ArgMatches, Command, value_parser};

use crate::checkout;
use crate::profile::Profile;
use crate::summary::plan_summary;
use crate::validate::{Failure, Plan, validate};

/// The largest configuration `validate` reads. Real files are a few kilobytes, so
/// this is generous; it only stops a mistaken or hostile path from being read whole.
pub const MAX_CONFIG_BYTES: u64 = 1024 * 1024;

const EXIT_INVALID: u8 = 1;
const EXIT_USAGE: u8 = 2;

fn command() -> Command {
    Command::new("runners")
        .about("Manages the self-hosted runner hosts in this repository")
        .subcommand_required(true)
        .arg_required_else_help(true)
        .subcommand(
            Command::new("validate")
                .about("Check a host configuration and print its plan")
                .arg(
                    Arg::new("config")
                        .long("config")
                        .value_name("PATH")
                        .required(true)
                        .value_parser(value_parser!(PathBuf))
                        .help("The host configuration file"),
                )
                .arg(
                    Arg::new("profile")
                        .long("profile")
                        .value_name("PROFILE")
                        .value_parser(["axpc", "axmc"])
                        .help("The host to check it for; defaults to axpc on Windows and axmc on macOS"),
                ),
        )
}

/// Runs the CLI with the given arguments, the first being the program name.
pub fn run(
    args: impl IntoIterator<Item = OsString>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> ExitCode {
    let matches = match command().try_get_matches_from(args) {
        Ok(matches) => matches,
        Err(error) => {
            let code = error.exit_code();
            let rendered = error.render().to_string();
            let written = if error.use_stderr() {
                write!(err, "{rendered}")
            } else {
                write!(out, "{rendered}")
            };
            return finish(written, u8::try_from(code).unwrap_or(EXIT_USAGE));
        }
    };
    match matches.subcommand() {
        Some(("validate", validate_matches)) => run_validate(validate_matches, out, err),
        _ => ExitCode::from(EXIT_USAGE),
    }
}

fn finish(written: io::Result<()>, code: u8) -> ExitCode {
    if written.is_err() {
        return ExitCode::from(EXIT_INVALID);
    }
    ExitCode::from(code)
}

fn run_validate(matches: &ArgMatches, out: &mut dyn Write, err: &mut dyn Write) -> ExitCode {
    let Some(config) = matches.get_one::<PathBuf>("config") else {
        return ExitCode::from(EXIT_USAGE);
    };
    let profile = matches
        .get_one::<String>("profile")
        .and_then(|name| Profile::from_name(name))
        .or_else(Profile::of_this_machine);
    let Some(profile) = profile else {
        return finish(
            writeln!(
                err,
                "error: --profile axpc or --profile axmc is required on this operating system, \
                 because the profile follows the OS only on Windows (axpc) and macOS (axmc)"
            ),
            EXIT_USAGE,
        );
    };
    match validate_file(config, profile, out, err) {
        Ok(code) => ExitCode::from(code),
        Err(_) => ExitCode::from(EXIT_INVALID),
    }
}

/// Reads the file, refusing one over [`MAX_CONFIG_BYTES`] without reading past the
/// limit.
fn read_limited(path: &Path) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(MAX_CONFIG_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_CONFIG_BYTES {
        return Err(io::Error::other(format!(
            "the file is larger than {MAX_CONFIG_BYTES} bytes (1 MiB)"
        )));
    }
    Ok(bytes)
}

/// Why a host configuration file gave no plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LoadError {
    /// The file is missing, cannot be read, or is not JSON; the message says which.
    Unreadable(String),
    /// The document breaks rules, listed in the order they were found.
    Invalid(Vec<String>),
}

impl LoadError {
    /// Every problem, one per entry.
    pub(crate) fn into_problems(self) -> Vec<String> {
        match self {
            Self::Unreadable(message) => vec![message],
            Self::Invalid(errors) => errors,
        }
    }
}

/// A host configuration file, read and validated against the checkout above it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Loaded {
    pub(crate) outcome: Result<Plan, LoadError>,
    /// True when no checkout was found above the file, so `diagnosticDirectory` was
    /// not compared with one.
    pub(crate) checkout_rule_skipped: bool,
}

/// Reads the file and validates it for `profile`.
pub(crate) fn load_plan(config: &Path, profile: Profile) -> Loaded {
    let bytes = match read_limited(config) {
        Ok(bytes) => bytes,
        Err(error) => {
            let message = if error.kind() == io::ErrorKind::NotFound {
                format!("Host configuration file not found: {}", config.display())
            } else {
                format!(
                    "Host configuration cannot be read: {}: {error}",
                    config.display()
                )
            };
            return Loaded {
                outcome: Err(LoadError::Unreadable(message)),
                checkout_rule_skipped: false,
            };
        }
    };
    let checkout = checkout::find(config);
    let validation = validate(&bytes, profile, checkout.as_deref());
    let outcome = validation.outcome.map_err(|failure| match failure {
        Failure::Unreadable(problem) => {
            LoadError::Unreadable(format!("Host configuration is not valid JSON: {problem}"))
        }
        Failure::Invalid(errors) => LoadError::Invalid(errors),
    });
    Loaded {
        outcome,
        checkout_rule_skipped: validation.checkout_rule_skipped,
    }
}

/// Validates the file and prints the outcome. Returns the exit code, or the error
/// that stopped output.
fn validate_file(
    config: &Path,
    profile: Profile,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> io::Result<u8> {
    let loaded = load_plan(config, profile);
    if loaded.checkout_rule_skipped {
        writeln!(
            err,
            "note: no checkout of this project was found above {}, so diagnosticDirectory was not compared with it",
            config.display()
        )?;
    }
    match loaded.outcome {
        Ok(plan) => {
            for line in plan_summary(&plan) {
                writeln!(out, "{line}")?;
            }
            Ok(0)
        }
        Err(LoadError::Unreadable(message)) => {
            writeln!(err, "{message}")?;
            Ok(EXIT_INVALID)
        }
        Err(LoadError::Invalid(errors)) => {
            writeln!(err, "{}", invalid_message(config, &errors))?;
            Ok(EXIT_INVALID)
        }
    }
}

/// Returns the message that lists every problem, in the PowerShell validator's
/// format: a header naming the file, then one line per problem.
#[must_use]
pub fn invalid_message(config: &Path, errors: &[String]) -> String {
    let mut message = format!("Invalid host configuration ({}):", config.display());
    for error in errors {
        message.push_str("\n  - ");
        message.push_str(error);
    }
    message
}
