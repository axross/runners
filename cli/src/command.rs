//! the hidden modes the app's executable relaunches itself in (the diagnostics
//! exporter, and on Unix the container and VM shims), and reading a host
//! configuration file for the agent, `start` and `build`. There are no user-facing
//! commands; see `docs/operations/cli.md`.

use std::ffi::OsString;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Arg, ArgAction, ArgMatches, Command, value_parser};

use crate::checkout;
use crate::diagnostics::{EXPORT_COMMAND, ExportRequest};
use crate::profile::Profile;
use crate::validate::{CheckoutRule, Failure, Plan, validate_with};

/// the largest configuration `validate` reads; it only stops a mistaken or hostile
/// path from being read whole.
pub const MAX_CONFIG_BYTES: u64 = 1024 * 1024;

pub const EXIT_OK: u8 = 0;
pub const EXIT_FAILED: u8 = 1;
pub const EXIT_USAGE: u8 = 2;

// the four test-override variables below: only a debug build honours them, see docs/operations/cli.md
pub const TEST_ROOT_VARIABLE: &str = "MARSHAL_TEST_ROOT";

pub const TEST_GITHUB_API_VARIABLE: &str = "MARSHAL_TEST_GITHUB_API";

pub const TEST_DOCKER_VARIABLE: &str = "MARSHAL_TEST_DOCKER";

pub const TEST_BUILD_DEFINITION_VARIABLE: &str = "MARSHAL_TEST_BUILD_DEFINITION";

fn command() -> Command {
    let command = Command::new("marshal")
        .subcommand_required(true)
        .subcommand(
            Command::new(EXPORT_COMMAND)
                .hide(true)
                .arg(Arg::new("pool").long("pool").required(true))
                .arg(Arg::new("directory").long("directory").required(true))
                .arg(Arg::new("container").long("container"))
                .arg(Arg::new("vm").long("vm"))
                .arg(
                    Arg::new("raw-records")
                        .long("raw-records")
                        .action(ArgAction::SetTrue),
                )
                .arg(
                    Arg::new("checkout")
                        .long("checkout")
                        .value_parser(value_parser!(PathBuf)),
                )
                .arg(
                    Arg::new("deadline-ms")
                        .long("deadline-ms")
                        .required(true)
                        .value_parser(value_parser!(u64)),
                )
                .arg(
                    Arg::new("client")
                        .required(true)
                        .num_args(1)
                        .last(true)
                        .value_parser(value_parser!(OsString)),
                ),
        );
    #[cfg(unix)]
    let command = command.subcommand(
        Command::new(crate::shim::SHIM_COMMAND)
            .hide(true)
            .arg(Arg::new("name").long("name").required(true))
            .arg(
                Arg::new("diagnostic")
                    .long(crate::shim::DIAGNOSTIC_FLAG.trim_start_matches('-'))
                    .action(ArgAction::SetTrue),
            )
            .arg(
                Arg::new("client")
                    .required(true)
                    .num_args(1..)
                    .last(true)
                    .value_parser(value_parser!(OsString)),
            ),
    );
    #[cfg(unix)]
    let command = command.subcommand(
        Command::new(crate::vm_shim::VM_SHIM_COMMAND)
            .hide(true)
            .arg(Arg::new("name").long("name").required(true))
            .arg(Arg::new("diagnostic-pool").long("diagnostic-pool"))
            .arg(
                Arg::new("client")
                    .required(true)
                    .num_args(1)
                    .last(true)
                    .value_parser(value_parser!(OsString)),
            ),
    );
    command
}

/// runs the hidden mode the arguments name, the first being the program name.
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
        Some((EXPORT_COMMAND, export_matches)) => run_export(export_matches),
        #[cfg(unix)]
        Some((crate::shim::SHIM_COMMAND, shim_matches)) => run_shim(shim_matches),
        #[cfg(unix)]
        Some((crate::vm_shim::VM_SHIM_COMMAND, shim_matches)) => run_vm_shim(shim_matches),
        _ => ExitCode::from(EXIT_USAGE),
    }
}

