//! the hourly request budget, read from the `x-ratelimit-*` headers of display
//! responses: within one reset window the lowest remaining count is kept, so the
//! level only moves down until the reset, and checks slow at 25% left and pause at
//! 10% or when GitHub says the limit is reached.

use std::time::Duration;

use super::Budget;

/// how often a repository is checked at most, at the normal level.
pub const NORMAL_INTERVAL: Duration = Duration::from_secs(15);

/// the rate limit's window; a reset is never taken as further away than this.
const HOUR_MS: u64 = 3_600_000;

/// how often a repository is checked at most, once checks are slowed.
pub const SLOWED_INTERVAL: Duration = Duration::from_secs(60);

/// the agent's level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Normal,
    Slowed,
    Paused,
}

impl Level {
    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Slowed => "slowed",
            Self::Paused => "paused",
        }
    }

    /// the least time between two checks of a repository; `None` while paused.
    #[must_use]
    pub fn interval(self) -> Option<Duration> {
        match self {
            Self::Normal => Some(NORMAL_INTERVAL),
            Self::Slowed => Some(SLOWED_INTERVAL),
            Self::Paused => None,
        }
    }
}

/// the rate-limit headers of one response.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RateHeaders {
    pub limit: Option<u64>,
    pub remaining: Option<u64>,
    /// seconds since the Unix epoch.
    pub reset: Option<u64>,
    pub used: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Window {
    limit: u64,
    remaining: u64,
    /// milliseconds since the epoch.
    resets_at: u64,
    seen_at: u64,
    /// GitHub answered that the limit is reached.
    reached: bool,
}

/// the budget as the agent tracks it; times are milliseconds since the epoch.
#[derive(Debug, Clone, Default)]
pub struct Tracker {
    window: Option<Window>,
}

impl Tracker {
    /// takes the figures of a display response that arrived at `now`; a response
    /// without all three figures is not counted.
    pub fn observe(&mut self, headers: &RateHeaders, now: u64) {
        let (Some(limit), Some(remaining), Some(reset)) =
            (headers.limit, headers.remaining, headers.reset)
        else {
            return;
        };
        // GitHub's window is an hour, so a later reset is held to an hour from now
        let resets_at = reset.saturating_mul(1000).min(now.saturating_add(HOUR_MS));
        match &mut self.window {
            Some(window) if resets_at > window.resets_at && now >= window.resets_at => {
                *window = Window {
                    limit,
                    remaining,
                    resets_at,
                    seen_at: now,
                    reached: false,
                };
            }
            Some(window) if resets_at < window.resets_at => window.seen_at = now,
            Some(window) => {
                window.remaining = window.remaining.min(remaining);
                window.seen_at = now;
            }
            None => {
                self.window = Some(Window {
                    limit,
                    remaining,
                    resets_at,
                    seen_at: now,
                    reached: false,
                });
            }
        }
    }

    /// records that GitHub answered that the limit is reached, after the response's
    /// own figures were observed.
    pub fn reached(&mut self) {
        if let Some(window) = &mut self.window {
            window.reached = true;
            window.remaining = 0;
        }
    }

    /// the level at `now`: normal again once the window's reset has passed.
    #[must_use]
    pub fn level(&self, now: u64) -> Level {
        let Some(window) = self.window else {
            return Level::Normal;
        };
        if now >= window.resets_at {
            return Level::Normal;
        }
        let remaining = u128::from(window.remaining);
        let limit = u128::from(window.limit);
        if window.reached || remaining * 10 <= limit {
            Level::Paused
        } else if remaining * 4 <= limit {
            Level::Slowed
        } else {
            Level::Normal
        }
    }

    /// when a pause ends, while paused.
    #[must_use]
    pub fn paused_until(&self, now: u64) -> Option<u64> {
        (self.level(now) == Level::Paused)
            .then_some(self.window.map(|window| window.resets_at))
            .flatten()
    }

