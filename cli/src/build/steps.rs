//! the `docker` steps of `build`, and what a stop does at each of them; the configured
//! name changes only at the commit, the final retag, so a stop before it keeps the
//! previous image; see `docs/operations/cli.md`.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::ffi::OsString;
use std::future::Future;
use std::pin::pin;
use std::process::Stdio;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::sync::watch;
use tokio::time::Instant;

use crate::image::check::{ImageLabels, checked_reference};
use crate::runtime::docker::client_command;
use crate::runtime::docker_args::{image_inspect_arguments, image_list_arguments, info_arguments};
use crate::runtime::log::Log;
use crate::runtime::scrub::scrub_error_text;

use super::arguments::{
    build_arguments, image_remove_arguments, is_temporary_tag, leftover_list_arguments,
    prune_arguments, tag_arguments, temporary_tag,
};

/// the time bounds of `build`, none for the build itself, which takes minutes and
/// ends on a stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuildLimits {
    /// Docker must answer `docker info` within this, or nothing is built.
    pub docker_answer: Duration,
    /// a bound on every other client, a stop aside.
    pub command_budget: Duration,
    /// how long a retag under way may still finish.
    pub tag_grace_after_stop: Duration,
    /// how long the inspect that decides an interrupted retag may take.
    pub commit_inspect: Duration,
    /// the temporary tag is removed within this.
    pub removal_by_after_stop: Duration,
}

impl Default for BuildLimits {
    fn default() -> Self {
        Self {
            docker_answer: Duration::from_secs(30),
            command_budget: Duration::from_secs(120),
            tag_grace_after_stop: Duration::from_secs(5),
            commit_inspect: Duration::from_secs(4),
            removal_by_after_stop: Duration::from_secs(26),
        }
    }
}

/// writes each line `reader` yields to the log as a build line, and keeps the last
/// few, with the log's masks applied, for the error of a failed build; a line longer
/// than [`BUILD_LINE_BYTES`] is cut before any masked value the cut would split,
/// marked with `...`, and the rest of it dropped, so it is logged once and no part of
/// a masked value is kept.
async fn forward_lines<R: AsyncRead + Unpin>(
    reader: R,
    log: &dyn Log,
    tail: &RefCell<VecDeque<String>>,
) {
    let mut reader = BufReader::new(reader);
    let mut bytes = Vec::new();
    loop {
        bytes.clear();
        let mut limited = (&mut reader).take(BUILD_LINE_BYTES);
        match limited.read_until(b'\n', &mut bytes).await {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        let reached_limit = bytes.last() != Some(&b'\n') && bytes.len() as u64 == BUILD_LINE_BYTES;
        let dropped = reached_limit && drop_rest_of_line(&mut reader).await.unwrap_or(false);
        if dropped {
            bytes.truncate(bytes.len() - log.held_back(&bytes));
        }
        let text = String::from_utf8_lossy(&bytes);
        let mut line = text.trim_end_matches(['\r', '\n']).to_string();
        if dropped {
            line.push_str("...");
        } else if line.trim().is_empty() {
            continue;
        }
        log.info(&format!("docker build: {line}"));
        let mut tail = tail.borrow_mut();
        if tail.len() == BUILD_TAIL_LINES {
            tail.pop_front();
        }
        tail.push_back(cut_line(&log.masks().apply(line.trim())));
    }
}

/// reads and discards the rest of the current line, up to and including its newline or
/// the end of the output, in the reader's own buffer; true when any of it came before
/// the newline.
async fn drop_rest_of_line<R: AsyncBufRead + Unpin>(reader: &mut R) -> std::io::Result<bool> {
    let mut dropped = false;
    loop {
        let buffered = reader.fill_buf().await?;
        if buffered.is_empty() {
            return Ok(dropped);
        }
        if let Some(newline) = buffered.iter().position(|byte| *byte == b'\n') {
            reader.consume(newline + 1);
            return Ok(dropped || newline > 0);
        }
        dropped = true;
        let all = buffered.len();
        reader.consume(all);
    }
}

/// cuts a line kept for the error to [`BUILD_TAIL_LINE_CHARS`] characters, after its
/// masks were applied, so no part of a masked value survives the cut.
fn cut_line(line: &str) -> String {
    let mut chars = line.chars();
    let mut kept: String = chars.by_ref().take(BUILD_TAIL_LINE_CHARS).collect();
    if chars.next().is_some() {
        kept.push_str("...");
    }
    kept
}

/// where the output of `docker build` goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildOutput {
    /// the program's own standard output and error, which is what `build` shows.
    Inherit,
    /// each line goes through the log, so its masks apply, and the last few lines go
    /// into the error of a failed build.
    Log,
}

