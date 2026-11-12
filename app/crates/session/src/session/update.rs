//! the session's part in an update: it checks the feed at launch and every hour, holds
//! a verified update, asks its agent to drain for it once the agent is running or
//! stopped, waits for the agent's process to exit after `ending`, writes the record of
//! "stopped by you", and starts the installer. An older agent that drains is replaced
//! the same way, with no install. Every other end of the agent is handled as W1's app
//! handles one.

use std::sync::Arc;
use std::time::Duration;

use marshal_cli::agent::Launch;
use marshal_cli::agent::wire::UpdateEnding;
use marshal_feed::{DEVELOPMENT_VERSION, newer};
use tokio::time::Instant;

use super::{CALL_WITHIN, Ended, Internal, Session, Stage, stopped_by_you};
use crate::seam::{AgentInfo, Dial, Failure, Link, Process};

/// how often the feed is read while the app runs.
pub const CHECK_EVERY: Duration = Duration::from_secs(60 * 60);

/// how long the app waits for the agent's process to exit after it said it is ending.
pub const EXIT_WITHIN: Duration = Duration::from_secs(60);

/// how long before a drain the agent did not take is asked for again.
const DRAIN_AGAIN: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Purpose {
    /// for the verified update, which installs once the agent has exited.
    Install,
    /// for an older agent, which this app's own agent replaces.
    ReplaceOlder,
}

/// a drain this app asked its agent for.
pub struct Drain {
    version: String,
    purpose: Purpose,
    /// the agent answered that it drains.
    answered: bool,
    pid: Option<u32>,
    process: Option<Arc<dyn Process>>,
    /// no process with the agent's ID runs any more.
    gone: bool,
    /// the agent said it is ending for the update, and whether the runners were stopped
    /// by you.
    ending: Option<bool>,
    /// the wait for the agent's process has begun.
    waiting: bool,
    /// the connection dropped during the drain, and the app is reaching the agent again
    /// without starting it.
    lost: bool,
}

impl<D: Dial> Session<D> {
    fn checks(&self) -> bool {
        self.updater.is_some() && self.version != DEVELOPMENT_VERSION
    }

    pub(super) fn first_check(&mut self) {
        if self.checks() {
            self.next_check = Some(Instant::now());
        }
    }

    /// when the next check is due, while one may run: never while the app holds a
    /// verified update or drains.
    pub(super) fn check_due(&self) -> Option<Instant> {
        let may = !self.checking && self.verified.is_none() && self.drain.is_none();
        self.next_check.filter(|_| may)
    }

    pub(super) fn update_due(&mut self, now: Instant) {
        if self.check_due().is_some_and(|at| now >= at) {
            self.begin_check();
        }
        if self.drain_again_at.is_some_and(|at| now >= at) {
            self.drain_again_at = None;
            self.maybe_drain();
        }
    }

    fn begin_check(&mut self) {
        let Some(updater) = self.updater.clone() else {
            return;
        };
        self.checking = true;
        self.next_check = Some(Instant::now() + CHECK_EVERY);
        let report = self.report.clone();
        tokio::spawn(async move {
            let outcome = updater.check().await;
            let _ = report.send(Internal::Checked(outcome));
        });
    }

    pub(super) fn checked(&mut self, outcome: Result<Option<Box<dyn super::Verified>>, String>) {
        self.checking = false;
        match outcome {
            Ok(Some(verified)) => {
                self.log.info(&format!(
                    "The update to {} is downloaded and its signature verified; it installs once the agent's running jobs have finished.",
                    verified.version()
                ));
                self.verified = Some(verified);
                self.maybe_drain();
            }
            Ok(None) => self.log.info("The update feed has no newer release."),
            Err(why) => self.log.warn(&format!(
                "The update check failed: {why}. The next check is in an hour."
            )),
        }
    }

    /// an agent older than this app, which this app's own agent replaces.
    fn older_agent(&self) -> bool {
        self.version != DEVELOPMENT_VERSION && newer(&self.agent.version, &self.version) == Ok(true)
    }

    pub(super) fn connected_to(&mut self, agent: AgentInfo) {
        // the agent ended an earlier drain with the connection that asked for it
        if self.drain.as_ref().is_some_and(|drain| drain.lost) {
            self.drain = None;
        }
        self.agent = agent;
        self.told_no_drain = false;
        if self.older_agent() {
            self.log.info(&format!(
                "The agent is version {}, older than this app's {}.",
                marshal_view::plain(&self.agent.version),
                self.version
            ));
        }
    }

