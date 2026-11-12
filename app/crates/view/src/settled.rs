//! the count of runners up that the icon, tooltip, menu and announcement show while
//! the agent runs: a rise counts at once, a fall once it has lasted 30 s, so a runner
//! registering again between jobs does not flip them.

/// how long a fall must last before it counts.
pub const FALL_DELAY_MS: u64 = 30_000;

/// the count as it stood over the last [`FALL_DELAY_MS`]; the caller passes the time,
/// so a test controls the clock.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Settled {
    /// when each change of the count was seen and what it changed to, oldest first.
    changes: Vec<(u64, u32)>,
}

impl Settled {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// notes that `up` runners are up at `now_ms` and returns the count to show. While
    /// the agent does not run, the count is shown as it is and the history forgotten,
    /// so a stop shows at once.
    pub fn observe(&mut self, agent_runs: bool, up: u32, now_ms: u64) -> u32 {
        if !agent_runs {
            self.changes.clear();
            return up;
        }
        if self.changes.last().map(|(_, count)| *count) != Some(up) {
            self.changes.push((now_ms, up));
        }
        // the count at the start of the window is the last change at or before it
        let window_start = now_ms.saturating_sub(FALL_DELAY_MS);
        let first = self
            .changes
            .iter()
            .rposition(|(at, _)| *at <= window_start)
            .unwrap_or(0);
        self.changes.drain(..first);
        self.changes
            .iter()
            .map(|(_, count)| *count)
            .max()
            .unwrap_or(up)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECOND: u64 = 1_000;

    #[test]
    fn the_first_count_is_shown_as_it_is() {
        assert_eq!(Settled::new().observe(true, 3, 0), 3);
    }

    #[test]
    fn a_rise_counts_at_once() {
        let mut settled = Settled::new();
        settled.observe(true, 3, 0);
        assert_eq!(settled.observe(true, 4, SECOND), 4);
    }

    #[test]
    fn a_fall_counts_once_it_has_lasted_30_s() {
        let mut settled = Settled::new();
        settled.observe(true, 4, 0);
        assert_eq!(settled.observe(true, 3, 10 * SECOND), 4);
        assert_eq!(settled.observe(true, 3, 39 * SECOND), 4);
        assert_eq!(settled.observe(true, 3, 40 * SECOND - 1), 4);
        assert_eq!(settled.observe(true, 3, 40 * SECOND), 3);
    }

    #[test]
    fn a_runner_that_registers_again_within_30_s_never_shows_a_fall() {
        let mut settled = Settled::new();
        settled.observe(true, 4, 0);
        assert_eq!(settled.observe(true, 3, 10 * SECOND), 4);
        assert_eq!(settled.observe(true, 4, 20 * SECOND), 4);
        assert_eq!(settled.observe(true, 4, 60 * SECOND), 4);
    }

    #[test]
    fn a_deeper_fall_shows_the_highest_count_the_window_held() {
        let mut settled = Settled::new();
        settled.observe(true, 4, 0);
        settled.observe(true, 3, 10 * SECOND);
        // 4 left the window at 40 s; 3 held until 20 s and 2 since
        settled.observe(true, 2, 20 * SECOND);
        assert_eq!(settled.observe(true, 2, 39 * SECOND), 4);
        assert_eq!(settled.observe(true, 2, 40 * SECOND), 3);
        assert_eq!(settled.observe(true, 2, 50 * SECOND), 2);
    }

    #[test]
    fn a_stop_shows_at_once_and_forgets_the_history() {
        let mut settled = Settled::new();
        settled.observe(true, 4, 0);
        assert_eq!(settled.observe(false, 0, 5 * SECOND), 0);
        assert_eq!(settled.observe(true, 1, 6 * SECOND), 1);
    }
}
