//! `build`: builds the runner image the host configuration names, from the
//! definition embedded in this binary, as an instance in its own `building` state that
//! holds the instance lock throughout, so it is refused while any instance exists and
//! so is a `start` during it; see `docs/operations/cli.md`.

mod arguments;
mod steps;

use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::path::Path;
use std::pin::pin;
use std::time::Duration;

use tokio::sync::{mpsc, watch};
use tokio::time::Instant;

use crate::command::{EXIT_FAILED, EXIT_OK, checkout_rule_note, load_plan, print_error};
use crate::image::archive::context_archive;
use crate::image::definition::resolve_definition;
use crate::instance::channel::{ChannelServer, exchange};
use crate::instance::lock::{InstanceLock, LockError};
use crate::instance::protocol::{Incoming, Request, Response};
use crate::instance::roots::Roots;
use crate::profile::Profile;
use crate::runtime::docker::resolve_program;
use crate::runtime::log::{Log, StderrLog};
use crate::start::{ANSWERS_WITHIN, LOCK_HELD_ELSEWHERE, signals, single_threaded_runtime};

use steps::{BuildLimits, Steps};

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
    if request.profile == Profile::Axmc {
        return print_error(
            err,
            &"the configuration is valid, but axmc's runner image is a golden VM, and the golden VM build comes in Phase 6",
        );
    }
    let Some(image) = plan.image_name else {
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
        program: resolve_program(request.docker),
        log: &log,
    };
    match runtime.block_on(build.run(context)) {
        Ok(()) => EXIT_OK,
        Err(error) => print_error(err, &error),
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
    program: OsString,
    log: &'a dyn Log,
}

impl Build<'_> {
    /// opens the channel and runs the build.
    async fn run(&self, context: Vec<u8>) -> Result<(), String> {
        let (sender, requests) = mpsc::unbounded_channel();
        let server = ChannelServer::open(self.lock, self.roots, sender.clone())
            .map_err(|error| error.to_string())?;
        let (stop, stop_receiver) = watch::channel(None);
        let steps = Steps {
            program: self.program.clone(),
            image: self.image,
            context,
            log: self.log,
            limits: BuildLimits::default(),
            stop: stop_receiver,
        };
        let outcome = {
            // the receiver ends with this block, before the channel closes
            let mut requests = requests;
            tokio::select! {
                outcome = self.serve(steps.run(), &mut requests, &stop) => outcome,
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
                         ended now; `docker image inspect {}` tells which image the name holds",
                        EXIT_DEADLINE_AFTER_STOP.as_secs(),
                        self.image
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
