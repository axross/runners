//! times as the window words them. The view knows no time zone: the caller says how
//! far local time is from UTC, so a test sets the clock and the app reads the system's.

const MINUTE_MS: i64 = 60_000;
const DAY_MS: i64 = 86_400_000;

/// the time now and the local offset from UTC, which the caller reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Clock {
    /// milliseconds since the Unix epoch.
    pub now_ms: u64,
    /// minutes local time is ahead of UTC.
    pub utc_offset_minutes: i32,
}

/// a time against today.
enum Day {
    Today,
    Yesterday,
    On(String),
}

impl Clock {
    #[must_use]
    pub fn new(now_ms: u64, utc_offset_minutes: i32) -> Self {
        Self {
            now_ms,
            utc_offset_minutes,
        }
    }

    fn local(&self, ms: u64) -> i64 {
        i64::try_from(ms)
            .unwrap_or(i64::MAX)
            .saturating_add(i64::from(self.utc_offset_minutes) * MINUTE_MS)
    }

    fn day_of(&self, ms: u64) -> Day {
        let days = self.local(ms).div_euclid(DAY_MS);
        let today = self.local(self.now_ms).div_euclid(DAY_MS);
        if days == today {
            Day::Today
        } else if days == today - 1 {
            Day::Yesterday
        } else {
            let (year, month, day) = civil(days);
            Day::On(format!("{year:04}-{month:02}-{day:02}"))
        }
    }

    fn time_of_day(&self, ms: u64) -> String {
        let minutes = self.local(ms).rem_euclid(DAY_MS) / MINUTE_MS;
        format!("{:02}:{:02}", minutes / 60, minutes % 60)
    }

    /// "03:15" for a time today, else with its day: "yesterday 03:15" or
    /// "2026-11-03 03:15".
    #[must_use]
    pub fn when(&self, ms: u64) -> String {
        match self.day_of(ms) {
            Day::Today => self.time_of_day(ms),
            Day::Yesterday => format!("yesterday {}", self.time_of_day(ms)),
            Day::On(date) => format!("{date} {}", self.time_of_day(ms)),
        }
    }

    /// as [`Clock::when`], but a time today says so: "today 03:15".
    #[must_use]
    pub fn when_with_day(&self, ms: u64) -> String {
        match self.day_of(ms) {
            Day::Today => format!("today {}", self.time_of_day(ms)),
            _ => self.when(ms),
        }
    }
}

/// the proleptic Gregorian date of a day count since 1970-01-01.
fn civil(days: i64) -> (i64, i64, i64) {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// a span as the window words it: "42 s", "1 min 42 s", "1 h 5 min".
#[must_use]
pub fn elapsed(ms: u64) -> String {
    let seconds = ms / 1_000;
    match (seconds / 3_600, seconds % 3_600 / 60, seconds % 60) {
        (0, 0, seconds) => format!("{seconds} s"),
        (0, minutes, seconds) => format!("{minutes} min {seconds} s"),
        (hours, minutes, _) => format!("{hours} h {minutes} min"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // 2026-11-04 03:52 UTC
    const NOW: u64 = 1_793_764_320_000;

    #[test]
    fn a_time_today_is_a_time_of_day() {
        let clock = Clock::new(NOW, 0);
        assert_eq!(clock.when(NOW - 37 * 60_000), "03:15");
        assert_eq!(clock.when_with_day(NOW - 37 * 60_000), "today 03:15");
    }

    #[test]
    fn the_offset_moves_the_day_and_the_hour() {
        let clock = Clock::new(NOW, -9 * 60);
        assert_eq!(clock.when(NOW), "18:52");
        // 07:52 UTC the day before is 22:52 the day before that, local
        assert_eq!(clock.when_with_day(NOW - 20 * 3_600_000), "yesterday 22:52");
        let ahead = Clock::new(NOW, 9 * 60);
        assert_eq!(ahead.when(NOW), "12:52");
    }

    #[test]
    fn an_earlier_day_names_its_date() {
        let clock = Clock::new(NOW, 0);
        assert_eq!(clock.when(NOW - DAY_MS as u64), "yesterday 03:52");
        assert_eq!(clock.when(NOW - 3 * DAY_MS as u64), "2026-11-01 03:52");
        assert_eq!(
            clock.when_with_day(NOW - 3 * DAY_MS as u64),
            "2026-11-01 03:52"
        );
    }

    #[test]
    fn the_calendar_is_right_across_a_leap_day() {
        // 2028-02-29 12:00 UTC
        let leap = 1_835_438_400_000;
        let clock = Clock::new(leap + 5 * DAY_MS as u64, 0);
        assert_eq!(clock.when(leap), "2028-02-29 12:00");
    }

    #[test]
    fn a_span_is_worded_in_its_two_largest_units() {
        assert_eq!(elapsed(0), "0 s");
        assert_eq!(elapsed(42_000), "42 s");
        assert_eq!(elapsed(102_000), "1 min 42 s");
        assert_eq!(elapsed(161_000), "2 min 41 s");
        assert_eq!(elapsed(3_900_000), "1 h 5 min");
    }
}