    /// asks the agent to drain when this app has a reason to and the agent can: it is
    /// connected, running or stopped, with no drain of anyone's under way and no action
    /// of the window's waiting for its answer.
    pub(super) fn maybe_drain(&mut self) {
        if self.drain.is_some() || self.drain_again_at.is_some() {
            return;
        }
        let Some(link) = self.link() else {
            return;
        };
        let (version, purpose) = if self.older_agent() {
            (self.version.clone(), Purpose::ReplaceOlder)
        } else if let Some(verified) = &self.verified {
            (verified.version().to_string(), Purpose::Install)
        } else {
            return;
        };
        if !self.agent.drains {
            if !self.told_no_drain {
                self.told_no_drain = true;
                self.log.info(&format!(
                    "The agent, version {}, cannot drain for an update, so nothing installs while it runs.",
                    marshal_view::plain(&self.agent.version)
                ));
            }
            return;
        }
        let Some(snapshot) = &self.snapshot else {
            return;
        };
        let settled = matches!(snapshot.agent.state.as_str(), "running" | "stopped");
        if !settled || snapshot.agent.update.is_some() || self.pending.is_some() {
            return;
        }
        self.log.info(&format!(
            "Asking the agent to drain for {}.",
            match purpose {
                Purpose::Install => format!("the update to {version}"),
                Purpose::ReplaceOlder => format!("this app's version, {version}"),
            }
        ));
        self.drain = Some(Drain {
            version: version.clone(),
            purpose,
            answered: false,
            pid: None,
            process: None,
            gone: false,
            ending: None,
            waiting: false,
            lost: false,
        });
        let report = self.report.clone();
        tokio::spawn(async move {
            let outcome = tokio::time::timeout(CALL_WITHIN, link.drain_for_update(&version))
                .await
                .unwrap_or(Err(Failure::TimedOut));
            let _ = report.send(Internal::DrainAnswered(outcome));
        });
    }

    pub(super) fn drain_answered(&mut self, outcome: Result<Option<u32>, Failure>) {
        let Some(drain) = self.drain.as_mut() else {
            return;
        };
        if drain.answered || drain.lost {
            return;
        }
        match outcome {
            Ok(pid) => {
                drain.answered = true;
                drain.pid = drain.pid.or(pid);
                self.log.info(&format!(
                    "The agent drains for {}: nothing registers, and it ends once its running jobs have finished.",
                    drain.version
                ));
                self.open_process();
                self.begin_exit_wait();
            }
            Err(failure) if drain.ending.is_some() => {
                // the agent said it is ending for this drain, so its end is the answer
                self.log.warn(&format!(
                    "The agent ended for the update without answering the drain: {}",
                    failure.words()
                ));
                drain.answered = true;
                self.begin_exit_wait();
            }
            // the connection's end, which follows, is handled as a drop
            Err(Failure::Lost) => {}
            Err(failure) => {
                self.log.warn(&format!(
                    "The agent did not drain for {}: {} It is asked again in a minute.",
                    drain.version,
                    failure.words()
                ));
                self.drain = None;
                self.drain_again_at = Some(Instant::now() + DRAIN_AGAIN);
            }
        }
    }

    /// opens a handle to the agent's process once its ID is known, so the app can tell
    /// when it has exited.
    fn open_process(&mut self) {
        let Some(drain) = self.drain.as_mut() else {
            return;
        };
        let Some(pid) = drain.pid else {
            return;
        };
        if drain.process.is_some() || drain.gone {
            return;
        }
        match self.processes.open(pid) {
            Ok(Some(process)) => drain.process = Some(process),
            Ok(None) => drain.gone = true,
            Err(why) => self
                .log
                .warn(&format!("The agent's process cannot be watched: {why}")),
        }
    }

