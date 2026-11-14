//! the life of a build VM that becomes a golden VM, whatever its OS: booting it
//! under the launcher, running guest calls under the stop signal, shutting it down,
//! committing it by renaming it, and deleting it when the build does not commit.

use std::future::Future;
use std::pin::pin;
use std::time::Duration;

use tokio::sync::watch;
use tokio::time::Instant;

use crate::golden::steps::GuestCall;
use crate::runtime::clock::SystemClock;
use crate::runtime::log::Log;
use crate::runtime::scrub::scrub_error_text;
use crate::runtime::tart::boot::{BUILD_VM, BootEnd, boot_failure_text, wait_for_agent};
use crate::runtime::tart::list::{VmEntry, VmSource};
use crate::runtime::tart::teardown::{Stop, tear_down};
use crate::runtime::tart::{Captured, HeldVm, TartClient, VmLauncher};

use super::golden::GoldenTimings;

/// why a step ended the build.
pub(super) enum Abort {
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
    /// the build's refusal or failure, naming the VM it builds as `vm`, such as "the
    /// golden VM".
    pub(super) fn message(self, vm: &str) -> String {
        match self {
            Self::Refused(reason) | Self::CommitUnknown(reason) => reason,
            Self::Stopped => {
                format!("the build was stopped before {vm} was complete, so {vm} is unchanged")
            }
            Self::Failed(reason) => format!("{vm} build failed ({reason}), so {vm} is unchanged"),
        }
    }
}

/// the build VM, from its name being chosen until it is committed or deleted.
#[derive(Default)]
pub(super) struct Session {
    pub(super) vm: Option<String>,
    pub(super) held: Option<HeldVm>,
}

pub(super) fn text_of(error: &impl std::fmt::Display) -> String {
    scrub_error_text(&error.to_string(), "")
}

/// what a golden build shares with its build VM's steps.
pub(super) struct BuildVm<'a> {
    pub(super) client: &'a TartClient,
    pub(super) launcher: &'a VmLauncher,
    pub(super) log: &'a dyn Log,
    pub(super) timings: GoldenTimings,
    pub(super) stop: &'a watch::Receiver<Option<Instant>>,
}

impl BuildVm<'_> {
    /// completes when a stop is asked for, with its time; never, when the sender is
    /// gone without one.
    pub(super) async fn stopped(&self) -> Instant {
        super::stop_requested(self.stop).await
    }

    /// runs `work` until it ends or a stop is asked for, which drops it, killing the
    /// client it runs.
    pub(super) async fn guard<T>(
        &self,
        work: impl Future<Output = Result<T, String>>,
    ) -> Result<T, Abort> {
        tokio::select! {
            biased;
            _ = self.stopped() => Err(Abort::Stopped),
            done = work => done.map_err(Abort::Failed),
        }
    }

    /// starts the build VM under the launcher and waits for its guest agent.
    pub(super) async fn boot(&self, session: &mut Session, vm: &str) -> Result<(), Abort> {
        self.log.info(&format!("Booting the build VM {vm}."));
        let held = HeldVm::start(self.client, self.launcher, vm).map_err(|error| {
            Abort::Failed(boot_failure_text(
                BUILD_VM,
                vm,
                &BootEnd::NotStarted(text_of(&error)),
            ))
        })?;
        let held = session.held.insert(held);
        let waited = wait_for_agent(self.client, &SystemClock, self.timings.agent, vm, held);
        self.guard(async {
            waited
                .await
                .map_err(|end| boot_failure_text(BUILD_VM, vm, &end))
        })
        .await
    }

    /// runs `command` in the VM and returns what it printed; the error begins with
    /// `failure`, whether the client failed or the command did.
    pub(super) async fn exec_checked(
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
    pub(super) async fn guest(
        &self,
        vm: &str,
        call: &GuestCall,
        what: &str,
    ) -> Result<Captured, String> {
        self.exec_checked(
            vm,
            &call.command,
            call.input.as_deref(),
            call.budget,
            &format!("{what} failed"),
        )
        .await
    }

    /// writes the files the build VM has not yet written to its disk, stops it, and
    /// waits for its `tart run` to end.
    pub(super) async fn shut_down(
        &self,
        session: &Session,
        vm: &str,
        flush: &GuestCall,
    ) -> Result<(), Abort> {
        self.log
            .info(&format!("Writing the build VM {vm}'s files to its disk."));
        self.guard(self.guest(vm, flush, "writing the build VM's files to its disk"))
            .await?;
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
    pub(super) async fn commit(
        &self,
        session: &mut Session,
        vm: &str,
        golden: &str,
    ) -> Result<(), Abort> {
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

    /// deletes the build VM, if the build has not committed it, within the cleanup
    /// budget; the VM's shim deletes it too when this process ends.
    pub(super) async fn remove_build_vm(&self, session: &Session) {
        let Some(vm) = &session.vm else {
            return;
        };
        let stop = match &session.held {
            Some(held) if !held.has_ended() => Stop::Graceful,
            _ => Stop::NotStarted,
        };
        if let Err(error) = tear_down(self.client, vm, stop, self.timings.cleanup).await {
            self.log.warn(&format!(
                "{error}; the next build deletes it if it is still there."
            ));
        }
        if let Some(held) = &session.held {
            held.close_liveness();
        }
    }
}

pub(super) fn has_local(entries: &[VmEntry], name: &str) -> bool {
    entries
        .iter()
        .any(|entry| entry.source == VmSource::Local && entry.name == name)
}
