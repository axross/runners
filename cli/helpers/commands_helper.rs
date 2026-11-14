//! a stand-in for the command line that the library no longer ships, so the tests can
//! run the foreground `start`, `build`, `validate` and the version 1 clients as real
//! processes: they hold the lock, take signals and run in a Job Object, which only a
//! process shows. Any other first argument is a hidden mode, run as the app's
//! executable runs it, so a test can hand this program to the exporter and the shims
//! as the program to relaunch. A release must not ship it.
//!
//! ```text
//! commands-helper validate|build|start --config PATH [--profile axpc|axmc]
//! commands-helper stop|restart|status|diagnostics
//! commands-helper export|shim|vm-shim ...
//! ```
//!
//! The test variables are read as the agent's options read them: the library honours
//! them in a debug build only.

use std::ffi::OsString;
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use marshal_cli::build::{BuildRequest, build};
use marshal_cli::command::{
    EXIT_FAILED, EXIT_OK, EXIT_USAGE, TEST_BUILD_DEFINITION_VARIABLE, TEST_DOCKER_VARIABLE,
    TEST_GITHUB_API_VARIABLE, TEST_ROOT_VARIABLE, checkout_rule_note, load_plan,
};
use marshal_cli::control::{Client, run_client};
use marshal_cli::profile::Profile;
use marshal_cli::start::{StartRequest, start};
use marshal_cli::summary::plan_summary;

/// the options of `validate`, `build` and `start`.
struct Options {
    config: PathBuf,
    profile: Profile,
}

fn options(args: &[OsString], err: &mut dyn Write) -> Result<Options, u8> {
    let mut config = None;
    let mut profile = None;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match (arg.to_str(), rest.next()) {
            (Some("--config"), Some(value)) => config = Some(PathBuf::from(value)),
            (Some("--profile"), Some(value)) => {
                profile = Some(
                    value
                        .to_str()
                        .and_then(Profile::from_name)
                        .ok_or(EXIT_USAGE)?,
                );
            }
            _ => return Err(EXIT_USAGE),
        }
    }
    let config = config.ok_or(EXIT_USAGE)?;
    let Some(profile) = profile.or_else(Profile::of_this_machine) else {
        let _ = writeln!(
            err,
            "error: --profile axpc or --profile axmc is required on this operating system"
        );
        return Err(EXIT_USAGE);
    };
    Ok(Options { config, profile })
}

fn validate(options: &Options, out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    let loaded = load_plan(&options.config, options.profile);
    if loaded.checkout_rule_skipped {
        let _ = writeln!(err, "{}", checkout_rule_note(&options.config));
    }
    match loaded.outcome {
        Ok(plan) => {
            for line in plan_summary(&plan) {
                let _ = writeln!(out, "{line}");
            }
            EXIT_OK
        }
        Err(error) => {
            let _ = writeln!(err, "{}", error.message(&options.config));
            EXIT_FAILED
        }
    }
}

fn client(word: &str) -> Option<Client> {
    match word {
        "stop" => Some(Client::Stop),
        "restart" => Some(Client::Restart),
        "status" => Some(Client::Status),
        "diagnostics" => Some(Client::Diagnostics),
        _ => None,
    }
}

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().collect();
    let word = args
        .get(1)
        .and_then(|word| word.to_str())
        .unwrap_or_default();
    let rest = args.get(2..).unwrap_or_default();
    let out = &mut std::io::stdout();
    let err = &mut std::io::stderr();
    let test_root = std::env::var_os(TEST_ROOT_VARIABLE).map(PathBuf::from);
    let test_root = test_root.as_deref();
    if let Some(client) = client(word) {
        if !rest.is_empty() {
            return ExitCode::from(EXIT_USAGE);
        }
        return ExitCode::from(run_client(client, test_root, out, err));
    }
    if !matches!(word, "validate" | "build" | "start") {
        return marshal_cli::command::run(args, out, err);
    }
    let options = match options(rest, err) {
        Ok(options) => options,
        Err(code) => return ExitCode::from(code),
    };
    let docker = std::env::var_os(TEST_DOCKER_VARIABLE);
    let code = match word {
        "validate" => validate(&options, out, err),
        "build" => {
            let definition = std::env::var_os(TEST_BUILD_DEFINITION_VARIABLE).map(PathBuf::from);
            build(
                &BuildRequest {
                    config: &options.config,
                    profile: options.profile,
                    test_root,
                    docker: docker.as_deref(),
                    definition: definition.as_deref(),
                },
                err,
            )
        }
        _ => {
            let github_api = std::env::var(TEST_GITHUB_API_VARIABLE).ok();
            start(
                &StartRequest {
                    config: &options.config,
                    profile: options.profile,
                    test_root,
                    github_api: github_api.as_deref(),
                    docker: docker.as_deref(),
                },
                err,
            )
        }
    };
    ExitCode::from(code)
}
