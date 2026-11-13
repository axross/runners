//! running the agent: the resident loop on its seams, and the entry that wires the
//! Docker backend, GitHub, the token store, the lock, the channel and the log, with
//! the Job Object that ends the agent's children with it.

use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use tokio::sync::{mpsc, watch};

use crate::command::{EXIT_FAILED, EXIT_OK, print_error};
use crate::github_status::collector::Collector;
use crate::github_status::http::GitHubReader;
use crate::instance::channel::ChannelError;
use crate::instance::lifecycle::{Monitor, NoMonitor, StopTimes};
use crate::instance::lock::{InstanceLock, LockError};
use crate::instance::roots::Roots;
use crate::lifecycle_settings::Current;
use crate::profile::Profile;
use crate::runtime::backend::Backend;
use crate::runtime::clock::{Clock, SystemClock};
use crate::runtime::docker::{DockerBackend, DockerConfig, resolve_program};
use crate::runtime::log::Log;
use crate::runtime::registration::{GitHubRegistrar, HttpLimits, Registrar};
use crate::start::{ANSWERS_WITHIN, contain, hide_panic_messages, single_threaded_runtime};
use crate::token_store::TokenStore;

use super::builder::{DockerImageBuilder, ImageBuilder};
use super::channel::AgentChannel;
use super::executable::{CHECK_EVERY, Executable};
use super::log::AgentLog;
use super::machine::{Agent, DRAIN_BOUND, Ended, Launch, Seams, Settings};
use super::server::Limits;
use super::snapshot::Snapshot;
use super::tokens::StoreTokens;

/// a resident agent on its seams: the tests give it fakes, and [`run`] gives it
/// Docker and GitHub.
pub struct Resident<'a, B, K, R, D, M = NoMonitor> {
    pub backend: &'a B,
    pub clock: &'a K,
    pub registrar: &'a R,
    /// reads GitHub status for the window, only while a client wants it.
    pub reader: &'a GitHubReader,
    pub builder: &'a D,
    /// the signals that stop every runner, and the keep-awake hold.
    pub monitor: &'a M,
    /// the settings in effect, which the agent sets and the monitor reads.
    pub in_effect: &'a Current,
    /// `None` on a host with no store; every pool is then skipped.
    pub store: Option<Arc<dyn TokenStore>>,
    pub log: &'a dyn Log,
    pub roots: &'a Roots,
    /// the lock the caller holds, which lets the channel open.
    pub lock: &'a InstanceLock,
    pub settings: Settings,
    /// the file the agent started from, which it watches.
    pub executable: Option<PathBuf>,
    pub limits: Limits,
}

/// runs the agent until it is asked to end, then tells every connection why and
/// closes the channel; the caller releases the lock afterwards.
pub async fn run_resident<B, K, R, D, M>(
    resident: Resident<'_, B, K, R, D, M>,
) -> Result<Ended, ChannelError>
where
    B: Backend,
    K: Clock,
    R: Registrar,
    D: ImageBuilder,
    M: Monitor,
{
    let store = resident.store.clone();
    let tokens = StoreTokens::new(resident.store);
    let (calls, mut requests) = mpsc::unbounded_channel();
    let (snapshots, receiver) = watch::channel(Arc::new(Snapshot::default()));
    let followed = receiver.clone();
    let mut channel = AgentChannel::open(
        resident.lock,
        resident.roots,
        calls,
        receiver,
        resident.limits,
    )?;
    let clock = resident.clock;
    let collector = channel.github_feed().map(|feed| {
        Collector::new(
            resident.reader,
            store,
            resident.log,
            Box::new(move || super::view::millis(clock.now())),
            feed.published,
            feed.active,
        )
    });
    resident.log.info(&format!(
        "The agent is running. Its channel is {}.",
        channel.address()
    ));
    let (changed, executable) = watch::channel(false);
    let watched = resident.executable.as_deref().and_then(|path| {
        let watched = Executable::at(path);
        if watched.is_none() {
            resident.log.warn(&format!(
                "The agent cannot watch {} for a change, so it will not end when it is replaced.",
                path.display()
            ));
        }
        watched
    });
    let watcher = tokio::spawn(async move {
        Executable::wait_for_change(watched, CHECK_EVERY).await;
        changed.send_replace(true);
    });
    let agent = Agent::new(
        resident.settings,
        Seams {
            backend: resident.backend,
            clock: resident.clock,
            registrar: resident.registrar,
            builder: resident.builder,
            tokens: &tokens,
            log: resident.log,
            monitor: resident.monitor,
            in_effect: resident.in_effect,
            snapshots,
            executable,
        },
    );
    let collecting = async {
        if let Some(collector) = collector {
            collector.run(followed).await;
        }
        std::future::pending::<()>().await;
    };
    let ended = tokio::select! {
        ended = agent.run(&mut requests) => ended,
        // collection never ends on its own
        () = collecting => std::future::pending::<Ended>().await,
    };
    drop(agent);
    watcher.abort();
    channel
        .close(ended.reason, ended.update.clone(), ANSWERS_WITHIN)
        .await;
    Ok(ended)
}

