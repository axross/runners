//! `build`: builds the runner image the host configuration names, from the
//! definition embedded in this binary, as an instance in its own `building` state that
//! holds the instance lock throughout, so it is refused while any instance exists and
//! so is a `start` during it. On axmc the image is built inside the golden VM and
//! `build` makes that VM; see `docs/operations/cli.md`.

mod arguments;
mod build_vm;
pub mod golden;
pub mod golden_macos;
pub(crate) mod steps;

use std::ffi::OsStr;
use std::future::Future;
use std::io::Write;
use std::path::Path;
use std::pin::pin;
use std::time::Duration;

use tokio::sync::{mpsc, watch};
use tokio::time::Instant;

use crate::command::{EXIT_FAILED, EXIT_OK, checkout_rule_note, load_plan, print_error};
use crate::golden::pins::RETIRED_BASE_PINS;
use crate::image::archive::context_archive;
use crate::image::definition::resolve_definition;
use crate::image::macos_base::MACOS_BASE;
use crate::image::runner::{TEST_RUNNER_VARIABLE, declared_runner};
use crate::instance::channel::{ChannelServer, exchange};
use crate::instance::lock::{InstanceLock, LockError};
use crate::instance::protocol::{Incoming, Request, Response};
use crate::instance::roots::Roots;
use crate::macos::count::ConfiguredPool;
use crate::profile::{Os, Profile, host_refusal};
use crate::runtime::docker::resolve_program;
use crate::runtime::log::{Log, StderrLog};
use crate::runtime::pool::DEFAULT_IMAGE_NAME;
use crate::runtime::tart::client::program_from_environment;
use crate::runtime::tart::{TartClient, TartConfig, VmLauncher};
use crate::start::{ANSWERS_WITHIN, LOCK_HELD_ELSEWHERE, signals, single_threaded_runtime};

use golden::{GoldenBuild, GoldenTimings};
use golden_macos::{GoldenMacosBuild, MACOS_AGENT_DEADLINE, MACOS_PULL_BUDGET};
use steps::{BuildLimits, BuildOutput, Steps};

const HOLDER_ANSWER_WITHIN: Duration = Duration::from_secs(5);

/// whatever still runs at this deadline is dropped, which kills its client, so
/// `build` ends inside `stop`'s 30 s.
const EXIT_DEADLINE_AFTER_STOP: Duration = Duration::from_secs(28);

/// what `build` was given.
pub struct BuildRequest<'a> {
    pub config: &'a Path,
    pub profile: Profile,
    /// moves the lock, channel and log roots; a debug build only.
    pub test_root: Option<&'a Path>,
    /// names the `docker` client; a debug build only.
    pub docker: Option<&'a OsStr>,
    /// names a test definition to build instead of the embedded one; a debug build only.
    pub definition: Option<&'a Path>,
}