    /// a snapshot during this app's drain gives the agent's process ID when the answer
    /// did not; then whether a drain can be asked for now.
    pub(super) fn snapshot_for_drain(&mut self) {
        let shown = self
            .snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.agent.update.as_ref())
            .map(|update| (update.version.clone(), update.pid));
        if let (Some(drain), Some((version, pid))) = (self.drain.as_mut(), shown)
            && drain.version == version
            && drain.pid.is_none()
            && pid != 0
        {
            drain.pid = Some(pid);
            self.open_process();
        }
        self.maybe_drain();
    }

    /// whether `update` ends the drain this connection asked for.
    pub(super) fn drain_is_for(&self, update: &UpdateEnding) -> bool {
        matches!(self.stage, Stage::Live)
            && self
                .drain
                .as_ref()
                .is_some_and(|drain| !drain.lost && drain.version == update.version)
    }

    /// the agent said it is ending for this app's drain.
    pub(super) fn agent_ending(&mut self, stopped_by_you: bool) {
        let Some(drain) = self.drain.as_mut() else {
            return;
        };
        self.log.info(&format!(
            "The agent is ending for the update to {}{}.",
            drain.version,
            if stopped_by_you {
                ", with the runners stopped by you"
            } else {
                ""
            }
        ));
        drain.ending = Some(stopped_by_you);
        self.stage = Stage::Ending;
        self.begin_exit_wait();
    }

    /// once the agent has ended and its answer is in, waits up to 60 s for its process to
    /// exit.
    fn begin_exit_wait(&mut self) {
        let Some(drain) = self.drain.as_mut() else {
            return;
        };
        if drain.ending.is_none() || !drain.answered || drain.waiting {
            return;
        }
        drain.waiting = true;
        if drain.gone {
            self.agent_gone(true);
            return;
        }
        let Some(process) = drain.process.clone() else {
            self.log
                .warn("The agent's process was not watched, so the app cannot tell it exited.");
            self.agent_gone(false);
            return;
        };
        let report = self.report.clone();
        tokio::spawn(async move {
            let gone = process.wait(EXIT_WITHIN).await;
            let _ = report.send(Internal::AgentGone(gone));
        });
    }

    /// the agent's process exited, or did not within 60 s: install, start this app's
    /// own agent, or handle the agent's end as W1 does.
    pub(super) fn agent_gone(&mut self, gone: bool) {
        let Some(drain) = self.drain.take() else {
            return;
        };
        let stopped = drain.ending.unwrap_or(false);
        self.stage = Stage::Attempting;
        if !gone {
            self.log.warn(&format!(
                "The agent's process did not exit within {} s, so nothing installs.",
                EXIT_WITHIN.as_secs()
            ));
            if drain.purpose == Purpose::Install {
                self.drop_update();
            }
            self.restart_after_drop();
            return;
        }
        match drain.purpose {
            Purpose::Install => self.install(&drain.version, stopped),
            Purpose::ReplaceOlder => {
                self.log.info(
                    "The older agent has exited; this app starts its own, as the older one left the runners.",
                );
                self.next_launch = Some(if stopped {
                    Launch::Stopped
                } else {
                    Launch::Running
                });
                self.restart_after_drop();
            }
        }
    }

    fn install(&mut self, version: &str, stopped: bool) {
        if stopped && let Err(why) = self.update_record.write(version, self.time.now_ms()) {
            self.log.warn(&format!(
                "The record that the runners were stopped by you was not written: {why}"
            ));
        }
        let Some(verified) = self.verified.take() else {
            self.log
                .warn("No verified update is held, so nothing installs.");
            self.drop_update();
            self.restart_after_drop();
            return;
        };
        self.log.info(&format!(
            "Starting the installer of the update to {version}."
        ));
        match verified.install() {
            Ok(()) => self.ended = Some(Ended::Installing(version.to_string())),
            Err(why) => {
                self.log
                    .warn(&format!("The update's installer did not start: {why}"));
                self.drop_update();
                self.restart_after_drop();
            }
        }
    }

    /// drops the update and its record; the feed is read again in an hour.
    fn drop_update(&mut self) {
        self.verified = None;
        self.update_record.remove();
        self.next_check = Some(Instant::now() + CHECK_EVERY);
    }

    /// the connection dropped during this app's drain: when the agent's process has
    /// exited it ended unexpectedly, and the app goes on as if it had said so; when it
    /// still runs, the app reaches it again without starting another and asks again.
    /// `false` when no drain was answered on it, and the drop is W1's to handle.
    pub(super) fn drain_closed(&mut self) -> bool {
        let Some(drain) = self.drain.as_mut() else {
            return false;
        };
        // a drain whose agent's process is known was taken, even if its answer has not
        // been read yet
        let taken = drain.answered || drain.process.is_some() || drain.gone;
        if !taken {
            self.drain = None;
            return false;
        }
        drain.answered = true;
        if self.agent_exited() {
            self.ended_unannounced();
            return true;
        }
        if let Some(drain) = self.drain.as_mut() {
            drain.lost = true;
        }
        self.reconnect_after_drop(false);
        true
    }

    fn agent_exited(&self) -> bool {
        self.drain.as_ref().is_some_and(|drain| {
            drain.gone
                || drain
                    .process
                    .as_ref()
                    .is_some_and(|process| process.has_exited())
        })
    }

    /// the agent's process exited during the drain without `ending`: the runners were
    /// stopped by you if that is the last state the app heard.
    fn ended_unannounced(&mut self) {
        self.log
            .warn("The agent's process ended during the drain without saying so.");
        let stopped = self.snapshot.as_ref().is_some_and(stopped_by_you);
        if let Some(drain) = self.drain.as_mut() {
            drain.ending = Some(stopped);
            drain.gone = true;
            drain.waiting = true;
        }
        self.stage = Stage::Ending;
        self.agent_gone(true);
    }

    /// no agent answered while the app reached it again after a drop during its drain:
    /// if the agent's process has exited, it ended unexpectedly.
    pub(super) fn unreached_during_drain(&mut self) {
        let lost = self.drain.as_ref().is_some_and(|drain| drain.lost);
        if !lost {
            return;
        }
        let watched = self
            .drain
            .as_ref()
            .is_some_and(|drain| drain.process.is_some() || drain.gone);
        if self.agent_exited() {
            self.ended_unannounced();
        } else if watched {
            self.drain = None;
        } else {
            // with no handle the app cannot tell the agent ended, so it starts it once
            // more, as W1 does after a drop
            self.drain = None;
            self.restart_after_drop();
        }
    }
}