/// what the agent was given; the fields after `keep_awake` are for the tests, and a
/// release build ignores them.
pub struct AgentOptions<'a> {
    pub launch: Launch,
    /// the keep-awake request on Windows, which the executable supplies; without one
    /// nothing is held.
    pub keep_awake: Option<crate::windows_host::keep_awake::Request>,
    /// moves the lock, channel, log and configuration roots; a debug build only.
    pub test_root: Option<&'a Path>,
    /// moves the GitHub base address; a debug build only.
    pub github_api: Option<&'a str>,
    /// names the `docker` client; a debug build only.
    pub docker: Option<&'a OsStr>,
    /// variables added to every `docker` client's environment; a debug build only.
    pub docker_environment: Vec<(OsString, OsString)>,
    /// names a test definition to build instead of the embedded one; a debug build
    /// only.
    pub definition: Option<&'a Path>,
    /// names the file to watch instead of this program; a debug build only.
    pub executable: Option<&'a Path>,
    /// replaces the platform's token store; a debug build only.
    pub store: Option<Arc<dyn TokenStore>>,
    /// replaces the channel's limits; a debug build only.
    pub limits: Option<Limits>,
    /// replaces the log file size limit; a debug build only.
    pub max_log_bytes: Option<u64>,
    /// replaces the host's profile, which the configuration is checked under; a debug
    /// build only.
    pub profile: Option<Profile>,
}

impl AgentOptions<'_> {
    #[must_use]
    pub fn new(launch: Launch) -> Self {
        Self {
            launch,
            keep_awake: None,
            test_root: None,
            github_api: None,
            docker: None,
            docker_environment: Vec::new(),
            definition: None,
            executable: None,
            store: None,
            limits: None,
            max_log_bytes: None,
            profile: None,
        }
    }
}

/// the settings a debug build lets the caller replace, with a release build's always
/// the host's.
#[doc(hidden)]
pub struct Applied {
    pub max_log_bytes: u64,
    pub environment: Vec<(OsString, OsString)>,
    pub store: Option<Arc<dyn TokenStore>>,
    pub limits: Limits,
    pub executable: Option<PathBuf>,
    pub profile: Profile,
}

/// the settings `run` uses: the options' replacements in a debug build, and in a
/// release build only the host's own.
#[doc(hidden)]
#[must_use]
pub fn applied_overrides(options: &AgentOptions<'_>) -> Applied {
    #[allow(unused_mut)]
    let mut applied = Applied {
        max_log_bytes: AgentLog::default_max_bytes(),
        environment: Vec::new(),
        store: None,
        limits: Limits::default(),
        executable: std::env::current_exe().ok(),
        // a host that is neither runs the agent only in the tests
        profile: Profile::of_this_machine().unwrap_or(Profile::Axpc),
    };
    #[cfg(debug_assertions)]
    {
        if let Some(profile) = options.profile {
            applied.profile = profile;
        }
        if let Some(max) = options.max_log_bytes {
            applied.max_log_bytes = max;
        }
        applied.environment.clone_from(&options.docker_environment);
        applied.store.clone_from(&options.store);
        if let Some(replacement) = options.limits {
            applied.limits = replacement;
        }
        if let Some(path) = options.executable {
            applied.executable = Some(path.to_path_buf());
        }
    }
    #[cfg(not(debug_assertions))]
    let _ = options;
    applied
}

/// the platform's token store: the Credential Manager on Windows, and none elsewhere
/// until a store is added behind the interface.
fn platform_store(log: &dyn Log) -> Option<Arc<dyn TokenStore>> {
    #[cfg(windows)]
    {
        match crate::token_store::windows::credential_manager() {
            Ok(store) => Some(Arc::new(store)),
            Err(error) => {
                log.warn(&format!("The Credential Manager cannot be opened: {error}"));
                None
            }
        }
    }
    #[cfg(not(windows))]
    {
        let _ = log;
        None
    }
}