/// runs `build` and returns its exit code; Docker is reached only after the lock,
/// validation and the tag check.
pub fn build(request: &BuildRequest<'_>, err: &mut dyn Write) -> u8 {
    if let Some(refusal) = host_refusal(request.profile, cfg!(windows)) {
        return print_error(err, &refusal);
    }
    let roots = match Roots::resolve(request.test_root) {
        Ok(roots) => roots,
        Err(error) => return print_error(err, &error),
    };
    let lock = match InstanceLock::acquire(&roots) {
        Ok(lock) => lock,
        Err(LockError::Held) => return refuse_held(&roots, err),
        Err(error) => return print_error(err, &error),
    };
    let loaded = load_plan(request.config, request.profile);
    if loaded.checkout_rule_skipped {
        let _ = writeln!(err, "{}", checkout_rule_note(request.config));
    }
    let plan = match loaded.outcome {
        Ok(plan) => plan,
        Err(error) => {
            let _ = writeln!(err, "{}", error.message(request.config));
            return EXIT_FAILED;
        }
    };
    let Some(image) = plan
        .image_name
        .clone()
        .or_else(|| (request.profile == Profile::Axmc).then(|| DEFAULT_IMAGE_NAME.to_string()))
    else {
        return print_error(err, &"the configuration names no image to build");
    };
    if image.contains('@') {
        return print_error(
            err,
            &format!(
                "the image name {image} names a digest, which cannot be tagged; give it a tag instead"
            ),
        );
    }
    let context = match resolve_definition(request.definition)
        .and_then(|definition| context_archive(&definition.files))
    {
        Ok(context) => context,
        Err(error) => return print_error(err, &error),
    };
    let log = StderrLog::default();
    log.info(&format!("Building {image}."));
    #[cfg(windows)]
    if let Err(error) = crate::start::contain() {
        return print_error(
            err,
            &format!("the children of this process cannot be contained: {error}"),
        );
    }
    let runtime = match single_threaded_runtime() {
        Ok(runtime) => runtime,
        Err(error) => return print_error(err, &format!("the async runtime cannot start: {error}")),
    };
    let build = Build {
        lock: &lock,
        roots: &roots,
        image: &image,
        log: &log,
        after_deadline: match request.profile {
            Profile::Axpc => {
                format!("`docker image inspect {image}` tells which image the name holds")
            }
            Profile::Axmc => {
                "the build VM is deleted by its shim, or by the next build".to_string()
            }
        },
    };
    let outcome = match request.profile {
        Profile::Axpc => {
            let program = resolve_program(request.docker);
            let (image, log) = (&image, &log);
            runtime.block_on(build.run(|stop| async move {
                Steps {
                    program,
                    environment: Vec::new(),
                    image,
                    context,
                    log,
                    limits: BuildLimits::default(),
                    output: BuildOutput::Inherit,
                    stop,
                }
                .run()
                .await
            }))
        }
        Profile::Axmc => {
            let launcher = match golden_launcher() {
                Ok(launcher) => launcher,
                Err(error) => return print_error(err, &error),
            };
            let client = TartClient::new(TartConfig {
                program: program_from_environment(),
                ..TartConfig::default()
            });
            let pools = ConfiguredPool::of_plan(&plan);
            let builds_linux = pools.iter().any(|pool| pool.os == Os::Linux);
            let builds_macos = pools.iter().any(|pool| pool.os == Os::Macos);
            let (image, log) = (&image, &log);
            // boxed: the whole golden build is a large future, which `block_on` would
            // otherwise hold on the main thread's stack
            runtime.block_on(build.run(|stop| {
                Box::pin(async move {
                    let mut failures = Vec::new();
                    if builds_linux {
                        let linux = GoldenBuild {
                            client: client.clone(),
                            launcher: launcher.clone(),
                            image,
                            context,
                            log,
                            timings: GoldenTimings::default(),
                            retired: RETIRED_BASE_PINS,
                            softnet_owner_mode: None,
                            stop: stop.clone(),
                        };
                        if let Err(error) = linux.run().await {
                            failures.push(error);
                        }
                    }
                    if builds_macos && stop.borrow().is_none() {
                        let macos = macos_build(client, launcher, pools, log, stop);
                        if let Err(error) = async { macos?.run().await }.await {
                            failures.push(error);
                        }
                    }
                    if failures.is_empty() {
                        Ok(())
                    } else {
                        Err(failures.join("; "))
                    }
                })
            }))
        }
    };
    match outcome {
        Ok(()) => EXIT_OK,
        Err(error) => print_error(err, &error),
    }
}

/// the golden macOS VM's build, with the declared runner and this Mac's macOS
/// version; the error says which of them could not be read.
fn macos_build<'a>(
    client: TartClient,
    launcher: VmLauncher,
    pools: Vec<ConfiguredPool>,
    log: &'a dyn Log,
    stop: watch::Receiver<Option<Instant>>,
) -> Result<GoldenMacosBuild<'a>, String> {
    let requested = std::env::var(TEST_RUNNER_VARIABLE).ok();
    let runner = declared_runner(requested.as_deref())?;
    let host_macos = host_macos_version()?;
    let mut timings = GoldenTimings::default();
    timings.agent.deadline = MACOS_AGENT_DEADLINE;
    let mut config = client.config().clone();
    config.limits.pull_budget = MACOS_PULL_BUDGET;
    Ok(GoldenMacosBuild {
        client: TartClient::new(config),
        launcher,
        base: MACOS_BASE,
        runner,
        pools,
        host_macos,
        log,
        timings,
        softnet_owner_mode: None,
        stop,
    })
}

/// this Mac's macOS version, as `sw_vers -productVersion` prints it.
fn host_macos_version() -> Result<String, String> {
    let output = std::process::Command::new("sw_vers")
        .arg("-productVersion")
        .output()
        .map_err(|error| format!("this Mac's macOS version could not be read: {error}"))?;
    let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !output.status.success() || version.is_empty() {
        return Err("this Mac's macOS version could not be read: sw_vers printed none".into());
    }
    Ok(version)
}

/// holds the build VM under the VM shim, which is this program run again, so a CLI that
/// dies leaves the VM to the shim.
fn golden_launcher() -> Result<VmLauncher, String> {
    #[cfg(unix)]
    {
        std::env::current_exe()
            .map(VmLauncher::Shim)
            .map_err(|error| format!("the path of this program cannot be read: {error}"))
    }
    #[cfg(not(unix))]
    {
        Ok(VmLauncher::Direct)
    }
}