/// the longest line of a build's output the log keeps; a longer line is cut here, or
/// before a masked value the cut would split, and marked with `...`, and the rest of
/// it is dropped.
const BUILD_LINE_BYTES: u64 = 8 * 1024;

/// the last lines of a failed build's output the error carries.
const BUILD_TAIL_LINES: usize = 5;

/// the longest each of those lines is kept, so the five of them and the message
/// before them fit the error's 500 characters, and the newest line is not the one cut.
const BUILD_TAIL_LINE_CHARS: usize = 70;

struct Output {
    succeeded: bool,
    stdout: String,
    stderr: String,
}

impl Output {
    fn text(&self) -> String {
        scrub_error_text(&format!("{} {}", self.stdout, self.stderr), "")
    }
}

enum Stoppable<T> {
    Done(T),
    Stopped,
}

enum Retag {
    Committed,
    NotCommitted(String),
    /// the labels could not be read after the retag was cut short.
    Unknown(String),
}

/// one build of `image` from `context`.
pub struct Steps<'a> {
    pub program: OsString,
    /// variables added to every client's environment.
    pub environment: Vec<(OsString, OsString)>,
    pub image: &'a str,
    /// the archive streamed to `docker build -`.
    pub context: Vec<u8>,
    pub log: &'a dyn Log,
    pub limits: BuildLimits,
    pub output: BuildOutput,
    /// when a stop was asked for; it is set once.
    pub stop: watch::Receiver<Option<Instant>>,
}

