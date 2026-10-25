//! The command line: argument parsing, reading the file, printing, and the exit code.
//! `start` and the clients live in their own modules; this one routes to them.

use std::ffi::OsString;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Arg, ArgAction, ArgMatches, Command, value_parser};

use crate::build::{BuildRequest, build};
use crate::checkout;
use crate::control::{Client, run_client};
use crate::diagnostics::{EXPORT_COMMAND, ExportRequest};
use crate::profile::Profile;
use crate::start::{StartRequest, start};
use crate::summary::plan_summary;
use crate::validate::{Failure, Plan, validate};

/// The largest configuration `validate` reads. Real files are a few kilobytes, so
/// this is generous; it only stops a mistaken or hostile path from being read whole.
pub const MAX_CONFIG_BYTES: u64 = 1024 * 1024;

pub(crate) const EXIT_OK: u8 = 0;
pub(crate) const EXIT_FAILED: u8 = 1;
pub(crate) const EXIT_USAGE: u8 = 2;

/// Moves the lock, channel and log roots; only a debug build honours it.
const TEST_ROOT_VARIABLE: &str = "RUNNERS_TEST_ROOT";

/// Moves the GitHub base address; only a debug build honours it.
const TEST_GITHUB_API_VARIABLE: &str = "RUNNERS_TEST_GITHUB_API";

/// Names the `docker` client `start` and `build` run; only a debug build honours it.
const TEST_DOCKER_VARIABLE: &str = "RUNNERS_TEST_DOCKER";

/// Names a test definition `build` builds instead of the embedded one; only a debug
/// build honours it.
const TEST_BUILD_DEFINITION_VARIABLE: &str = "RUNNERS_TEST_BUILD_DEFINITION";

fn config_arg(help: &'static str) -> Arg {
    Arg::new("config")
        .long("config")
        .value_name("PATH")
        .required(true)
        .value_parser(value_parser!(PathBuf))
        .help(help)
}

fn profile_arg(help: &'static str) -> Arg {
    Arg::new("profile")
        .long("profile")
        .value_name("PROFILE")
        .value_parser(["axpc", "axmc"])
        .help(help)
}

fn command() -> Command {
    let command = Command::new("runners")
        .about("Manages the self-hosted runner hosts in this repository")
        .subcommand_required(true)
        .arg_required_else_help(true)
        .subcommand(
            Command::new("validate")
                .about("Check a host configuration and print its plan")
                .arg(config_arg("The host configuration file"))
                .arg(profile_arg(
                    "The host to check it for; defaults to axpc on Windows and axmc on macOS",
                )),
        )
        .subcommand(
            Command::new("build")
                .about("Build the runner image the host configuration names, from the definition in this binary")
                .arg(config_arg("The host configuration file"))
                .arg(profile_arg(
                    "The host to build it for; defaults to axpc on Windows and axmc on macOS",
                )),
        )
        .subcommand(
            Command::new("start")
                .about("Run the host's pools in the foreground, or resume a stopped instance")
                .arg(config_arg("The host configuration file"))
                .arg(profile_arg(
                    "The host to run it as; defaults to axpc on Windows and axmc on macOS",
                )),
        )
        .subcommand(Command::new("stop").about("Stop the running instance and wait for it to end"))
        .subcommand(
            Command::new("restart")
                .about("Stop the instance's containers, read its configuration again, and resume"),
        )
        .subcommand(
            Command::new("status").about("Print what the instance and its pools are doing"),
        )
        .subcommand(Command::new("diagnostics").about(
            "Export, then remove, the diagnostic containers the instance still holds",
        ))
        .subcommand(
            Command::new(EXPORT_COMMAND)
                .hide(true)
                .arg(Arg::new("pool").long("pool").required(true))
                .arg(Arg::new("directory").long("directory").required(true))
                .arg(Arg::new("container").long("container"))
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
    command
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
    let test_root = std::env::var_os(TEST_ROOT_VARIABLE).map(PathBuf::from);
    let test_root = test_root.as_deref();
    match matches.subcommand() {
        Some(("validate", validate_matches)) => run_validate(validate_matches, out, err),
        Some(("build", build_matches)) => run_build(build_matches, test_root, err),
        Some(("start", start_matches)) => run_start(start_matches, test_root, err),
        Some(("stop", _)) => ExitCode::from(run_client(Client::Stop, test_root, out, err)),
        Some(("restart", _)) => ExitCode::from(run_client(Client::Restart, test_root, out, err)),
        Some(("status", _)) => ExitCode::from(run_client(Client::Status, test_root, out, err)),
        Some(("diagnostics", _)) => {
            ExitCode::from(run_client(Client::Diagnostics, test_root, out, err))
        }
        Some((EXPORT_COMMAND, export_matches)) => run_export(export_matches),
        #[cfg(unix)]
        Some((crate::shim::SHIM_COMMAND, shim_matches)) => run_shim(shim_matches),
        _ => ExitCode::from(EXIT_USAGE),
    }
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
        directory: directory.clone(),
        raw_records: matches.get_flag("raw-records"),
        checkout: matches.get_one::<PathBuf>("checkout").cloned(),
        deadline: std::time::Duration::from_millis(*deadline),
    };
    crate::start::hide_panic_messages();
    crate::diagnostics::exporter::run(&request, docker)
}

