//! `start`: runs the host's pools in the foreground as the machine's one instance,
//! or asks a stopped instance to resume.
//!
//! A fresh `start` takes the instance lock, validates the configuration and prints
//! its plan, opens its log file, contains its children, builds the HTTPS client,
//! opens the control channel, and then runs the instance until a stop. A `start`
//! that finds the lock held changes nothing locally: it asks this user's instance
//! to resume, which only a stopped instance does.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::task::{Context, Poll};
use std::time::Duration;

use tokio::sync::{mpsc, oneshot};

use crate::command::{EXIT_FAILED, EXIT_OK, checkout_rule_note, load_plan, print_error};
use crate::instance::channel::{ChannelServer, ClientError, exchange};
use crate::instance::lifecycle::{Ended, Instance, Settings};
use crate::instance::lock::{InstanceLock, LockError};
use crate::instance::log_file::{StartLog, open_log_file};
use crate::instance::protocol::{Incoming, Outcome, Request};
use crate::instance::roots::Roots;
use crate::instance::runtime_core::RuntimeCore;
use crate::instance::source::FileSource;
use crate::profile::Profile;
use crate::runtime::clock::SystemClock;
use crate::runtime::docker::{DockerBackend, DockerConfig, JobLauncher};
use crate::runtime::log::Log;
use crate::runtime::registration::{GitHubRegistrar, HttpLimits};
use crate::summary::plan_summary;

/// How long a resuming `start` waits for the instance's answer.
const RESUME_ANSWER_WITHIN: Duration = Duration::from_secs(10);

/// What a console close, a logoff or a shutdown leaves the instance's forced stop,
/// inside the 5 s the operating system allows a close handler.
const CLOSE_REMOVE_BUDGET: Duration = Duration::from_secs(4);

/// What `start` was given.
pub struct StartRequest<'a> {
    pub config: &'a Path,
    pub profile: Profile,
    /// Moves the lock, channel and log roots; a debug build only.
    pub test_root: Option<&'a Path>,
    /// Moves the GitHub base address; a debug build only.
    pub github_api: Option<&'a str>,
}

/// Runs `start` and returns its exit code. Messages before the log file opens go to
/// `err`; after that, every line goes through the log.
pub fn start(request: &StartRequest<'_>, err: &mut dyn Write) -> u8 {
    let roots = match Roots::resolve(request.test_root) {
        Ok(roots) => roots,
        Err(error) => return print_error(err, &error),
    };
    let lock = match InstanceLock::acquire(&roots) {
        Ok(lock) => lock,
        Err(LockError::Held) => return resume(&roots, request, err),
        Err(error) => return print_error(err, &error),
    };
    let loaded = load_plan(request.config, request.profile);
    let plan = match loaded.outcome {
        Ok(plan) => plan,
        Err(error) => {
            if loaded.checkout_rule_skipped {
                let _ = writeln!(err, "{}", checkout_rule_note(request.config));
            }
            let _ = writeln!(err, "{}", error.message(request.config));
            return EXIT_FAILED;
        }
    };
    if request.profile == Profile::Axmc {
        return print_error(
            err,
            &"the configuration is valid, but axmc runs its pools in a VM, and the VM backend comes in Phase 6",
        );
    }
    let (canonical, config_path) = match fs::canonicalize(request.config).and_then(|canonical| {
        std::path::absolute(request.config).map(|absolute| (canonical, absolute))
    }) {
        Ok(paths) => paths,
        Err(error) => {
            return print_error(
                err,
                &format!("the configuration path cannot be resolved: {error}"),
            );
        }
    };
    let opened = match open_log_file(roots.log_directory()) {
        Ok(opened) => opened,
        Err(error) => return print_error(err, &error),
    };
    let log = Rc::new(StartLog::new(opened.file));
    log.info(&format!("Logging to {}.", opened.path.display()));
    if let Some(failure) = opened.pruning_failed {
        log.warn(&format!("An old log file could not be removed: {failure}"));
    }
    if loaded.checkout_rule_skipped {
        log.info(&checkout_rule_note(request.config));
    }
    for line in plan_summary(&plan) {
        log.info(&line);
    }
    hide_panic_messages();
    let launcher = match contain() {
        Ok(launcher) => launcher,
        Err(error) => {
            log.warn(&format!(
                "The children of this process cannot be contained: {error}"
            ));
            return EXIT_FAILED;
        }
    };
    let registrar = match GitHubRegistrar::new(request.github_api, HttpLimits::default()) {
        Ok(registrar) => registrar,
        Err(error) => {
            log.warn(&format!("The start failed: {error}"));
            return EXIT_FAILED;
        }
    };
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            log.warn(&format!("The async runtime cannot start: {error}"));
            return EXIT_FAILED;
        }
    };
    let parts = Parts {
        lock: &lock,
        roots: &roots,
        settings: Settings {
            config_path: canonical.to_string_lossy().into_owned(),
            profile: request.profile,
        },
        config_path,
        launcher,
        registrar: &registrar,
        log,
    };
    runtime.block_on(run_instance(parts, plan))
}