/// runs the agent as the machine's one instance and returns its exit code; messages
/// before the log file opens go to `err`, and every line after goes through the log.
pub fn run(options: AgentOptions<'_>, err: &mut dyn Write) -> u8 {
    let roots = match Roots::resolve(options.test_root) {
        Ok(roots) => roots,
        Err(error) => return print_error(err, &error),
    };
    let lock = match InstanceLock::acquire(&roots) {
        Ok(lock) => lock,
        Err(LockError::Held) => {
            return print_error(
                err,
                &"another process holds the instance lock, so this agent was not started",
            );
        }
        Err(error) => return print_error(err, &error),
    };
    let applied = applied_overrides(&options);
    let log = match AgentLog::open(roots.log_directory(), applied.max_log_bytes) {
        Ok(log) => Rc::new(log),
        Err(error) => return print_error(err, &error),
    };
    hide_panic_messages();
    let launcher = match contain() {
        Ok(launcher) => launcher,
        Err(error) => {
            return log_error(
                &*log,
                &format!("the children of this process cannot be contained: {error}"),
            );
        }
    };
    let registrar = match GitHubRegistrar::new(options.github_api, HttpLimits::default()) {
        Ok(registrar) => registrar,
        Err(error) => return log_error(&*log, &error),
    };
    let reader = match GitHubReader::new(options.github_api, HttpLimits::default()) {
        Ok(reader) => reader,
        Err(error) => return log_error(&*log, &error),
    };
    let runtime = match single_threaded_runtime() {
        Ok(runtime) => runtime,
        Err(error) => {
            return log_error(&*log, &format!("the async runtime cannot start: {error}"));
        }
    };
    let program = resolve_program(options.docker);
    let environment = applied.environment;
    let backend = DockerBackend::new(
        DockerConfig {
            program: program.clone(),
            environment: environment.clone(),
            launcher,
            exporter: std::env::current_exe().ok(),
            ..DockerConfig::default()
        },
        SystemClock,
        log.clone(),
    );
    let builder = DockerImageBuilder {
        program,
        environment,
        definition: options.definition.map(Path::to_path_buf),
    };
    let store = applied.store.or_else(|| platform_store(&*log));
    let in_effect = Current::default();
    #[cfg(windows)]
    let monitor = windows_monitor(options.keep_awake, &in_effect, &*log);
    #[cfg(not(windows))]
    let monitor = NoMonitor;
    let resident = Resident {
        backend: &backend,
        clock: &SystemClock,
        registrar: &registrar,
        reader: &reader,
        builder: &builder,
        monitor: &monitor,
        in_effect: &in_effect,
        store,
        log: &*log,
        roots: &roots,
        lock: &lock,
        settings: Settings {
            version: env!("CARGO_PKG_VERSION").to_string(),
            config_folder: roots.config_directory().to_path_buf(),
            profile: applied.profile,
            times: StopTimes::default(),
            launch: options.launch,
            drain_bound: DRAIN_BOUND,
        },
        executable: applied.executable,
        limits: applied.limits,
    };
    let ended = runtime.block_on(run_resident(resident));
    // the runtime is not waited for: on Windows a stalled client can keep a blocking
    // task of the pipe crate alive for as long as it stays, and the exit and the lock
    // must not wait for it
    runtime.shutdown_background();
    match ended {
        Ok(ended) => finish(&*log, &ended),
        Err(error) => log_error(&*log, &error),
    }
}

fn finish(log: &dyn Log, ended: &Ended) -> u8 {
    if !ended.kept.is_empty() {
        log.warn(&format!(
            "The agent has ended and kept the diagnostic containers {}; the next start with \
             their pools' diagnostics on exports and removes them.",
            ended.kept.join(", ")
        ));
        return EXIT_FAILED;
    }
    if ended.containers_removed {
        log.info(&format!("The agent has ended: {}.", ended.reason));
        EXIT_OK
    } else {
        log.warn(
            "The agent has ended, but a container could not be removed; the next start \
             removes it.",
        );
        EXIT_FAILED
    }
}

fn log_error(log: &dyn Log, error: &dyn std::fmt::Display) -> u8 {
    log.info(&format!("error: {error}"));
    EXIT_FAILED
}

/// the Windows signals, holding the keep-awake request the executable supplied on a
/// thread of its own; a thread that cannot start leaves the request untaken.
#[cfg(windows)]
fn windows_monitor<'a>(
    request: Option<crate::windows_host::keep_awake::Request>,
    in_effect: &'a Current,
    log: &'a dyn Log,
) -> crate::windows_host::monitor::WindowsMonitor<
    'a,
    crate::windows_host::signals::WindowsSignals,
    SystemClock,
> {
    use crate::windows_host::keep_awake::{Holder, KeepAwake, RETRY_INTERVAL};
    let keep_awake = request.and_then(|request| match Holder::spawn(request) {
        Ok(holder) => Some(KeepAwake::new(holder, RETRY_INTERVAL)),
        Err(error) => {
            log.warn(&format!(
                "The keep-awake thread could not start ({error}), so Windows may sleep when idle."
            ));
            None
        }
    });
    crate::windows_host::monitor::WindowsMonitor::new(
        crate::windows_host::signals::WindowsSignals::real(keep_awake),
        in_effect,
        &SystemClock,
        log,
        crate::macos::monitor::Cadence::default(),
    )
}