    /// the budget as the `github` topic carries it.
    #[must_use]
    pub fn budget(&self, now: u64) -> Option<Budget> {
        self.window.map(|window| Budget {
            limit: window.limit,
            remaining: window.remaining,
            resets_at: window.resets_at,
            seen_at: window.seen_at,
            level: self.level(now).word().to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOUR: u64 = 3_600_000;

    fn headers(remaining: u64, reset_s: u64) -> RateHeaders {
        RateHeaders {
            limit: Some(5000),
            remaining: Some(remaining),
            reset: Some(reset_s),
            used: Some(5000 - remaining),
        }
    }

    #[test]
    fn a_reset_is_never_taken_as_more_than_an_hour_away() {
        let mut tracker = Tracker::default();
        tracker.observe(&headers(100, u64::MAX), 5_000);
        assert_eq!(tracker.paused_until(5_000), Some(5_000 + HOUR));
        assert_eq!(tracker.level(5_000 + HOUR), Level::Normal);
    }

    #[test]
    fn nothing_is_reported_before_a_response() {
        let tracker = Tracker::default();
        assert_eq!(tracker.budget(0), None);
        assert_eq!(tracker.level(0), Level::Normal);
    }

    #[test]
    fn the_lowest_remaining_count_of_a_window_is_kept() {
        let mut tracker = Tracker::default();
        tracker.observe(&headers(4000, 3600), 1_000);
        tracker.observe(&headers(4100, 3600), 2_000);
        let budget = tracker.budget(2_000).unwrap();
        assert_eq!((budget.limit, budget.remaining), (5000, 4000));
        assert_eq!((budget.resets_at, budget.seen_at), (HOUR, 2_000));
    }

    #[test]
    fn the_level_slows_at_25_percent_and_pauses_at_10_percent() {
        let mut tracker = Tracker::default();
        tracker.observe(&headers(1251, 3600), 0);
        assert_eq!(tracker.level(0), Level::Normal);
        tracker.observe(&headers(1250, 3600), 0);
        assert_eq!(tracker.level(0), Level::Slowed);
        tracker.observe(&headers(501, 3600), 0);
        assert_eq!(tracker.level(0), Level::Slowed);
        tracker.observe(&headers(500, 3600), 0);
        assert_eq!(tracker.level(0), Level::Paused);
        assert_eq!(tracker.paused_until(0), Some(HOUR));
    }

    #[test]
    fn the_level_never_moves_up_before_the_reset_and_is_normal_after_it() {
        let mut tracker = Tracker::default();
        tracker.observe(&headers(1000, 3600), 0);
        tracker.observe(&headers(4900, 3600), 10);
        assert_eq!(tracker.level(HOUR - 1), Level::Slowed);
        assert_eq!(tracker.level(HOUR), Level::Normal);
    }

    #[test]
    fn a_later_reset_after_the_window_ended_starts_a_new_window() {
        let mut tracker = Tracker::default();
        tracker.observe(&headers(400, 3600), 0);
        tracker.observe(&headers(4999, 7200), HOUR - 1);
        assert_eq!(tracker.budget(HOUR - 1).unwrap().remaining, 400);
        tracker.observe(&headers(4999, 7200), HOUR + 5);
        let budget = tracker.budget(HOUR + 5).unwrap();
        assert_eq!((budget.remaining, budget.resets_at), (4999, 2 * HOUR));
        assert_eq!(budget.level, "normal");
    }

    #[test]
    fn a_response_saying_the_limit_is_reached_pauses_until_the_reset() {
        let mut tracker = Tracker::default();
        tracker.observe(&headers(3000, 3600), 0);
        tracker.reached();
        assert_eq!(tracker.level(1), Level::Paused);
        assert_eq!(tracker.budget(1).unwrap().remaining, 0);
        assert_eq!(tracker.level(HOUR), Level::Normal);
    }

    #[test]
    fn a_response_without_the_figures_is_not_counted() {
        let mut tracker = Tracker::default();
        tracker.observe(
            &RateHeaders {
                remaining: Some(1),
                ..RateHeaders::default()
            },
            0,
        );
        assert_eq!(tracker.budget(0), None);
    }
}