pub(super) fn unix_seconds(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

impl Steps<'_> {
    /// runs the build until the configured name holds the new image; the error says
    /// why not, and what the name holds.
    pub async fn run(&self) -> Result<(), String> {
        let info = info_arguments();
        let answered = match self
            .stoppable(tokio::time::timeout(
                self.limits.docker_answer,
                self.output_of(&info),
            ))
            .await
        {
            Stoppable::Stopped => return self.abort_before_commit(None).await,
            Stoppable::Done(answered) => answered,
        };
        let failed = match answered {
            Ok(Ok(output)) if output.succeeded => None,
            Ok(Ok(output)) => Some(output.text()),
            Ok(Err(reason)) => Some(reason),
            Err(_) => {
                return Err(format!(
                    "Docker did not answer within {} s, so nothing was built",
                    self.limits.docker_answer.as_secs()
                ));
            }
        };
        if let Some(reason) = failed {
            return Err(if self.output == BuildOutput::Log {
                format!("Docker could not report its state ({reason}), so nothing was built")
            } else {
                format!("`docker info` failed ({reason}), so nothing was built")
            });
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
                return Err(format!(
                    "the build failed ({reason}), so {} keeps its previous image",
                    self.image
                ));
            }
        }
        if self.stop.borrow().is_some() {
            return self.abort_before_commit(Some(&temporary)).await;
        }
        match self.retag(&temporary, built).await {
            Retag::Committed => {
                self.after_commit(&temporary).await;
                Ok(())
            }
            Retag::NotCommitted(reason) => {
                self.remove_temporary(&temporary).await;
                Err(format!(
                    "{reason}, so {} keeps its previous image",
                    self.image
                ))
            }
            Retag::Unknown(cut_short) => {
                self.remove_temporary(&temporary).await;
                Err(if self.output == BuildOutput::Log {
                    format!(
                        "commit unknown: {cut_short}, and its labels could not be read; \
                         the image {} may be the previous one or the new one",
                        self.image
                    )
                } else {
                    format!(
                        "commit unknown: {cut_short}, and its labels could not be read; \
                         `docker image inspect {}` tells which image it is",
                        self.image
                    )
                })
            }
        }
    }

    /// completes when a stop is asked for, with its time.
    async fn stopped(&self) -> Instant {
        super::stop_requested(&self.stop).await
    }

    /// runs `work` until it ends or a stop is asked for, which drops it, killing the
    /// client it runs.
    async fn stoppable<T>(&self, work: impl Future<Output = T>) -> Stoppable<T> {
        tokio::select! {
            biased;
            _ = self.stopped() => Stoppable::Stopped,
            done = work => Stoppable::Done(done),
        }
    }

    /// runs `work` to its end, but once a stop is asked for only until `after_stop`
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

    /// runs a client to its end within `budget` and returns what it printed.
    async fn capture(&self, arguments: &[String], budget: Duration) -> Result<Output, String> {
        tokio::time::timeout(budget, self.output_of(arguments))
            .await
            .map_err(|_| {
                format!(
                    "the docker client did not finish within {} s",
                    budget.as_secs()
                )
            })?
    }

    /// runs a client to its end and returns what it printed.
    async fn output_of(&self, arguments: &[String]) -> Result<Output, String> {
        let mut command = client_command(&self.program, &self.environment, arguments);
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
        let child = command
            .spawn()
            .map_err(|error| format!("the docker client could not start: {error}"))?;
        let output = child
            .wait_with_output()
            .await
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

    /// removes the temporary tags of this repository that a killed `build` left; a
    /// failure is a warning.
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

    /// runs the one `docker build`, streaming the context to its standard input and
    /// its output to the terminal or the log.
    async fn build_image(&self, temporary: &str, built: u64) -> Result<(), String> {
        let mut command = client_command(
            &self.program,
            &self.environment,
            &build_arguments(temporary, built),
        );
        let logged = self.output == BuildOutput::Log;
        command.stdin(Stdio::piped());
        if logged {
            command.stdout(Stdio::piped()).stderr(Stdio::piped());
        } else {
            command.stdout(Stdio::inherit()).stderr(Stdio::inherit());
        }
        let mut child = command
            .spawn()
            .map_err(|error| format!("the docker client could not start: {error}"))?;
        let stdin = child.stdin.take();
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let context = &self.context;
        // a client that ends early closes the pipe; its exit code says why
        let feed = async move {
            if let Some(mut stdin) = stdin {
                let _ = stdin.write_all(context).await;
                let _ = stdin.shutdown().await;
            }
        };
        let tail = RefCell::new(VecDeque::new());
        let forward_out = async {
            if let Some(stdout) = stdout {
                forward_lines(stdout, self.log, &tail).await;
            }
        };
        let forward_err = async {
            if let Some(stderr) = stderr {
                forward_lines(stderr, self.log, &tail).await;
            }
        };
        let (_, _, _, status) = tokio::join!(feed, forward_out, forward_err, child.wait());
        let status = status.map_err(|error| format!("the docker client failed: {error}"))?;
        if status.success() {
            Ok(())
        } else {
            let mut message = format!(
                "{} exited with code {}",
                if logged {
                    "the image build"
                } else {
                    "docker build"
                },
                status.code().unwrap_or(-1)
            );
            let tail = tail.into_inner();
            if !tail.is_empty() {
                let lines: Vec<String> = tail.into_iter().collect();
                message.push_str(&format!("; its last output: {}", lines.join(" | ")));
            }
            Err(message)
        }
    }

    /// removes the temporary tag by the stop's removal deadline once a stop is asked
    /// for; a failure is a warning naming the tag.
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
            .within_stop_budget(removal, self.limits.removal_by_after_stop)
            .await
        {
            self.log.warn(&format!(
                "The temporary tag {temporary} could not be removed ({reason}); the next build removes it."
            ));
        }
    }

    /// a stop before the commit: the build client is already gone.
    async fn abort_before_commit(&self, temporary: Option<&str>) -> Result<(), String> {
        if let Some(temporary) = temporary {
            self.remove_temporary(temporary).await;
        }
        Err(format!(
            "the build was stopped before it finished, so {} keeps its previous image",
            self.image
        ))
    }

    /// tags the build as the configured image, which commits it; a stop during the tag
    /// gives the client [`BuildLimits::tag_grace_after_stop`] to succeed, and when it
    /// does not, or the tag stalls for the command budget, the configured name's
    /// build-time label says whether it committed.
    async fn retag(&self, temporary: &str, built: u64) -> Retag {
        let mut command = client_command(
            &self.program,
            &self.environment,
            &tag_arguments(temporary, self.image),
        );
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
        let child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                return Retag::NotCommitted(format!("the docker client could not start: {error}"));
            }
        };
        let mut waiting = Box::pin(child.wait_with_output());
        let cut_short = tokio::select! {
            output = &mut waiting => {
                return match output {
                    Ok(output) if output.status.success() => Retag::Committed,
                    Ok(output) => Retag::NotCommitted(format!(
                        "tagging the build as {} failed: {}",
                        self.image,
                        scrub_error_text(&String::from_utf8_lossy(&output.stderr), "")
                    )),
                    Err(error) => Retag::NotCommitted(format!("the docker client failed: {error}")),
                };
            }
            at = self.stopped() => {
                let graced = tokio::time::timeout_at(at + self.limits.tag_grace_after_stop, &mut waiting).await;
                if let Ok(Ok(output)) = graced
                    && output.status.success()
                {
                    return Retag::Committed;
                }
                format!("the build was stopped while {} was being tagged", self.image)
            }
            () = tokio::time::sleep(self.limits.command_budget) => format!(
                "tagging the build as {} stalled for {} s",
                self.image,
                self.limits.command_budget.as_secs()
            ),
        };
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
            None => Retag::Unknown(cut_short),
            Some(labels) if labels.built == Some(built.to_string()) => Retag::Committed,
            Some(_) => Retag::NotCommitted(format!("{cut_short}, and the tag did not take effect")),
        }
    }

    /// after the commit: removes the temporary tag, then prunes the dangling images
    /// unless a stop was asked for.
    async fn after_commit(&self, temporary: &str) {
        self.remove_temporary(temporary).await;
        if self.stop.borrow().is_some() {
            self.log.info(&format!(
                "{} is the new image; the stop skips the prune, which the next build runs.",
                self.image
            ));
            return;
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
    }
}

