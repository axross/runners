//! The `docker` steps of `build`, and what a stop does at each of them.
//!
//! The build runs under a temporary tag and becomes the configured image only when
//! it is tagged with that name, the commit point. A stop before the commit kills
//! the build client at once and removes the temporary tag, so the configured name
//! keeps its previous image. A stop during the retag gives the tag client a short
//! grace and then reads the configured name's build-time label to tell whether the
//! build committed. A stop after the commit finishes the tag removal and skips the
//! prune.

use std::ffi::OsString;
use std::future::Future;
use std::pin::pin;
use std::process::Stdio;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tokio::io::AsyncWriteExt;
use tokio::sync::watch;
use tokio::time::Instant;

use crate::command::{EXIT_FAILED, EXIT_OK};
use crate::image::check::{ImageLabels, checked_reference};
use crate::runtime::docker::client_command;
use crate::runtime::docker_args::{image_inspect_arguments, image_list_arguments, info_arguments};
use crate::runtime::log::Log;
use crate::runtime::scrub::scrub_error_text;

use super::arguments::{
    build_arguments, image_remove_arguments, is_temporary_tag, leftover_list_arguments,
    prune_arguments, tag_arguments, temporary_tag,
};

/// The time bounds of `build`. The build itself has none: a full build takes
/// minutes, and a stop ends it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuildLimits {
    /// Docker must answer `docker info` within this, or nothing is built.
    pub docker_answer: Duration,
    /// A bound on every other client, a stop aside.
    pub command_budget: Duration,
    /// Counted from a stop: how long a retag under way may still finish.
    pub tag_grace: Duration,
    /// How long the inspect that decides an interrupted retag may take.
    pub commit_inspect: Duration,
    /// Counted from a stop: the temporary tag is removed by then.
    pub removal_by: Duration,
}

impl Default for BuildLimits {
    fn default() -> Self {
        Self {
            docker_answer: Duration::from_secs(30),
            command_budget: Duration::from_secs(120),
            tag_grace: Duration::from_secs(5),
            commit_inspect: Duration::from_secs(4),
            removal_by: Duration::from_secs(26),
        }
    }
}

/// What a finished client left.
struct Output {
    succeeded: bool,
    stdout: String,
    stderr: String,
}

impl Output {
    /// Its output for a message, on one line and cut as an error text is.
    fn text(&self) -> String {
        scrub_error_text(&format!("{} {}", self.stdout, self.stderr), "")
    }
}

/// How a step ended: by itself, or by a stop.
enum Stoppable<T> {
    Done(T),
    Stopped,
}

/// Where the retag left the configured name.
enum Retag {
    Committed { stopped: bool },
    NotCommitted(String),
    Unknown,
}

/// One build of `image` from `context`.
pub struct Steps<'a> {
    pub program: OsString,
    pub image: &'a str,
    /// The archive streamed to `docker build -`.
    pub context: Vec<u8>,
    pub log: &'a dyn Log,
    pub limits: BuildLimits,
    /// When a stop was asked for; it is set once.
    pub stop: watch::Receiver<Option<Instant>>,
}

