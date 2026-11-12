//! what a runner's own output says about its job: the runner writes
//! `Listening for Jobs` when it is ready and `Running job: <name>` when a job starts,
//! each after a UTC timestamp and a colon; only that fixed text is compared, and a
//! job's name is never kept.

/// the two lines a runner container's output has carried; a mark never clears, so the
/// order of lines across its two streams does not matter.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct JobMarks {
    pub listening: bool,
    pub job_started: bool,
}

/// what the marks say about the runner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Activity {
    /// it is ready and has started no job.
    Idle,
    /// it has started a job.
    Busy,
    /// its output has carried neither line yet.
    Unknown,
}

const LISTENING: &str = "Listening for Jobs";
const JOB_STARTED: &str = "Running job:";

/// the text after a line's timestamp, `yyyy-MM-dd HH:mm:ssZ: `, or `None` when the
/// line does not start with one.
fn after_timestamp(line: &str) -> Option<&str> {
    let (stamp, rest) = line.split_once(": ")?;
    let starts_with_digit = stamp.chars().next().is_some_and(|c| c.is_ascii_digit());
    (starts_with_digit && stamp.ends_with('Z')).then_some(rest)
}

impl JobMarks {
    /// the marks once `line` has been seen; every other line leaves them as they are.
    #[must_use]
    pub fn observe(self, line: &str) -> Self {
        let Some(text) = after_timestamp(line.trim_end_matches(['\n', '\r'])) else {
            return self;
        };
        Self {
            listening: self.listening || text.starts_with(LISTENING),
            job_started: self.job_started || text.starts_with(JOB_STARTED),
        }
    }

    #[must_use]
    pub fn activity(self) -> Activity {
        match (self.listening, self.job_started) {
            (_, true) => Activity::Busy,
            (true, false) => Activity::Idle,
            (false, false) => Activity::Unknown,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn after(lines: &[&str]) -> Activity {
        lines
            .iter()
            .fold(JobMarks::default(), |marks, line| marks.observe(line))
            .activity()
    }

    #[test]
    fn a_runner_is_idle_once_ready_and_busy_once_a_job_started() {
        assert_eq!(after(&[]), Activity::Unknown);
        assert_eq!(
            after(&["2026-11-12 08:00:01Z: Listening for Jobs"]),
            Activity::Idle
        );
        assert_eq!(
            after(&[
                "2026-11-12 08:00:01Z: Listening for Jobs",
                "2026-11-12 08:03:00Z: Running job: build (ubuntu)",
            ]),
            Activity::Busy
        );
    }

    #[test]
    fn the_order_across_streams_and_a_reconnect_change_nothing() {
        assert_eq!(
            after(&[
                "2026-11-12 08:03:00Z: Running job: build",
                "2026-11-12 08:00:01Z: Listening for Jobs\r\n",
            ]),
            Activity::Busy
        );
        assert_eq!(
            after(&[
                "2026-11-12 08:00:01Z: Listening for Jobs",
                "2026-11-12 08:10:00Z: Runner reconnected.",
            ]),
            Activity::Idle
        );
    }

    #[test]
    fn a_line_without_the_timestamp_or_with_the_text_elsewhere_is_ignored() {
        assert_eq!(after(&["Listening for Jobs"]), Activity::Unknown);
        assert_eq!(after(&["Running job: build"]), Activity::Unknown);
        assert_eq!(
            after(&["2026-11-12 08:00:01Z: echo Running job: fake"]),
            Activity::Unknown
        );
        assert_eq!(
            after(&["step output: Listening for Jobs"]),
            Activity::Unknown
        );
    }
}