/// completes when a stop is asked for, with its time; never, when the sender is gone
/// without one.
pub(crate) async fn stop_requested(stop: &watch::Receiver<Option<Instant>>) -> Instant {
    let mut stop = stop.clone();
    loop {
        if let Some(at) = *stop.borrow_and_update() {
            return at;
        }
        if stop.changed().await.is_err() {
            return std::future::pending().await;
        }
    }
}

/// names the holder's state when this user's channel answers.
fn refuse_held(roots: &Roots, err: &mut dyn Write) -> u8 {
    match exchange(roots, &Request::Status, HOLDER_ANSWER_WITHIN) {
        Ok(response) => {
            let state = response.field("state").unwrap_or("unknown");
            print_error(
                err,
                &format!(
                    "an instance exists and is {state}, so nothing was built; `marshal stop` ends it"
                ),
            )
        }
        Err(_) => print_error(err, &LOCK_HELD_ELSEWHERE),
    }
}

struct Build<'a> {
    lock: &'a InstanceLock,
    roots: &'a Roots,
    image: &'a str,
    log: &'a dyn Log,
    /// ends the message for a build still running at the exit deadline.
    after_deadline: String,
}

impl Build<'_> {
    /// opens the channel and runs the build that `steps` makes from the stop signal.
    async fn run<S, F>(&self, steps: S) -> Result<(), String>
    where
        S: FnOnce(watch::Receiver<Option<Instant>>) -> F,
        F: Future<Output = Result<(), String>>,
    {
        let (sender, requests) = mpsc::unbounded_channel();
        let server = ChannelServer::open(self.lock, self.roots, sender.clone())
            .map_err(|error| error.to_string())?;
        let (stop, stop_receiver) = watch::channel(None);
        let steps = steps(stop_receiver);
        let outcome = {
            // the receiver ends with this block, before the channel closes
            let mut requests = requests;
            tokio::select! {
                outcome = self.serve(steps, &mut requests, &stop) => outcome,
                never = signals(&sender, None, self.log) => match never {},
            }
        };
        server.close(ANSWERS_WITHIN).await;
        outcome
    }

    /// runs the steps while answering requests, ending them at the exit deadline
    /// once a stop is asked for.
    async fn serve(
        &self,
        steps: impl std::future::Future<Output = Result<(), String>>,
        requests: &mut mpsc::UnboundedReceiver<Incoming>,
        stop: &watch::Sender<Option<Instant>>,
    ) -> Result<(), String> {
        let started = Instant::now();
        let mut steps = pin!(steps);
        let mut open = true;
        loop {
            let exit_by = stop.borrow().map(|at| at + EXIT_DEADLINE_AFTER_STOP);
            tokio::select! {
                outcome = &mut steps => return outcome,
                () = tokio::time::sleep_until(exit_by.unwrap_or(started)), if exit_by.is_some() => {
                    return Err(format!(
                        "the build did not end within {} s of the stop, so what still runs is \
                         ended now; {}",
                        EXIT_DEADLINE_AFTER_STOP.as_secs(),
                        self.after_deadline
                    ));
                }
                incoming = requests.recv(), if open => match incoming {
                    None => open = false,
                    Some(incoming) => {
                        let response = self.answer(&incoming.request, stop, started);
                        let _ = incoming.reply.send(response);
                    }
                },
            }
        }
    }

    fn answer(
        &self,
        request: &Request,
        stop: &watch::Sender<Option<Instant>>,
        started: Instant,
    ) -> Response {
        let state = if stop.borrow().is_some() {
            "stopping"
        } else {
            "building"
        };
        match request {
            Request::Status => Response::ok("")
                .with("state", state)
                .with("image", self.image)
                .with("elapsed", &format!("{} s", started.elapsed().as_secs())),
            Request::Stop => {
                if stop.borrow().is_none() {
                    self.log.info("Stopping: the build is aborted.");
                    stop.send_replace(Some(Instant::now()));
                }
                Response::ok("stopping")
            }
            Request::Restart | Request::Diagnostics | Request::Resume(_) => {
                Response::refused(&format!("the instance is building {}", self.image))
                    .with("state", state)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stop_in_the_rename_finishes_its_cleanup_inside_the_exit_deadline() {
        let timings = GoldenTimings::default();
        let after_stop = timings.commit_grace_after_stop + timings.commit_listing + timings.cleanup;
        assert!(
            after_stop < EXIT_DEADLINE_AFTER_STOP,
            "{after_stop:?} is not inside {EXIT_DEADLINE_AFTER_STOP:?}"
        );
    }
}
