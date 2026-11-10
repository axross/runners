//! runner and container names, `<pool>-<slot>-<yyyyMMddHHmmssfff>` in UTC; cleanup
//! recognises a pool's own containers by this shape and nothing looser.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub(crate) const DIGITS_IN_STAMP: usize = 17;
pub(crate) const DIGITS_IN_SECONDS_STAMP: usize = 14;
const LATEST_UNIX_MILLIS: u64 = 253_402_300_799_999;
const MILLIS_PER_DAY: u64 = 86_400_000;

/// the next container name of a pool's slot, counting slots from 1.
#[must_use]
pub fn job_container_name(pool: &str, slot: u32, now: SystemTime) -> String {
    format!("{pool}-{slot}-{}", stamp(now))
}

/// writes `time` as `yyyyMMddHHmmssfff` in UTC; a time before the epoch as the
/// epoch.
#[must_use]
pub fn stamp(time: SystemTime) -> String {
    let since_epoch = time.duration_since(UNIX_EPOCH).unwrap_or(Duration::ZERO);
    format_stamp(u64::try_from(since_epoch.as_millis()).unwrap_or(u64::MAX))
}

/// writes milliseconds since the Unix epoch as `yyyyMMddHHmmssfff` in UTC, clamped
/// to the last millisecond of the year 9999.
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

/// reads a `yyyyMMddHHmmssfff` stamp, or a `yyyyMMddHHmmss` stamp as its whole
/// second, as milliseconds since the Unix epoch; `None` for any other text, a date
/// that does not exist, or a year before 1970.
#[must_use]
pub fn parse_stamp(text: &str) -> Option<u64> {
    let seconds_only = text.len() == DIGITS_IN_SECONDS_STAMP;
    if !(seconds_only || text.len() == DIGITS_IN_STAMP)
        || !text.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    let field = |from: usize, to: usize| -> Option<u64> { text.get(from..to)?.parse().ok() };
    let (year, month, day) = (field(0, 4)?, field(4, 6)?, field(6, 8)?);
    let (hour, minute, second) = (field(8, 10)?, field(10, 12)?, field(12, 14)?);
    let milli = if seconds_only { 0 } else { field(14, 17)? };
    if year < 1970
        || !(1..=12).contains(&month)
        || day == 0
        || hour > 23
        || minute > 59
        || second > 59
    {
        return None;
    }
    let days = days_from_civil(year, month, day);
    if civil_from_days(days) != (year, month, day) {
        return None;
    }
    Some((((days * 24 + hour) * 60 + minute) * 60 + second) * 1000 + milli)
}

/// converts days since 1970-01-01 to a proleptic Gregorian date, by Howard
/// Hinnant's `civil_from_days`.
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

/// the inverse of `civil_from_days`, for dates from 1970 on.
pub(crate) fn days_from_civil(year: u64, month: u64, day: u64) -> u64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year / 400;
    let year_of_era = year - era * 400;
    let month_index = if month > 2 { month - 3 } else { month + 9 };
    let day_of_year = (153 * month_index + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    (era * 146_097 + day_of_era).saturating_sub(719_468)
}

/// the name of a pool's CPU probe container for a slot.
#[must_use]
pub fn cpu_probe_name(pool: &str, slot: u32) -> String {
    format!("{pool}-cpu-probe-{slot}")
}

/// whether `candidate` is the pool's name, a hyphen, a slot number, a hyphen and
/// exactly 17 ASCII digits, and nothing else; case matters.
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
    fn a_pinned_instant_gives_the_expected_name() {
        // 2024-03-05 06:07:08.009 UTC.
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
    fn a_stamp_reads_back_as_its_milliseconds_and_a_short_one_as_its_second() {
        for millis in [0, 1_709_618_828_009, 4_107_542_399_999, 951_782_400_000] {
            assert_eq!(parse_stamp(&format_stamp(millis)), Some(millis));
        }
        assert_eq!(
            parse_stamp("20240305060708"),
            Some(1_709_618_828_000),
            "a stamp without milliseconds"
        );
    }

    #[test]
    fn a_stamp_that_names_no_instant_does_not_read() {
        for text in [
            "",
            "2024030506070800",
            "202403050607080091",
            "20240230060708009",
            "20240305240000000",
            "20240305066000000",
            "20240305060760000",
            "20241305060708009",
            "20240005060708009",
            "20240300060708009",
            "19691231235959999",
            "2024030506070800x",
            "+2024030506070800",
            "20240305060708 09",
        ] {
            assert_eq!(parse_stamp(text), None, "{text:?}");
        }
    }

    #[test]
    fn a_time_before_the_epoch_is_the_epoch() {
        let before = UNIX_EPOCH - Duration::from_secs(5);
        assert_eq!(job_container_name("p", 1, before), "p-1-19700101000000000");
    }

    #[test]
    fn own_names_follow_the_job_container_pattern() {
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
