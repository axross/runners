//! CPU, memory, disk and slot limits, and the CPU count as text.

use crate::json::Value;

use super::fields::Errors;

pub const DEFAULT_CPUS: f64 = 2.0;
pub const MAX_CPUS: f64 = 64.0;
pub const DEFAULT_MEMORY_GB: u32 = 8;
pub const MAX_MEMORY_GB: u32 = 256;
pub const MAX_SLOTS: u32 = 16;
pub const MAX_BUDGET_MEMORY_GB: u32 = 1024;
pub const MAX_DISK_GB: u32 = 16384;

const NANOS_PER_CPU: u128 = 1_000_000_000;
const SIGNIFICANT_DIGITS_AFTER_FIRST: usize = 14;

/// Returns an integer field's value when it is an integer literal from 1 to `max`.
/// A literal such as `8.0` or `1e1` is not an integer, as in PowerShell.
#[must_use]
pub fn bounded_integer(value: &Value, max: u32) -> Option<u32> {
    let Value::Number(number) = value else {
        return None;
    };
    let integer = number.as_integer()?;
    u32::try_from(integer)
        .ok()
        .filter(|n| (1..=max).contains(n))
}

/// Reads a CPU count, recording why it is unusable. A count must be above 0 and at
/// most 64, and large enough that its text form is not 0, which Docker reads as no
/// limit.
pub fn cpu_count(value: &Value, path: &str, errors: &mut Errors) -> Option<f64> {
    let Value::Number(number) = value else {
        errors.add(format!("{path}: {RANGE_PROBLEM}"));
        return None;
    };
    let cpus = number.as_f64();
    if cpus.is_nan() || cpus <= 0.0 || cpus > MAX_CPUS {
        errors.add(format!("{path}: {RANGE_PROBLEM}"));
        return None;
    }
    if nanos(cpus) == 0 {
        errors.add(format!(
            "{path}: {RANGE_PROBLEM}, and large enough not to be written as 0, which Docker reads as no limit"
        ));
        return None;
    }
    Some(cpus)
}

const RANGE_PROBLEM: &str = "must be a number greater than 0 and at most 64";

/// Returns the CPU count in billionths, rounded half away from zero at nine
/// decimals on the value's 15 significant digits. That is what the PowerShell
/// validator's `0.#########` format does, and it differs from Rust's own rounding.
#[must_use]
pub fn nanos(cpus: f64) -> u128 {
    let scientific = format!("{cpus:.SIGNIFICANT_DIGITS_AFTER_FIRST$e}");
    let (mantissa, exponent) = scientific
        .split_once('e')
        .unwrap_or((scientific.as_str(), "0"));
    let digits: Vec<u32> = mantissa.chars().filter_map(|c| c.to_digit(10)).collect();
    let exponent: i64 = exponent.parse().unwrap_or(0);
    let kept = exponent + 1 + 9;
    let Ok(kept_digits) = usize::try_from(kept) else {
        return 0;
    };
    let mut scaled: u128 = 0;
    for position in 0..kept_digits {
        scaled = scaled * 10 + u128::from(digits.get(position).copied().unwrap_or(0));
    }
    let round_up = digits.get(kept_digits).is_some_and(|digit| *digit >= 5);
    scaled + u128::from(round_up)
}

/// Returns the CPU count as text for the summary: at most nine decimals, with no
/// trailing zeros, whatever the machine's regional settings.
#[must_use]
pub fn format_cpu_count(cpus: f64) -> String {
    format_nanos(nanos(cpus))
}

/// Returns a count given in billionths as text, without trailing zeros.
#[must_use]
pub fn format_nanos(nanos: u128) -> String {
    let whole = nanos / NANOS_PER_CPU;
    let fraction = nanos % NANOS_PER_CPU;
    if fraction == 0 {
        return whole.to_string();
    }
    let fraction = format!("{fraction:09}");
    format!("{whole}.{}", fraction.trim_end_matches('0'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounds_half_away_from_zero_at_nine_decimals() {
        for (cpus, text) in [
            (5e-10, "0.000000001"),
            (4.999_999_999_999_9e-10, "0"),
            (1.5e-9, "0.000000002"),
            (1e-10, "0"),
            (0.123_456_789_5, "0.12345679"),
            (0.300_000_000_000_000_04, "0.3"),
            (0.999_999_999_5, "1"),
            (63.999_999_999_6, "64"),
            (2.0, "2"),
            (1.5, "1.5"),
            (64.0, "64"),
            (10.0, "10"),
        ] {
            assert_eq!(format_cpu_count(cpus), text, "{cpus:e}");
        }
    }

    #[test]
    fn integer_fields_take_only_integer_literals_in_range() {
        let value = |text: &str| crate::json::parse(text.as_bytes()).unwrap();
        assert_eq!(bounded_integer(&value("8"), 16), Some(8));
        assert_eq!(bounded_integer(&value("16"), 16), Some(16));
        assert_eq!(bounded_integer(&value("17"), 16), None);
        assert_eq!(bounded_integer(&value("0"), 16), None);
        assert_eq!(bounded_integer(&value("-1"), 16), None);
        assert_eq!(bounded_integer(&value("8.0"), 16), None);
        assert_eq!(bounded_integer(&value("1e1"), 16), None);
        assert_eq!(
            bounded_integer(&value("99999999999999999999999999999999999999999"), 16),
            None
        );
        assert_eq!(bounded_integer(&value("\"8\""), 16), None);
        assert_eq!(bounded_integer(&value("null"), 16), None);
    }
}
