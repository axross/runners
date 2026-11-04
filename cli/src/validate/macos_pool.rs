//! the fields of an axmc pool with `os: "macos"`: the defaults that make `slots`,
//! `cpus`, `memoryGb` and `diskGb` optional, the minimums the golden macOS VM
//! needs, and the refusal of diagnostics; see `docs/operations/cli.md`. a value
//! below its minimum is recorded as an error but still read, so the pool takes part
//! in the cross-entry and budget checks.

use crate::json::Value;

use super::fields::{Errors, Field};
use super::limits::{self, MAX_DISK_GB, MAX_MEMORY_GB, MAX_SLOTS};

pub const DEFAULT_SLOTS: u32 = 1;
pub const DEFAULT_CPUS: f64 = 4.0;
pub const MIN_CPUS: f64 = 2.0;
pub const DEFAULT_MEMORY_GB: u32 = 8;
pub const MIN_MEMORY_GB: u32 = 4;
pub const DEFAULT_DISK_GB: u32 = 50;
pub const MIN_DISK_GB: u32 = 50;

/// reads `slots`: absent is [`DEFAULT_SLOTS`], and a `null` is a type error, as it
/// is for `cpus`.
pub(super) fn read_slots(slots: &Field, path: &str, errors: &mut Errors) -> Option<u32> {
    let Some(value) = slots.get() else {
        return Some(DEFAULT_SLOTS);
    };
    limits::read_bounded_integer(value, &format!("{path}.slots"), MAX_SLOTS, errors)
}

/// reads `cpus`: absent is [`DEFAULT_CPUS`], and a count below [`MIN_CPUS`] after
/// the rounding the limit itself gets is an error.
pub(super) fn read_cpus(cpus: &Field, path: &str, errors: &mut Errors) -> Option<f64> {
    let field = format!("{path}.cpus");
    let Some(value) = cpus.get() else {
        return Some(DEFAULT_CPUS);
    };
    let cpus = limits::cpu_count(value, &field, errors)?;
    if limits::nanos(cpus) < limits::nanos(MIN_CPUS) {
        errors.add(format!(
            "{field}: must be at least {MIN_CPUS} on a macOS pool"
        ));
    }
    Some(cpus)
}

/// reads `memoryGb`: absent is [`DEFAULT_MEMORY_GB`].
pub(super) fn read_memory(memory_gb: &Field, path: &str, errors: &mut Errors) -> Option<u32> {
    let field = format!("{path}.memoryGb");
    let Some(value) = memory_gb.get() else {
        return Some(DEFAULT_MEMORY_GB);
    };
    read_at_least(value, &field, MAX_MEMORY_GB, MIN_MEMORY_GB, "", errors)
}

/// reads `diskGb`: absent is [`DEFAULT_DISK_GB`], and the golden macOS VM's disk is
/// the least a pool can ask for.
pub(super) fn read_disk(disk_gb: &Field, path: &str, errors: &mut Errors) -> Option<u32> {
    let field = format!("{path}.diskGb");
    let Some(value) = disk_gb.get() else {
        return Some(DEFAULT_DISK_GB);
    };
    read_at_least(
        value,
        &field,
        MAX_DISK_GB,
        MIN_DISK_GB,
        ", the golden macOS VM's disk size",
        errors,
    )
}

/// records that diagnostics are refused; the caller calls it only for a pool that
/// enables them.
pub(super) fn refuse_diagnostics(path: &str, errors: &mut Errors) {
    errors.add(format!(
        "{path}.diagnostics: not supported on a macOS pool, because macOS VM pools have no diagnostics"
    ));
}

fn read_at_least(
    value: &Value,
    field: &str,
    max: u32,
    min: u32,
    reason: &str,
    errors: &mut Errors,
) -> Option<u32> {
    let integer = limits::read_bounded_integer(value, field, max, errors)?;
    if integer < min {
        errors.add(format!(
            "{field}: must be at least {min} on a macOS pool{reason}"
        ));
    }
    Some(integer)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(text: &str) -> Field {
        Field::Value(crate::json::parse(text.as_bytes()).unwrap())
    }

    fn read<T>(
        reader: fn(&Field, &str, &mut Errors) -> Option<T>,
        field: &Field,
    ) -> (Option<T>, Vec<String>) {
        let mut errors = Errors::default();
        let read = reader(field, "repositories[2]", &mut errors);
        (read, errors.into_vec())
    }

    #[test]
    fn an_absent_field_takes_its_default() {
        assert_eq!(read(read_slots, &Field::Absent), (Some(1), vec![]));
        assert_eq!(read(read_cpus, &Field::Absent), (Some(4.0), vec![]));
        assert_eq!(read(read_memory, &Field::Absent), (Some(8), vec![]));
        assert_eq!(read(read_disk, &Field::Absent), (Some(50), vec![]));
    }

    #[test]
    fn a_set_field_replaces_its_default_and_each_minimum_is_inclusive() {
        assert_eq!(read(read_slots, &field("2")), (Some(2), vec![]));
        assert_eq!(read(read_cpus, &field("2")), (Some(2.0), vec![]));
        assert_eq!(read(read_memory, &field("4")), (Some(4), vec![]));
        assert_eq!(read(read_disk, &field("50")), (Some(50), vec![]));
        assert_eq!(read(read_disk, &field("80")), (Some(80), vec![]));
    }

    #[test]
    fn a_value_below_a_minimum_is_refused_with_the_pools_index() {
        let (cpus, errors) = read(read_cpus, &field("1.5"));
        assert_eq!(cpus, Some(1.5));
        assert_eq!(
            errors,
            ["repositories[2].cpus: must be at least 2 on a macOS pool"]
        );
        let (memory, errors) = read(read_memory, &field("3"));
        assert_eq!(memory, Some(3));
        assert_eq!(
            errors,
            ["repositories[2].memoryGb: must be at least 4 on a macOS pool"]
        );
        let (disk, errors) = read(read_disk, &field("49"));
        assert_eq!(disk, Some(49));
        assert_eq!(
            errors,
            [
                "repositories[2].diskGb: must be at least 50 on a macOS pool, the golden macOS VM's disk size"
            ]
        );
    }

    #[test]
    fn the_cpu_minimum_applies_after_the_rounding_the_limit_gets() {
        assert_eq!(
            read(read_cpus, &field("1.9999999996")).1,
            Vec::<String>::new()
        );
        assert_eq!(read(read_cpus, &field("1.999999999")).1.len(), 1);
    }

    #[test]
    fn a_null_or_mistyped_field_is_a_type_error_not_a_default() {
        let (slots, errors) = read(read_slots, &Field::Null);
        assert_eq!(slots, None);
        assert_eq!(
            errors,
            ["repositories[2].slots: must be an integer from 1 to 16"]
        );
        assert_eq!(read(read_disk, &field("\"50\"")).0, None);
    }
}