/// Asks this user's instance, which holds the lock, to resume with the same path
/// and profile.
fn resume(roots: &Roots, request: &StartRequest<'_>, err: &mut dyn Write) -> u8 {
    let path = match fs::canonicalize(request.config) {
        Ok(path) => path,
        Err(error) => {
            return print_error(
                err,
                &format!("the configuration path cannot be resolved: {error}"),
            );
        }
    };
    let resume = match Request::resume(request.profile, &path) {
        Ok(resume) => resume,
        Err(error) => return print_error(err, &error),
    };
    match exchange(roots, &resume, RESUME_ANSWER_WITHIN) {
        Ok(response) if response.outcome == Outcome::Ok => {
            let _ = writeln!(err, "The stopped instance resumed with {}.", path.display());
            EXIT_OK
        }
        Ok(response) => print_error(
            err,
            &format!("the instance did not resume: {}", response.message),
        ),
        Err(ClientError::NoInstance) => print_error(
            err,
            &"the instance lock is held, by another OS user or by an instance of this user \
              that is still starting or ending; the lock does not say which",
        ),
        Err(error) => print_error(err, &error),
    }
}

/// What the instance is run with.
struct Parts<'a> {
    lock: &'a InstanceLock,
    roots: &'a Roots,
    /// Holds the canonical path, which a resume is compared with.
    settings: Settings,
    /// The path as given, made absolute, which every read uses: the checkout rule
    /// walks it lexically, as at the first validation.
    config_path: PathBuf,
    launcher: JobLauncher,
    registrar: &'a GitHubRegistrar,
    log: Rc<StartLog>,
}

async fn run_instance(parts: Parts<'_>, plan: crate::validate::Plan) -> u8 {
    let log: &dyn Log = &*parts.log;
    let (sender, mut requests) = mpsc::unbounded_channel();
    let server = match ChannelServer::open(parts.lock, parts.roots, sender.clone()) {
        Ok(server) => server,
        Err(error) => {
            log.warn(&format!("The start failed: {error}"));
            return EXIT_FAILED;
        }
    };
    log.info(&format!(
        "The instance is running; stop it with Ctrl+C or `runners stop`. Its channel is {}.",
        server.address()
    ));
    let config = DockerConfig {
        launcher: parts.launcher,
        ..DockerConfig::default()
    };
    let backend = DockerBackend::new(config, SystemClock, parts.log.clone());
    let core = RuntimeCore::new(&backend, &SystemClock, parts.registrar, log);
    let source = FileSource::new(parts.config_path, parts.settings.profile, log);
    let instance = Instance::new(parts.settings, &backend, &SystemClock, &core, &source, log);
    let force = || instance.force_removal();
    let mut run = std::pin::pin!(instance.run(plan, &mut requests));
    let ended = tokio::select! {
        ended = &mut run => Some(ended),
        () = signals(&sender, &force, log) => None,
        () = console_closed(log) => {
            request_stop(&sender);
            force();
            let stopped = tokio::time::timeout(CLOSE_REMOVE_BUDGET, &mut run).await;
            Some(stopped.unwrap_or(Ended { containers_removed: false, nothing_to_run: false }))
        }
    };
    drop(server);
    let Some(ended) = ended else {
        return EXIT_FAILED;
    };
    if ended.nothing_to_run {
        log.warn("The instance ended because no pool could start.");
        EXIT_FAILED
    } else if ended.containers_removed {
        log.info("The instance has stopped.");
        EXIT_OK
    } else {
        log.warn(
            "The instance has stopped, but a container could not be removed; \
             the next start removes it.",
        );
        EXIT_FAILED
    }
}

/// Asks the instance to stop, as `stop` does, through its own request channel.
fn request_stop(sender: &mpsc::UnboundedSender<Incoming>) {
    let (reply, _) = oneshot::channel();
    let _ = sender.send(Incoming {
        request: Request::Stop,
        reply,
    });
}

