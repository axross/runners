//! `build --profile axmc`: makes the golden VM in a build VM and commits it by renaming
//! the build VM to the golden name; any failure or stop before the rename deletes the
//! build VM and leaves the previous golden VM as it was; see `docs/operations/cli.md`.

use std::future::Future;
use std::pin::pin;
use std::time::{Duration, SystemTime};

use tokio::io::AsyncWriteExt;
use tokio::sync::watch;
use tokio::time::Instant;

use crate::golden::key::check_key_listing;
use crate::golden::pins::BASE_REFERENCE;
use crate::golden::steps::{
    GuestCall, build_vm_docker_command, disable_ssh_call, fetch_key_call, install_docker_call,
    install_launcher_call, list_key_call, prepare_call,
};
use crate::image::check::{ImageCheck, ImageLabels, checked_reference};
use crate::macos::prerequisites::{
    Check, VERSION_BUDGET, check_softnet, check_softnet_with, check_tart,
};
use crate::macos::softnet::OwnerMode;
use crate::runtime::clock::SystemClock;
use crate::runtime::docker::forward_lines;
use crate::runtime::docker_args::{image_inspect_arguments, info_arguments};
use crate::runtime::log::Log;
use crate::runtime::scrub::scrub_error_text;
use crate::runtime::tart::boot::{AgentWait, BUILD_VM, BootEnd, boot_failure_text, wait_for_agent};
use crate::runtime::tart::list::{VmEntry, VmSource};
use crate::runtime::tart::names::{GoldenVmName, build_vm_name, golden_vms, leftover_build_vms};
use crate::runtime::tart::teardown::{Stop, tear_down};
use crate::runtime::tart::{Captured, HeldVm, TartClient, VmLauncher};

use super::arguments::{build_arguments, image_remove_arguments, tag_arguments, temporary_tag};
use super::steps::unix_seconds;

const PROVISIONING_STEPS: usize = 6;

/// the time bounds of the golden VM build, which tests shorten. The calls that
/// provision the VM carry their own budgets, and the image build has none, as
/// `docker build` has none on axpc.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GoldenTimings {
    /// the build VM's boot: from `tart run` to the guest agent's first answer.
    pub agent: AgentWait,
    /// how long the VM's Docker Engine gets to answer once the agent does.
    pub engine_wait: Duration,
    pub engine_poll: Duration,
    /// the bound on one question to the Docker Engine while waiting for it.
    pub engine_call: Duration,
    /// the bound on each other `docker` client the image build runs.
    pub docker_call: Duration,
    /// the wait Tart gives the build VM to shut itself down, and the bound on the call.
    pub shutdown_grace: Duration,
    pub shutdown_budget: Duration,
    /// how long `tart run` has to end once the VM has stopped.
    pub run_end: Duration,
    /// the budget to get rid of one leftover build VM.
    pub leftover: Duration,
    /// the budget to delete the build VM after a failure or a stop. With the commit's
    /// two waits it fits inside the stop's 28 s exit deadline.
    pub cleanup: Duration,
    /// how long a rename under way may still finish once a stop is asked for, and the
    /// bound on the listing that decides an interrupted one.
    pub commit_grace_after_stop: Duration,
    pub commit_listing: Duration,
}

impl Default for GoldenTimings {
    fn default() -> Self {
        Self {
            agent: AgentWait {
                deadline: Duration::from_secs(120),
                poll: Duration::from_secs(1),
                call: Duration::from_secs(5),
            },
            engine_wait: Duration::from_secs(60),
            engine_poll: Duration::from_secs(2),
            engine_call: Duration::from_secs(10),
            docker_call: Duration::from_secs(120),
            shutdown_grace: Duration::from_secs(60),
            shutdown_budget: Duration::from_secs(90),
            run_end: Duration::from_secs(15),
            leftover: Duration::from_secs(15),
            cleanup: Duration::from_secs(15),
            commit_grace_after_stop: Duration::from_secs(5),
            commit_listing: Duration::from_secs(4),
        }
    }
}

/// one build of the golden VM, with `image` as the runner image's name inside it.
pub struct GoldenBuild<'a> {
    pub client: TartClient,
    pub launcher: VmLauncher,
    pub image: &'a str,
    /// the archive streamed to `docker build -` in the VM.
    pub context: Vec<u8>,
    pub log: &'a dyn Log,
    pub timings: GoldenTimings,
    /// the base pins to remove from Tart's cache after the commit.
    pub retired: &'a [&'a str],
    /// the owner and mode Softnet's file is judged to have, as a test gives it; the
    /// environment's override applies when it is unset.
    pub softnet_owner_mode: Option<OwnerMode>,
    /// when a stop was asked for; it is set once.
    pub stop: watch::Receiver<Option<Instant>>,
}