/// The profile named on the command line, or the one this OS implies.
///
/// # Errors
///
/// Prints the usage error and returns its exit code when the OS implies none.
fn profile_of(matches: &ArgMatches, err: &mut dyn Write) -> Result<Profile, ExitCode> {
    matches
        .get_one::<String>("profile")
        .and_then(|name| Profile::from_name(name))
        .or_else(Profile::of_this_machine)
        .ok_or_else(|| {
            finish(
                writeln!(
                    err,
                    "error: --profile axpc or --profile axmc is required on this operating system, \
                     because the profile follows the OS only on Windows (axpc) and macOS (axmc)"
                ),
                EXIT_USAGE,
            )
        })
}

fn run_start(matches: &ArgMatches, test_root: Option<&Path>, err: &mut dyn Write) -> ExitCode {
    let Some(config) = matches.get_one::<PathBuf>("config") else {
        return ExitCode::from(EXIT_USAGE);
    };
    let profile = match profile_of(matches, err) {
        Ok(profile) => profile,
        Err(code) => return code,
    };
    let github_api = std::env::var(TEST_GITHUB_API_VARIABLE).ok();
    let docker = std::env::var_os(TEST_DOCKER_VARIABLE);
    let request = StartRequest {
        config,
        profile,
        test_root,
        github_api: github_api.as_deref(),
        docker: docker.as_deref(),
    };
    ExitCode::from(start(&request, err))
}

fn run_build(matches: &ArgMatches, test_root: Option<&Path>, err: &mut dyn Write) -> ExitCode {
    let Some(config) = matches.get_one::<PathBuf>("config") else {
        return ExitCode::from(EXIT_USAGE);
    };
    let profile = match profile_of(matches, err) {
        Ok(profile) => profile,
        Err(code) => return code,
    };
    let docker = std::env::var_os(TEST_DOCKER_VARIABLE);
    let definition = std::env::var_os(TEST_BUILD_DEFINITION_VARIABLE).map(PathBuf::from);
    let request = BuildRequest {
        config,
        profile,
        test_root,
        docker: docker.as_deref(),
        definition: definition.as_deref(),
    };
    ExitCode::from(build(&request, err))
}

pub(crate) fn print_error(err: &mut dyn Write, error: &dyn std::fmt::Display) -> u8 {
    let _ = writeln!(err, "error: {error}");
    EXIT_FAILED
}

fn finish(written: io::Result<()>, code: u8) -> ExitCode {
    if written.is_err() {
        return ExitCode::from(EXIT_FAILED);
    }
    ExitCode::from(code)
}

fn run_validate(matches: &ArgMatches, out: &mut dyn Write, err: &mut dyn Write) -> ExitCode {
    let Some(config) = matches.get_one::<PathBuf>("config") else {
        return ExitCode::from(EXIT_USAGE);
    };
    let profile = match profile_of(matches, err) {
        Ok(profile) => profile,
        Err(code) => return code,
    };
    match validate_file(config, profile, out, err) {
        Ok(code) => ExitCode::from(code),
        Err(_) => ExitCode::from(EXIT_FAILED),
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
    /// The message `validate` and `start` print for the file at `config`.
    pub(crate) fn message(&self, config: &Path) -> String {
        match self {
            Self::Unreadable(message) => message.clone(),
            Self::Invalid(errors) => invalid_message(config, errors),
        }
    }

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
        writeln!(err, "{}", checkout_rule_note(config))?;
    }
    match loaded.outcome {
        Ok(plan) => {
            for line in plan_summary(&plan) {
                writeln!(out, "{line}")?;
            }
            Ok(EXIT_OK)
        }
        Err(error) => {
            writeln!(err, "{}", error.message(config))?;
            Ok(EXIT_FAILED)
        }
    }
}

/// The note that no checkout was found above `config`, so the checkout rule was
/// skipped.
pub(crate) fn checkout_rule_note(config: &Path) -> String {
    format!(
        "note: no checkout of this project was found above {}, so diagnosticDirectory was not compared with it",
        config.display()
    )
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
