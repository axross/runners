//! Runner and container names: `<pool>-<slot>-<yyyyMMddHHmmssfff>` in UTC.
//!
//! The name is the GitHub runner name and the container name at once. Cleanup
//! recognises a pool's own containers by this shape and nothing looser.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

const DIGITS_IN_STAMP: usize = 17;
const LATEST_UNIX_MILLIS: u64 = 253_402_300_799_999;
const MILLIS_PER_DAY: u64 = 86_400_000;

/// Returns the next container name of a pool's slot, counting slots from 1.
#[must_use]
pub fn job_container_name(pool: &str, slot: u32, now: SystemTime) -> String {
    format!("{pool}-{slot}-{}", stamp(now))
}

/// Writes `time` as `yyyyMMddHHmmssfff` in UTC; a time before the epoch as the
/// epoch.
#[must_use]
pub fn stamp(time: SystemTime) -> String {
    let since_epoch = time.duration_since(UNIX_EPOCH).unwrap_or(Duration::ZERO);
    format_stamp(u64::try_from(since_epoch.as_millis()).unwrap_or(u64::MAX))
}

/// Writes milliseconds since the Unix epoch as `yyyyMMddHHmmssfff` in UTC. A time
/// after the year 9999 is written as the last millisecond of that year.
#[must_use]
pub fn format_stamp(unix_millis: u64) -> String {
    let millis = unix_millis.min(LATEST_UNIX_MILLIS);
    let days = millis / MILLIS_PER_DAY;
    let in_day = millis % MILLIS_PER_DAY;
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}{month:02}{day:02}{:02}{:02}{:02}{:03}",
        in_day / 3_600_000,
        in_day / 60_000 % 60,
        in_day / 1000 % 60,
        in_day % 1000
    )
}

/// Converts days since 1970-01-01 to a proleptic Gregorian date (Howard Hinnant's
/// `civil_from_days`), for days from the epoch on.
pub(crate) fn civil_from_days(days: u64) -> (u64, u64, u64) {
    let shifted = days + 719_468;
    let era = shifted / 146_097;
    let day_of_era = shifted % 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + u64::from(month <= 2);
    (year, month, day)
}

/// Converts a proleptic Gregorian date from 1970 on to days since 1970-01-01
/// (Howard Hinnant's `days_from_civil`), the inverse of `civil_from_days`.
pub(crate) fn days_from_civil(year: u64, month: u64, day: u64) -> u64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year / 400;
    let year_of_era = year - era * 400;
    let month_index = if month > 2 { month - 3 } else { month + 9 };
    let day_of_year = (153 * month_index + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    (era * 146_097 + day_of_era).saturating_sub(719_468)
}

/// Returns the name of a pool's CPU probe container for a slot.
#[must_use]
pub fn cpu_probe_name(pool: &str, slot: u32) -> String {
    format!("{pool}-cpu-probe-{slot}")
}

/// Whether `candidate` is a container name [`job_container_name`] gives this pool:
/// the pool's name, a hyphen, a slot number, a hyphen, and exactly 17 ASCII digits,
/// and nothing else. Case matters.
#[must_use]
pub fn is_own_container_name(pool: &str, candidate: &str) -> bool {
    let Some(rest) = candidate
        .strip_prefix(pool)
        .and_then(|rest| rest.strip_prefix('-'))
    else {
        return false;
    };
    let Some((slot, stamp)) = rest.split_once('-') else {
        return false;
    };
    !slot.is_empty()
        && slot.bytes().all(|byte| byte.is_ascii_digit())
        && stamp.len() == DIGITS_IN_STAMP
        && stamp.bytes().all(|byte| byte.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(millis: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_millis(millis)
    }

    #[test]
    fn the_powershell_test_pin_gives_the_same_name() {
        // 2024-03-05 06:07:08.009 UTC, the instant test-host-configuration.ps1 pins.
        assert_eq!(
            job_container_name("example-repo-one", 2, at(1_709_618_828_009)),
            "example-repo-one-2-20240305060708009"
        );
    }

    #[test]
    fn stamps_are_utc_across_calendar_edges() {
        for (millis, stamp) in [
            (0, "19700101000000000"),
            (951_782_400_000, "20000229000000000"),
            (4_107_542_399_999, "21000228235959999"),
            (4_107_542_400_000, "21000301000000000"),
            (1_735_689_599_999, "20241231235959999"),
            (1_735_689_600_000, "20250101000000000"),
            (253_402_300_799_999, "99991231235959999"),
            (u64::MAX, "99991231235959999"),
        ] {
            assert_eq!(format_stamp(millis), stamp, "{millis}");
            assert_eq!(stamp.len(), DIGITS_IN_STAMP);
        }
    }

    #[test]
    fn a_time_before_the_epoch_is_the_epoch() {
        let before = UNIX_EPOCH - Duration::from_secs(5);
        assert_eq!(job_container_name("p", 1, before), "p-1-19700101000000000");
    }

    #[test]
    fn own_names_follow_the_powershell_pattern() {
        let stamp = "20240305060708009";
        assert!(is_own_container_name(
            "example-repo-one",
            &format!("example-repo-one-2-{stamp}")
        ));
        assert!(is_own_container_name("p", &format!("p-12-{stamp}")));
        for (what, name) in [
            ("a non-numeric slot", format!("example-repo-one-x-{stamp}")),
            ("an empty slot", format!("example-repo-one--{stamp}")),
            (
                "a 16-digit timestamp",
                format!("example-repo-one-1-{}", &stamp[1..]),
            ),
            (
                "an 18-digit timestamp",
                format!("example-repo-one-1-{stamp}0"),
            ),
            (
                "a longer name before it",
                format!("xexample-repo-one-1-{stamp}"),
            ),
            (
                "another entry extending it",
                format!("example-repo-one-extra-1-{stamp}"),
            ),
            (
                "a trailing newline",
                format!("example-repo-one-1-{stamp}\n"),
            ),
            ("another case", format!("Example-repo-one-1-{stamp}")),
            ("a probe", "example-repo-one-cpu-probe-1".to_string()),
            (
                "a non-digit stamp",
                "example-repo-one-1-2024030506070800x".to_string(),
            ),
        ] {
            assert!(!is_own_container_name("example-repo-one", &name), "{what}");
        }
    }

    #[test]
    fn the_probe_name_is_the_pool_then_cpu_probe_then_the_slot() {
        assert_eq!(
            cpu_probe_name("example-entry", 1),
            "example-entry-cpu-probe-1"
        );
    }
}