/// why a step ended the build.
enum Abort {
    /// a prerequisite is not met; nothing was changed.
    Refused(String),
    Stopped,
    Failed(String),
    /// the rename was cut short and a listing could not say whether it took effect.
    CommitUnknown(String),
}

impl From<String> for Abort {
    fn from(reason: String) -> Self {
        Self::Failed(reason)
    }
}

impl Abort {
    fn message(self) -> String {
        match self {
            Self::Refused(reason) | Self::CommitUnknown(reason) => reason,
            Self::Stopped => "the build was stopped before the golden VM was complete, so the \
                              golden VM is unchanged"
                .to_string(),
            Self::Failed(reason) => {
                format!("the golden VM build failed ({reason}), so the golden VM is unchanged")
            }
        }
    }
}

/// the build VM, from its name being chosen until it is committed or deleted.
#[derive(Default)]
struct Session {
    vm: Option<String>,
    held: Option<HeldVm>,
}

fn text_of(error: &impl std::fmt::Display) -> String {
    scrub_error_text(&error.to_string(), "")
}

impl GoldenBuild<'_> {
    /// runs the build until the golden VM holds the new image; the error says why not.
    /// A stop after the commit ends the clean-up of the old golden VMs, not the build.
    pub async fn run(&self) -> Result<(), String> {
        let mut session = Session::default();
        let Err(abort) = self.make(&mut session).await else {
            return Ok(());
        };
        self.remove_build_vm(&session).await;
        Err(abort.message())
    }

    async fn make(&self, session: &mut Session) -> Result<(), Abort> {
        self.guard(self.check_prerequisites())
            .await
            .map_err(|abort| match abort {
                Abort::Failed(reason) => Abort::Refused(reason),
                other => other,
            })?;
        self.guard(self.delete_leftovers()).await?;
        self.log.info(&format!("Pulling {BASE_REFERENCE}."));
        self.guard(async {
            self.client
                .pull(BASE_REFERENCE)
                .await
                .map_err(|e| text_of(&e))
        })
        .await?;
        let now = SystemTime::now();
        let vm = build_vm_name(now);
        // recorded before the clone, so a clone that fails halfway is still deleted
        session.vm = Some(vm.clone());
        self.log
            .info(&format!("Cloning the base image to the build VM {vm}."));
        self.guard(async {
            self.client
                .clone_vm(BASE_REFERENCE, &vm)
                .await
                .map_err(|e| text_of(&e))
        })
        .await?;
        self.boot(session, &vm).await?;
        self.provision(&vm).await?;
        let built = self.build_runner_image(&vm).await?;
        self.shut_down(session, &vm).await?;
        let golden = GoldenVmName::current(built)
            .ok_or_else(|| Abort::Failed("the image definition declares no runner version".into()))?
            .to_string();
        self.commit(session, &vm, &golden).await?;
        self.log.info(&format!(
            "The golden VM {golden} is built, with {} inside.",
            self.image
        ));
        self.retire_old(&golden).await;
        Ok(())
    }

    /// completes when a stop is asked for, with its time; never, when the sender is
    /// gone without one.
    async fn stopped(&self) -> Instant {
        super::stop_requested(&self.stop).await
    }

    /// runs `work` until it ends or a stop is asked for, which drops it, killing the
    /// client it runs.
    async fn guard<T>(&self, work: impl Future<Output = Result<T, String>>) -> Result<T, Abort> {
        tokio::select! {
            biased;
            _ = self.stopped() => Err(Abort::Stopped),
            done = work => done.map_err(Abort::Failed),
        }
    }

    async fn check_prerequisites(&self) -> Result<(), String> {
        let tart = self.accepted(check_tart(&self.client).await)?;
        let path = self.client.config().path.as_deref();
        let softnet = self.accepted(match self.softnet_owner_mode {
            Some(owner_mode) => check_softnet_with(path, Some(owner_mode), VERSION_BUDGET).await,
            None => check_softnet(path).await,
        })?;
        self.log
            .info(&format!("Using Tart {tart} and Softnet {softnet}."));
        Ok(())
    }

    /// the version of a check that lets the build run, warning when it carries one.
    fn accepted(&self, check: Check) -> Result<String, String> {
        match check {
            Check::Pass { version } => Ok(version),
            Check::Warn { version, message } => {
                self.log.warn(&message);
                Ok(version)
            }
            Check::Refuse(refusal) => Err(refusal.message),
        }
    }

    /// deletes the build VMs an earlier build left, which a killed CLI leaves to its
    /// shim and a killed shim leaves to this; a VM that cannot be deleted is a warning.
    async fn delete_leftovers(&self) -> Result<(), String> {
        let entries = self
            .client
            .list()
            .await
            .map_err(|error| format!("the VMs could not be listed ({})", text_of(&error)))?;
        for leftover in leftover_build_vms(&entries) {
            let name = &leftover.name;
            self.log.info(&format!(
                "Deleting the build VM {name}, left by an earlier build."
            ));
            if let Err(error) =
                tear_down(&self.client, name, Stop::Forced, self.timings.leftover).await
            {
                self.log
                    .warn(&format!("{error}; the next build tries again."));
            }
        }
        Ok(())
    }

    /// starts the build VM under the launcher and waits for its guest agent.
    async fn boot(&self, session: &mut Session, vm: &str) -> Result<(), Abort> {
        self.log.info(&format!("Booting the build VM {vm}."));
        let held = HeldVm::start(&self.client, &self.launcher, vm).map_err(|error| {
            Abort::Failed(boot_failure_text(
                BUILD_VM,
                vm,
                &BootEnd::NotStarted(text_of(&error)),
            ))
        })?;
        let held = session.held.insert(held);
        let waited = wait_for_agent(&self.client, &SystemClock, self.timings.agent, vm, held);
        self.guard(async {
            waited
                .await
                .map_err(|end| boot_failure_text(BUILD_VM, vm, &end))
        })
        .await
    }

    /// runs `command` in the VM and returns what it printed; the error begins with
    /// `failure`, whether the client failed or the command did.
    async fn exec_checked(
        &self,
        vm: &str,
        command: &[String],
        input: Option<&[u8]>,
        budget: Duration,
        failure: &str,
    ) -> Result<Captured, String> {
        let answer = self
            .client
            .exec(vm, command, input, budget)
            .await
            .map_err(|error| format!("{failure}: {}", text_of(&error)))?;
        answer.checked(failure).map_err(|error| error.to_string())
    }

    /// runs one guest call and returns what it printed, a failure naming `what`.
    async fn guest(&self, vm: &str, call: &GuestCall, what: &str) -> Result<Captured, String> {
        self.exec_checked(
            vm,
            &call.command,
            call.input.as_deref(),
            call.budget,
            &format!("{what} failed"),
        )
        .await
    }

    async fn step(
        &self,
        vm: &str,
        number: usize,
        what: &str,
        call: &GuestCall,
    ) -> Result<Captured, Abort> {
        self.log.info(&format!(
            "Provisioning the golden VM, step {number} of {PROVISIONING_STEPS}: {what}."
        ));
        self.guard(self.guest(vm, call, what)).await
    }

    /// the six guest calls of the golden VM's provisioning, in order; the key's
    /// fingerprint must match before anything is installed from Docker's repository.
    async fn provision(&self, vm: &str) -> Result<(), Abort> {
        self.step(vm, 1, "upgrading the Ubuntu packages", &prepare_call())
            .await?;
        self.step(vm, 2, "downloading Docker's signing key", &fetch_key_call())
            .await?;
        let listing = self
            .step(vm, 3, "checking Docker's signing key", &list_key_call())
            .await?;
        let key =
            check_key_listing(&listing.stdout).map_err(|error| Abort::Failed(error.to_string()))?;
        self.step(
            vm,
            4,
            "installing Docker Engine",
            &install_docker_call(&key),
        )
        .await?;
        self.step(vm, 5, "installing the launcher", &install_launcher_call())
            .await?;
        self.step(vm, 6, "disabling the SSH login", &disable_ssh_call())
            .await?;
        Ok(())
    }

    /// runs `docker` as root in the VM within `budget`.
    async fn docker(
        &self,
        vm: &str,
        arguments: &[String],
        budget: Duration,
        failure: &str,
    ) -> Result<Captured, String> {
        self.exec_checked(
            vm,
            &build_vm_docker_command(arguments),
            None,
            budget,
            failure,
        )
        .await
    }

    /// builds the runner image under the temporary tag as axpc does, checks its contract
    /// label, and only then tags it as the configured name. The VM is new, so there are
    /// no earlier temporary tags to remove and no old images to prune. Returns the time
    /// taken just before the build, which the temporary tag and the `built` label carry
    /// and the golden VM's name must carry too, so its freshness counts from the image.
    async fn build_runner_image(&self, vm: &str) -> Result<SystemTime, Abort> {
        self.guard(self.wait_for_engine(vm)).await?;
        let now = SystemTime::now();
        let temporary = temporary_tag(self.image, now);
        self.log.info(&format!(
            "Building {} in the build VM under the temporary tag {temporary}.",
            self.image
        ));
        self.guard(self.docker_build(vm, &temporary, unix_seconds(now)))
            .await?;
        self.guard(self.check_contract(vm, &temporary)).await?;
        let budget = self.timings.docker_call;
        self.guard(async {
            self.docker(
                vm,
                &tag_arguments(&temporary, self.image),
                budget,
                &format!("tagging the build as {}", self.image),
            )
            .await
        })
        .await?;
        let removed = self
            .guard(async {
                self.docker(
                    vm,
                    &image_remove_arguments(&temporary),
                    budget,
                    "removing the temporary tag",
                )
                .await
            })
            .await;
        match removed {
            Ok(_) => Ok(now),
            Err(Abort::Failed(reason)) => {
                self.log.warn(&format!(
                    "The temporary tag {temporary} could not be removed ({reason})."
                ));
                Ok(now)
            }
            Err(other) => Err(other),
        }
    }

    /// waits until the Docker Engine in the VM answers; the install started it, but
    /// it may still be starting.
    async fn wait_for_engine(&self, vm: &str) -> Result<(), String> {
        let timings = self.timings;
        let deadline = Instant::now() + timings.engine_wait;
        loop {
            let answered = self
                .docker(vm, &info_arguments(), timings.engine_call, "docker info")
                .await;
            if answered.is_ok() {
                return Ok(());
            }
            if Instant::now() + timings.engine_poll >= deadline {
                return Err(format!(
                    "the Docker Engine in the build VM did not answer within {} s ({})",
                    timings.engine_wait.as_secs(),
                    answered.err().unwrap_or_default()
                ));
            }
            tokio::time::sleep(timings.engine_poll).await;
        }
    }

    /// runs the one `docker build` in the VM, streaming the context to its standard
    /// input and its output to the log in bounded, masked lines.
    async fn docker_build(&self, vm: &str, temporary: &str, built: u64) -> Result<(), String> {
        let command = build_vm_docker_command(&build_arguments(temporary, built));
        let mut child = self
            .client
            .spawn_exec(vm, &command)
            .map_err(|error| format!("the image build could not start: {}", text_of(&error)))?;
        let stdin = child.stdin.take();
        let context = &self.context;
        // a client that ends early closes the pipe; its exit code says why
        let feed = async move {
            if let Some(mut stdin) = stdin {
                let _ = stdin.write_all(context).await;
                let _ = stdin.shutdown().await;
            }
        };
        let (stdout, stderr) = (child.stdout.take(), child.stderr.take());
        let (_, _, _, status) = tokio::join!(
            feed,
            forward_lines(stdout, self.log),
            forward_lines(stderr, self.log),
            child.wait()
        );
        let status = status.map_err(|error| format!("the image build failed: {error}"))?;
        if status.success() {
            return Ok(());
        }
        Err(format!(
            "docker build exited with code {}",
            status.code().unwrap_or(-1)
        ))
    }

    /// reads the built image's labels in the VM and requires this CLI's contract.
    async fn check_contract(&self, vm: &str, temporary: &str) -> Result<(), String> {
        let answer = self
            .docker(
                vm,
                &image_inspect_arguments(&checked_reference(temporary)),
                self.timings.docker_call,
                "reading the built image's labels",
            )
            .await?;
        let labels = (!answer.stdout_cut)
            .then(|| ImageLabels::from_inspect(answer.stdout.as_bytes()))
            .flatten()
            .ok_or("the built image's labels are not in the form the inspect prints")?;
        match ImageCheck::of_labels(labels) {
            ImageCheck::Matching(_) => Ok(()),
            other => Err(other
                .refusal(self.image)
                .unwrap_or_else(|| "the built image has another image contract".to_string())),
        }
    }

    /// stops the build VM gracefully, so its disk is complete, and waits for its
    /// `tart run` to end.
    async fn shut_down(&self, session: &Session, vm: &str) -> Result<(), Abort> {
        self.log.info(&format!("Shutting the build VM {vm} down."));
        let timings = self.timings;
        self.guard(async {
            self.client
                .stop(vm, timings.shutdown_grace, timings.shutdown_budget)
                .await
                .map_err(|error| format!("shutting the build VM down failed: {}", text_of(&error)))
        })
        .await?;
        let Some(held) = &session.held else {
            return Ok(());
        };
        self.guard(async {
            tokio::time::timeout(timings.run_end, held.ended())
                .await
                .map(drop)
                .map_err(|_| {
                    format!(
                        "tart run did not end within {} s of the build VM stopping",
                        timings.run_end.as_secs()
                    )
                })
        })
        .await
    }

    /// renames the build VM to the golden name, which commits the build. A rename cut
    /// short by a stop gets a few seconds to finish, and a listing then says whether
    /// it took effect.
    async fn commit(&self, session: &mut Session, vm: &str, golden: &str) -> Result<(), Abort> {
        self.log
            .info(&format!("Renaming the build VM {vm} to {golden}."));
        let timings = self.timings;
        let finished = {
            let mut rename = pin!(self.client.rename(vm, golden));
            tokio::select! {
                biased;
                done = &mut rename => Some(done),
                at = self.stopped() => {
                    tokio::time::timeout_at(at + timings.commit_grace_after_stop, &mut rename)
                        .await
                        .ok()
                }
            }
        };
        match finished {
            Some(Ok(())) => {
                session.vm = None;
                Ok(())
            }
            Some(Err(error)) => Err(Abort::Failed(format!(
                "renaming the build VM failed: {}",
                text_of(&error)
            ))),
            None => self.decide_commit(session, vm, golden).await,
        }
    }

    /// after a rename cut short, whether the golden name exists says if it committed.
    /// The `tart rename` client has ended by now, so the listing is not racing it.
    async fn decide_commit(
        &self,
        session: &mut Session,
        vm: &str,
        golden: &str,
    ) -> Result<(), Abort> {
        let listed = tokio::time::timeout(self.timings.commit_listing, self.client.list()).await;
        match listed {
            Ok(Ok(entries)) if has_local(&entries, golden) => {
                session.vm = None;
                Ok(())
            }
            Ok(Ok(_)) => Err(Abort::Stopped),
            _ => {
                // the build VM stays recorded: deleting it by its own name is safe whether or
                // not the rename took effect, and never reaches the golden name
                Err(Abort::CommitUnknown(format!(
                    "commit unknown: the rename was stopped and the VMs could not be listed; \
                     `tart list` shows whether {golden} exists, and the build VM {vm} is also \
                     deleted by its own name, with a warning if that fails"
                )))
            }
        }
    }

    /// after the commit: deletes every other golden VM and the retired base pins from
    /// Tart's cache. All of it is best effort, and a stop skips what is left.
    async fn retire_old(&self, golden: &str) {
        match self.guard(self.delete_old(golden)).await {
            Ok(()) => {}
            Err(Abort::Failed(reason)) => self
                .log
                .warn(&format!("{reason}; the next build tries again.")),
            Err(_) => self.log.info(
                "The stop skipped removing the old golden VMs and retired base images; the next \
                 build removes them.",
            ),
        }
    }

    async fn delete_old(&self, golden: &str) -> Result<(), String> {
        let entries = self.client.list().await.map_err(|error| {
            format!(
                "the old golden VMs could not be listed ({})",
                text_of(&error)
            )
        })?;
        let budget = self.timings.leftover;
        let others = golden_vms(&entries)
            .into_iter()
            .map(|(entry, _)| entry.name.as_str())
            .filter(|name| *name != golden);
        let retired = entries
            .iter()
            .filter(|entry| entry.source == VmSource::Oci)
            .map(|entry| entry.name.as_str())
            .filter(|name| *name != BASE_REFERENCE && self.retired.contains(name));
        for (name, what) in others
            .map(|name| (name, "the old golden VM"))
            .chain(retired.map(|name| (name, "the retired base image")))
        {
            self.log.info(&format!("Deleting {what} {name}."));
            if let Err(error) = self.client.delete(name, budget).await {
                self.log.warn(&format!(
                    "{what} {name} could not be deleted ({}); the next build tries again.",
                    text_of(&error)
                ));
            }
        }
        Ok(())
    }

    /// deletes the build VM, if the build has not committed it, within the cleanup
    /// budget; the VM's shim deletes it too when this process ends.
    async fn remove_build_vm(&self, session: &Session) {
        let Some(vm) = &session.vm else {
            return;
        };
        let stop = match &session.held {
            Some(held) if !held.has_ended() => Stop::Graceful,
            _ => Stop::NotStarted,
        };
        if let Err(error) = tear_down(&self.client, vm, stop, self.timings.cleanup).await {
            self.log.warn(&format!(
                "{error}; the next build deletes it if it is still there."
            ));
        }
        if let Some(held) = &session.held {
            held.close_liveness();
        }
    }
}

fn has_local(entries: &[VmEntry], name: &str) -> bool {
    entries
        .iter()
        .any(|entry| entry.source == VmSource::Local && entry.name == name)
}