/// Polls a signal stream for its next signal.
type PollSignal = Box<dyn FnMut(&mut Context<'_>) -> Poll<Option<()>>>;

/// One signal `start` listens for.
struct Listener {
    name: &'static str,
    /// A second one forces removal.
    is_interrupt: bool,
    poll: PollSignal,
}

/// The listener for a stream `opened` gives, or a warning naming the signal when it
/// could not be opened.
fn listener<S: 'static>(
    name: &'static str,
    is_interrupt: bool,
    opened: io::Result<S>,
    poll: fn(&mut S, &mut Context<'_>) -> Poll<Option<()>>,
    log: &dyn Log,
) -> Option<Listener> {
    match opened {
        Ok(mut stream) => Some(Listener {
            name,
            is_interrupt,
            poll: Box::new(move |context| poll(&mut stream, context)),
        }),
        Err(error) => {
            log.warn(&format!("{name} cannot be handled: {error}"));
            None
        }
    }
}

/// Listens for SIGINT, SIGTERM and SIGHUP.
#[cfg(unix)]
fn listeners(log: &dyn Log) -> Vec<Listener> {
    use tokio::signal::unix::{Signal, SignalKind, signal};
    [
        ("SIGINT", true, SignalKind::interrupt()),
        ("SIGTERM", false, SignalKind::terminate()),
        ("SIGHUP", false, SignalKind::hangup()),
    ]
    .into_iter()
    .filter_map(|(name, is_interrupt, kind)| {
        listener(name, is_interrupt, signal(kind), Signal::poll_recv, log)
    })
    .collect()
}

/// Listens for Ctrl+C.
#[cfg(windows)]
fn listeners(log: &dyn Log) -> Vec<Listener> {
    use tokio::signal::windows::{CtrlC, ctrl_c};
    listener("Ctrl+C", true, ctrl_c(), CtrlC::poll_recv, log)
        .into_iter()
        .collect()
}

/// Turns each signal [`listeners`] could open into a stop, and a second interrupt
/// into forced removal. A signal that could not be opened keeps its default action.
/// Never ends.
async fn signals(sender: &mpsc::UnboundedSender<Incoming>, force: &dyn Fn(), log: &dyn Log) {
    let mut listeners = listeners(log);
    if listeners.is_empty() {
        log.warn("No signal can be handled, so only `runners stop` stops the instance.");
    }
    let mut stopping = false;
    loop {
        let (name, is_interrupt) = std::future::poll_fn(|context| {
            listeners
                .iter_mut()
                .find_map(|listener| match (listener.poll)(context) {
                    Poll::Ready(Some(())) => Some((listener.name, listener.is_interrupt)),
                    _ => None,
                })
                .map_or(Poll::Pending, Poll::Ready)
        })
        .await;
        on_signal(name, is_interrupt, &mut stopping, sender, force, log);
    }
}

fn on_signal(
    name: &str,
    is_interrupt: bool,
    stopping: &mut bool,
    sender: &mpsc::UnboundedSender<Incoming>,
    force: &dyn Fn(),
    log: &dyn Log,
) {
    if !*stopping {
        *stopping = true;
        log.info(&format!("{name} received: stopping the instance."));
        request_stop(sender);
    } else if is_interrupt {
        log.warn(&format!(
            "{name} received again: the containers are removed without their grace period."
        ));
        force();
    }
}

/// On Windows, waits for the console to close or the user to log off or the
/// machine to shut down. The async runtime's handler does not return to the
/// operating system for these events, so the process lives until `start` has
/// stopped the instance within [`CLOSE_REMOVE_BUDGET`] and exits; see the CLI
/// document.
#[cfg(windows)]
async fn console_closed(log: &dyn Log) {
    use tokio::signal::windows::{ctrl_close, ctrl_logoff, ctrl_shutdown};
    let (close, logoff, shutdown) = (ctrl_close(), ctrl_logoff(), ctrl_shutdown());
    let (Ok(mut close), Ok(mut logoff), Ok(mut shutdown)) = (close, logoff, shutdown) else {
        log.warn(
            "The console close cannot be handled; the Job Object and the next start clean up.",
        );
        return std::future::pending().await;
    };
    tokio::select! {
        _ = close.recv() => {}
        _ = logoff.recv() => {}
        _ = shutdown.recv() => {}
    }
    log.warn("The console is closing: removing every container now.");
}

/// Unix has no console close; a closed terminal is SIGHUP.
#[cfg(unix)]
async fn console_closed(_log: &dyn Log) {
    std::future::pending().await
}

/// Puts this process in a Job Object that kills every process in it when the last
/// handle closes. The handle is never closed by the CLI, so the operating system
/// closes it when the process ends, however it ends, after the exit code is set.
#[cfg(windows)]
fn contain() -> Result<JobLauncher, String> {
    use win32job::{ExtendedLimitInfo, Job};
    fn described(error: win32job::JobError) -> String {
        let call = error.to_string();
        format!("{call}: {}", io::Error::from(error))
    }
    let job = Job::create_with_limit_info(ExtendedLimitInfo::new().limit_kill_on_job_close())
        .map_err(described)?;
    job.assign_current_process().map_err(described)?;
    let _ = job.into_handle();
    Ok(JobLauncher::Direct)
}

/// Runs each job container's client through the shim, this program run again, so
/// the container is removed when this process ends, however it ends.
#[cfg(unix)]
fn contain() -> Result<JobLauncher, String> {
    std::env::current_exe()
        .map(JobLauncher::Shim)
        .map_err(|error| format!("this program cannot be found to run the shim: {error}"))
}

/// A panic's message could hold anything, so the default hook, which writes it to
/// the terminal unmasked, is replaced by one that names only where it happened. The
/// runtime core's panics reach the log, masked, as failures.
fn hide_panic_messages() {
    std::panic::set_hook(Box::new(|info| {
        let place = info.location().map_or_else(String::new, |location| {
            format!(" at {}:{}", location.file(), location.line())
        });
        let _ = writeln!(io::stderr(), "WARNING: a panic occurred{place}.");
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_resume_waits_ten_seconds_for_its_answer() {
        assert_eq!(RESUME_ANSWER_WITHIN, Duration::from_secs(10));
    }

    #[test]
    fn a_console_close_leaves_four_seconds_for_the_removal() {
        assert_eq!(CLOSE_REMOVE_BUDGET, Duration::from_secs(4));
    }
}