#[cfg(unix)]
fn run_vm_shim(matches: &ArgMatches) -> ExitCode {
    let (Some(name), Some(program)) = (
        matches.get_one::<String>("name"),
        matches.get_one::<OsString>("client"),
    ) else {
        return ExitCode::from(EXIT_USAGE);
    };
    let pool = matches.get_one::<String>("diagnostic-pool");
    crate::vm_shim::run(name, program, pool.map(String::as_str))
}

#[cfg(unix)]
fn run_shim(matches: &ArgMatches) -> ExitCode {
    let (Some(name), Some(mut client)) = (
        matches.get_one::<String>("name"),
        matches.get_many::<OsString>("client"),
    ) else {
        return ExitCode::from(EXIT_USAGE);
    };
    let Some(program) = client.next() else {
        return ExitCode::from(EXIT_USAGE);
    };
    let arguments: Vec<OsString> = client.cloned().collect();
    crate::shim::run(name, matches.get_flag("diagnostic"), program, &arguments)
}

fn run_export(matches: &ArgMatches) -> ExitCode {
    let (Some(pool), Some(directory), Some(deadline), Some(docker)) = (
        matches.get_one::<String>("pool"),
        matches.get_one::<String>("directory"),
        matches.get_one::<u64>("deadline-ms"),
        matches.get_one::<OsString>("client"),
    ) else {
        return ExitCode::from(EXIT_USAGE);
    };
    let request = ExportRequest {
        pool: pool.clone(),
        container: matches.get_one::<String>("container").cloned(),
        vm: matches.get_one::<String>("vm").cloned(),
        directory: directory.clone(),
        raw_records: matches.get_flag("raw-records"),
        checkout: matches.get_one::<PathBuf>("checkout").cloned(),
        deadline: std::time::Duration::from_millis(*deadline),
    };
    crate::start::hide_panic_messages();
    crate::diagnostics::exporter::run(&request, docker)
}

pub fn print_error(err: &mut dyn Write, error: &dyn std::fmt::Display) -> u8 {
    let _ = writeln!(err, "error: {error}");
    EXIT_FAILED
}

fn finish(written: io::Result<()>, code: u8) -> ExitCode {
    if written.is_err() {
        return ExitCode::from(EXIT_FAILED);
    }
    ExitCode::from(code)
}

/// reads the file, refusing one over [`MAX_CONFIG_BYTES`] without reading past the
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

/// why a host configuration file gave no plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadError {
    /// the file is missing, cannot be read, or is not JSON; the message says which.
    Unreadable(String),
    /// the document breaks rules, listed in the order they were found.
    Invalid(Vec<String>),
}

impl LoadError {
    /// the message `validate` and `start` print for the file at `config`.
    pub fn message(&self, config: &Path) -> String {
        match self {
            Self::Unreadable(message) => message.clone(),
            Self::Invalid(errors) => invalid_message(config, errors),
        }
    }

    /// every problem, one per entry.
    pub(crate) fn into_problems(self) -> Vec<String> {
        match self {
            Self::Unreadable(message) => vec![message],
            Self::Invalid(errors) => errors,
        }
    }
}

/// a host configuration file, read and validated against the checkout above it.
#[derive(Debug, Clone, PartialEq)]
pub struct Loaded {
    pub outcome: Result<Plan, LoadError>,
    /// true when no checkout was found above the file, so `diagnosticDirectory` was
    /// not compared with one.
    pub checkout_rule_skipped: bool,
}

/// reads the file and validates it for `profile`.
pub fn load_plan(config: &Path, profile: Profile) -> Loaded {
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
    let validation = validate_with(&bytes, profile, CheckoutRule::Above(checkout.as_deref()));
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

/// the note that no checkout was found above `config`, so the checkout rule was
/// skipped.
pub fn checkout_rule_note(config: &Path) -> String {
    format!(
        "note: no checkout of this project was found above {}, so diagnosticDirectory was not compared with it",
        config.display()
    )
}

/// returns the problems as the validator prints them: a header naming the file, then
/// one line each.
#[must_use]
pub fn invalid_message(config: &Path, errors: &[String]) -> String {
    let mut message = format!("Invalid host configuration ({}):", config.display());
    for error in errors {
        message.push_str("\n  - ");
        message.push_str(error);
    }
    message
}