fn unix_seconds(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

impl Steps<'_> {
    /// Runs the build and returns the exit code: 0 once the configured name holds
    /// the new image, 1 otherwise.
    pub async fn run(&self) -> u8 {
        let info = info_arguments();
        let answered = match self
            .stoppable(self.capture(&info, self.limits.docker_answer))
            .await
        {
            Stoppable::Stopped => return self.abort_before_commit(None).await,
            Stoppable::Done(answered) => answered,
        };
        if !answered.is_ok_and(|output| output.succeeded) {
            return self.fail(&format!(
                "Docker did not answer within {} s, so nothing was built",
                self.limits.docker_answer.as_secs()
            ));
        }
        if let Stoppable::Stopped = self.stoppable(self.remove_leftovers()).await {
            return self.abort_before_commit(None).await;
        }
        let now = SystemTime::now();
        let temporary = temporary_tag(self.image, now);
        let built = unix_seconds(now);
        self.log
            .info(&format!("Building under the temporary tag {temporary}."));
        match self.stoppable(self.build_image(&temporary, built)).await {
            Stoppable::Stopped => return self.abort_before_commit(Some(&temporary)).await,
            Stoppable::Done(Ok(())) => {}
            Stoppable::Done(Err(reason)) => {
                self.remove_temporary(&temporary).await;
                return self.fail(&format!(
                    "the build failed ({reason}), so {} keeps its previous image",
                    self.image
                ));
            }
        }
        if self.stop.borrow().is_some() {
            return self.abort_before_commit(Some(&temporary)).await;
        }
        match self.retag(&temporary, built).await {
            Retag::Committed { stopped } => self.after_commit(&temporary, stopped).await,
            Retag::NotCommitted(reason) => {
                self.remove_temporary(&temporary).await;
                self.fail(&format!(
                    "{reason}, so {} keeps its previous image",
                    self.image
                ))
            }
            Retag::Unknown => {
                self.remove_temporary(&temporary).await;
                self.fail(&format!(
                    "commit unknown: the build was stopped while {} was being tagged, and its \
                     labels could not be read; `docker image inspect {}` tells which image it is",
                    self.image, self.image
                ))
            }
        }
    }

    fn fail(&self, message: &str) -> u8 {
        self.log.info(&format!("error: {message}"));
        EXIT_FAILED
    }

    /// Completes when a stop is asked for, with its time; never, when the sender is
    /// gone without one.
    async fn stopped(&self) -> Instant {
        let mut stop = self.stop.clone();
        loop {
            if let Some(at) = *stop.borrow_and_update() {
                return at;
            }
            if stop.changed().await.is_err() {
                return std::future::pending().await;
            }
        }
    }

    /// Runs `work` until it ends or a stop is asked for, which drops it, killing the
    /// client it runs.
    async fn stoppable<T>(&self, work: impl Future<Output = T>) -> Stoppable<T> {
        tokio::select! {
            biased;
            _ = self.stopped() => Stoppable::Stopped,
            done = work => Stoppable::Done(done),
        }
    }

    /// Runs `work` to its end, but once a stop is asked for only until `after_stop`
    /// from it.
    async fn within_stop_budget<T>(
        &self,
        work: impl Future<Output = Result<T, String>>,
        after_stop: Duration,
    ) -> Result<T, String> {
        let mut work = pin!(work);
        tokio::select! {
            biased;
            done = &mut work => done,
            at = self.stopped() => tokio::time::timeout_at(at + after_stop, &mut work)
                .await
                .unwrap_or_else(|_| Err("the stop's time ran out".to_string())),
        }
    }

    /// Runs a client to its end within `budget` and returns what it printed.
    async fn capture(&self, arguments: &[String], budget: Duration) -> Result<Output, String> {
        let mut command = client_command(&self.program, &[], arguments);
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
        let child = command
            .spawn()
            .map_err(|error| format!("the docker client could not start: {error}"))?;
        let output = tokio::time::timeout(budget, child.wait_with_output())
            .await
            .map_err(|_| {
                format!(
                    "the docker client did not finish within {} s",
                    budget.as_secs()
                )
            })?
            .map_err(|error| format!("the docker client failed: {error}"))?;
        Ok(Output {
            succeeded: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }

    async fn checked(&self, arguments: &[String]) -> Result<Output, String> {
        let output = self.capture(arguments, self.limits.command_budget).await?;
        if output.succeeded {
            Ok(output)
        } else {
            Err(output.text())
        }
    }

    /// Removes the temporary tags an earlier `build` that was killed left: those of
    /// this repository that carry the image label. A failure is a warning.
    async fn remove_leftovers(&self) {
        let listed = match self.checked(&leftover_list_arguments(self.image)).await {
            Ok(listed) => listed,
            Err(reason) => {
                self.log.warn(&format!(
                    "Leftover temporary tags could not be listed ({reason}); the next build tries again."
                ));
                return;
            }
        };
        for reference in listed
            .stdout
            .lines()
            .map(str::trim)
            .filter(|reference| is_temporary_tag(self.image, reference))
        {
            self.log.info(&format!(
                "Removing the temporary tag {reference}, left by an earlier build."
            ));
            if let Err(reason) = self.checked(&image_remove_arguments(reference)).await {
                self.log.warn(&format!(
                    "The temporary tag {reference} could not be removed ({reason}); the next build tries again."
                ));
            }
        }
    }

    /// Runs the one `docker build`, streaming the context to its standard input and
    /// its output to the terminal.
    async fn build_image(&self, temporary: &str, built: u64) -> Result<(), String> {
        let mut command = client_command(&self.program, &[], &build_arguments(temporary, built));
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());
        let mut child = command
            .spawn()
            .map_err(|error| format!("the docker client could not start: {error}"))?;
        let stdin = child.stdin.take();
        let context = &self.context;
        // a client that ends early closes the pipe; its exit code says why
        let feed = async move {
            if let Some(mut stdin) = stdin {
                let _ = stdin.write_all(context).await;
                let _ = stdin.shutdown().await;
            }
        };
        let (_, status) = tokio::join!(feed, child.wait());
        let status = status.map_err(|error| format!("the docker client failed: {error}"))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!(
                "docker build exited with code {}",
                status.code().unwrap_or(-1)
            ))
        }
    }

    /// Removes the temporary tag, if the build made one, by the stop's removal
    /// deadline once a stop is asked for. A failure is a warning naming the tag.
    async fn remove_temporary(&self, temporary: &str) {
        let removal = async {
            let listed = self.checked(&image_list_arguments(temporary)).await?;
            if listed.stdout.trim().is_empty() {
                return Ok(());
            }
            self.checked(&image_remove_arguments(temporary))
                .await
                .map(|_| ())
        };
        if let Err(reason) = self
            .within_stop_budget(removal, self.limits.removal_by)
            .await
        {
            self.log.warn(&format!(
                "The temporary tag {temporary} could not be removed ({reason}); the next build removes it."
            ));
        }
    }

    /// A stop before the commit: the build client is already gone.
    async fn abort_before_commit(&self, temporary: Option<&str>) -> u8 {
        if let Some(temporary) = temporary {
            self.remove_temporary(temporary).await;
        }
        self.fail(&format!(
            "the build was stopped before it finished, so {} keeps its previous image",
            self.image
        ))
    }

    /// Tags the build as the configured image, which commits it. A stop during the
    /// tag gives the client [`BuildLimits::tag_grace`] from the stop, and the
    /// configured name's build-time label then says whether it committed.
    async fn retag(&self, temporary: &str, built: u64) -> Retag {
        let mut command = client_command(&self.program, &[], &tag_arguments(temporary, self.image));
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
        let child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                return Retag::NotCommitted(format!("the docker client could not start: {error}"));
            }
        };
        let mut waiting = Box::pin(child.wait_with_output());
        let interrupted = tokio::select! {
            output = &mut waiting => {
                return match output {
                    Ok(output) if output.status.success() => Retag::Committed { stopped: false },
                    Ok(output) => Retag::NotCommitted(format!(
                        "tagging the build as {} failed: {}",
                        self.image,
                        scrub_error_text(&String::from_utf8_lossy(&output.stderr), "")
                    )),
                    Err(error) => Retag::NotCommitted(format!("the docker client failed: {error}")),
                };
            }
            at = self.stopped() => at + self.limits.tag_grace,
            () = tokio::time::sleep(self.limits.command_budget) => Instant::now(),
        };
        let _ = tokio::time::timeout_at(interrupted, &mut waiting).await;
        // dropping the wait kills a client still running
        drop(waiting);
        let inspected = self
            .capture(
                &image_inspect_arguments(&checked_reference(self.image)),
                self.limits.commit_inspect,
            )
            .await;
        let labels = inspected
            .ok()
            .filter(|output| output.succeeded)
            .and_then(|output| ImageLabels::from_inspect(output.stdout.as_bytes()));
        match labels {
            None => Retag::Unknown,
            Some(labels) if labels.built == Some(built.to_string()) => {
                Retag::Committed { stopped: true }
            }
            Some(_) => Retag::NotCommitted(format!(
                "the build was stopped while {} was being tagged, before it was",
                self.image
            )),
        }
    }

    /// After the commit: the temporary tag goes, and the dangling images that
    /// carry the image label are pruned unless a stop was asked for.
    async fn after_commit(&self, temporary: &str, stopped: bool) -> u8 {
        self.remove_temporary(temporary).await;
        if stopped || self.stop.borrow().is_some() {
            self.log.info(&format!(
                "{} is the new image; the stop skips the prune, which the next build runs.",
                self.image
            ));
            return EXIT_OK;
        }
        match self.stoppable(self.checked(&prune_arguments())).await {
            Stoppable::Done(Ok(_)) => {}
            Stoppable::Done(Err(reason)) => self.log.warn(&format!(
                "The old runner images could not be pruned ({reason})."
            )),
            Stoppable::Stopped => self.log.info(
                "The stop ended the prune of the old runner images; the next build prunes them.",
            ),
        }
        self.log.info(&format!("{} is the new image.", self.image));
        EXIT_OK
    }
}