#[cfg(test)]
mod cut_tests {
    use super::*;

    #[test]
    fn a_line_is_cut_by_characters_and_a_short_one_is_kept() {
        let exact = "é".repeat(BUILD_TAIL_LINE_CHARS);
        assert_eq!(cut_line(&exact), exact);
        let longer = "é".repeat(BUILD_TAIL_LINE_CHARS + 1);
        assert_eq!(cut_line(&longer), format!("{exact}..."));
    }
}

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};

    use super::*;
    use crate::image::contract::BUILT_LABEL;
    use crate::runtime::fake::MemoryLog;
    use crate::runtime::secret::Secret;

    const IMAGE: &str = "runner:local";

    fn limits(command_budget: Duration) -> BuildLimits {
        BuildLimits {
            command_budget,
            ..BuildLimits::default()
        }
    }

    /// a fake `docker` in a temporary directory that logs each call's first two words
    /// to `calls`; its `tag` runs the shell command `tag`, and its inspect reports
    /// `inspected` or the build's own time.
    fn docker(name: &str, tag: &str, inspected: Option<&str>) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("marshal-build-steps-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let at = dir.display();
        let built = inspected.map_or_else(|| format!("$(cat '{at}/built')"), str::to_string);
        let script = format!(
            "#!/bin/sh\n\
             printf '%s %s\\n' \"$1\" \"$2\" >> '{at}/calls'\n\
             case \"$1\" in\n\
             build) cat > /dev/null\n\
               for argument; do case \"$argument\" in {BUILT_LABEL}=*) \
             printf '%s' \"${{argument#*=}}\" > '{at}/built' ;; esac; done ;;\n\
             tag) {tag} ;;\n\
             image) if [ \"$2\" = inspect ]; then printf '[\"\",\"%s\",\"\",\"\"]\\n' \"{built}\"; fi ;;\n\
             esac\n"
        );
        let path = dir.join("docker");
        std::fs::write(&path, script).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        // a child another test forks while the script is still open for writing
        // keeps it busy until that child execs, so the first run is retried
        let started = std::time::Instant::now();
        while std::process::Command::new(&path)
            .arg("ready")
            .status()
            .is_err()
        {
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "the script never ran"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        std::fs::remove_file(dir.join("calls")).unwrap();
        dir
    }

    fn calls(dir: &Path) -> Vec<String> {
        std::fs::read_to_string(dir.join("calls"))
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }

    async fn run(
        dir: &Path,
        log: &MemoryLog,
        limits: BuildLimits,
        stop: watch::Receiver<Option<Instant>>,
    ) -> Result<(), String> {
        let steps = Steps {
            program: dir.join("docker").into_os_string(),
            environment: Vec::new(),
            image: IMAGE,
            context: Vec::new(),
            log,
            limits,
            output: BuildOutput::Inherit,
            stop,
        };
        steps.run().await
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_retag_that_stalls_with_no_stop_and_tagged_commits_and_still_prunes() {
        let dir = docker("stall-tagged", "exec sleep 30", None);
        let log = MemoryLog::default();
        let (_stop, receiver) = watch::channel(None);
        run(&dir, &log, limits(Duration::from_secs(1)), receiver)
            .await
            .unwrap();
        let calls = calls(&dir);
        assert!(calls.contains(&"image inspect".to_string()), "{calls:?}");
        assert_eq!(calls.last().map(String::as_str), Some("image prune"));
        assert!(
            log.lines()
                .contains(&format!("INFO: {IMAGE} is the new image.")),
            "{:?}",
            log.lines()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_retag_that_stalls_with_no_stop_and_did_not_tag_is_reported_as_a_stall() {
        let dir = docker("stall-untagged", "exec sleep 30", Some("1"));
        let log = MemoryLog::default();
        let (_stop, receiver) = watch::channel(None);
        let error = run(&dir, &log, limits(Duration::from_secs(1)), receiver)
            .await
            .unwrap_err();
        assert_eq!(
            error,
            format!(
                "tagging the build as {IMAGE} stalled for 1 s, and the tag did not take \
                 effect, so {IMAGE} keeps its previous image"
            )
        );
        assert!(!calls(&dir).contains(&"image prune".to_string()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_retag_that_succeeds_within_the_grace_of_a_stop_commits_without_an_inspect() {
        // an inspect would find another build, so only the tag's own exit commits it
        let dir = docker("graced", "sleep 1", Some("1"));
        let log = MemoryLog::default();
        let (stop, receiver) = watch::channel(None);
        let stop_at_the_tag = async {
            let started = Instant::now();
            while !calls(&dir).iter().any(|call| call.starts_with("tag ")) {
                assert!(started.elapsed() < Duration::from_secs(10), "no tag call");
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            stop.send_replace(Some(Instant::now()));
        };
        let (outcome, ()) = tokio::join!(
            run(&dir, &log, limits(Duration::from_secs(60)), receiver),
            stop_at_the_tag
        );
        outcome.unwrap();
        let calls = calls(&dir);
        assert!(!calls.contains(&"image inspect".to_string()), "{calls:?}");
        assert!(!calls.contains(&"image prune".to_string()), "{calls:?}");
        assert!(
            log.lines().contains(&format!(
                "INFO: {IMAGE} is the new image; the stop skips the prune, which the next \
                 build runs."
            )),
            "{:?}",
            log.lines()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_build_line_longer_than_the_limit_is_cut_once_not_split() {
        let mut output = "x".repeat(20 * 1024).into_bytes();
        output.extend_from_slice(b"\nshort\n");
        let log = MemoryLog::default();
        let tail = RefCell::new(VecDeque::new());
        forward_lines(output.as_slice(), &log, &tail).await;
        let lines = log.lines();
        assert_eq!(lines.len(), 2, "{} records", lines.len());
        let cut = lines[0].strip_prefix("INFO: docker build: ").unwrap();
        assert!(cut.ends_with("..."), "{}", &cut[cut.len() - 8..]);
        assert!(cut.trim_end_matches("...").len() <= 8 * 1024);
        assert_eq!(lines[1], "INFO: docker build: short");
        let tail: Vec<String> = tail.into_inner().into();
        assert_eq!(
            tail,
            vec![format!("{}...", "x".repeat(70)), "short".to_string()]
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_masked_value_across_the_cut_of_a_build_line_leaves_no_part_of_it_in_the_log() {
        let secret = "jit-0123456789abcdef";
        let mut output = "x".repeat(8 * 1024 - 5).into_bytes();
        output.extend_from_slice(secret.as_bytes());
        output.extend_from_slice(b"\nshort\n");
        let log = MemoryLog::default();
        log.mask(&Secret::new(secret.to_string()));
        let tail = RefCell::new(VecDeque::new());
        forward_lines(output.as_slice(), &log, &tail).await;
        let lines = log.lines();
        assert_eq!(lines.len(), 2, "{} records", lines.len());
        assert!(
            lines[0].ends_with("..."),
            "{}",
            &lines[0][lines[0].len() - 8..]
        );
        assert!(
            !lines[0].contains("jit-"),
            "{}",
            &lines[0][lines[0].len() - 8..]
        );
        assert_eq!(lines[1], "INFO: docker build: short");
        let tail: Vec<String> = tail.into_inner().into();
        assert_eq!(
            tail,
            vec![format!("{}...", "x".repeat(70)), "short".to_string()]
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_masked_value_that_fills_the_kept_text_of_a_build_line_still_records_the_cut() {
        let secret = "s".repeat(8 * 1024 + 10);
        let mut output = secret.clone().into_bytes();
        output.extend_from_slice(b"\nnext\n");
        let log = MemoryLog::default();
        log.mask(&Secret::new(secret));
        let tail = RefCell::new(VecDeque::new());
        forward_lines(output.as_slice(), &log, &tail).await;
        assert_eq!(
            log.lines(),
            vec!["INFO: docker build: ...", "INFO: docker build: next"]
        );
        let tail: Vec<String> = tail.into_inner().into();
        assert_eq!(tail, vec!["...".to_string(), "next".to_string()]);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_build_line_cut_by_the_end_of_the_output_ends_the_forwarding() {
        let output = "y".repeat(20 * 1024).into_bytes();
        let log = MemoryLog::default();
        let tail = RefCell::new(VecDeque::new());
        forward_lines(output.as_slice(), &log, &tail).await;
        assert_eq!(log.lines().len(), 1, "{:?}", log.lines().len());
        assert_eq!(tail.borrow().len(), 1);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_build_line_of_exactly_the_limit_is_not_marked_as_cut() {
        let mut output = "z".repeat(8 * 1024).into_bytes();
        output.extend_from_slice(b"\nnext\n");
        let log = MemoryLog::default();
        let tail = RefCell::new(VecDeque::new());
        forward_lines(output.as_slice(), &log, &tail).await;
        let lines = log.lines();
        assert_eq!(lines.len(), 2, "{} records", lines.len());
        assert!(!lines[0].ends_with("..."));
        assert_eq!(lines[1], "INFO: docker build: next");
    }
}
