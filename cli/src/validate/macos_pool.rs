//! the fields of an axmc pool with `os: "macos"`: the defaults that make `slots`,
//! `cpus`, `memoryGb` and `diskGb` optional, and the minimums the golden macOS VM
//! needs; see `docs/operations/cli.md`. a value below its minimum is recorded as an
//! error but still read, so the pool takes part in the cross-entry and budget checks.

use crate::json::Value;

use super::fields::{Errors, Field};
use super::limits::{self, MAX_DISK_GB, MAX_MEMORY_GB, MAX_SLOTS, MAX_VM_CPUS};
use super::problem::{Key, KeyPath, Kind};

pub const DEFAULT_SLOTS: u32 = 1;
pub const DEFAULT_CPUS: u32 = 4;
pub const MIN_CPUS: u32 = 2;
pub const DEFAULT_MEMORY_GB: u32 = 8;
pub const MIN_MEMORY_GB: u32 = 4;
pub const DEFAULT_DISK_GB: u32 = 50;
pub const MIN_DISK_GB: u32 = 50;

/// reads `slots`: absent is [`DEFAULT_SLOTS`], and a `null` is a type error, as it
/// is for `cpus`.
pub(super) fn read_slots(slots: &Field, path: &KeyPath, errors: &mut Errors) -> Option<u32> {
    let Some(value) = slots.get() else {
        return Some(DEFAULT_SLOTS);
    };
    limits::read_bounded_integer(value, &path.key(Key::Slots), MAX_SLOTS, errors)
}

/// reads `cpus`: absent is [`DEFAULT_CPUS`]; a VM has whole vCPUs, as on every
/// axmc pool, and a count below [`MIN_CPUS`] is an error.
pub(super) fn read_cpus(cpus: &Field, path: &KeyPath, errors: &mut Errors) -> Option<f64> {
    let Some(value) = cpus.get() else {
        return Some(f64::from(DEFAULT_CPUS));
    };
    let cpus = read_at_least(
        value,
        &path.key(Key::Cpus),
        MAX_VM_CPUS,
        MIN_CPUS,
        "",
        errors,
    )?;
    Some(f64::from(cpus))
}

/// reads `memoryGb`: absent is [`DEFAULT_MEMORY_GB`].
pub(super) fn read_memory(memory_gb: &Field, path: &KeyPath, errors: &mut Errors) -> Option<u32> {
    let Some(value) = memory_gb.get() else {
        return Some(DEFAULT_MEMORY_GB);
    };
    read_at_least(
        value,
        &path.key(Key::MemoryGb),
        MAX_MEMORY_GB,
        MIN_MEMORY_GB,
        "",
        errors,
    )
}

/// reads `diskGb`: absent is [`DEFAULT_DISK_GB`], and the golden macOS VM's disk is
/// the least a pool can ask for.
pub(super) fn read_disk(disk_gb: &Field, path: &KeyPath, errors: &mut Errors) -> Option<u32> {
    let Some(value) = disk_gb.get() else {
        return Some(DEFAULT_DISK_GB);
    };
    read_at_least(
        value,
        &path.key(Key::DiskGb),
        MAX_DISK_GB,
        MIN_DISK_GB,
        ", the golden macOS VM's disk size",
        errors,
    )
}

fn read_at_least(
    value: &Value,
    field: &KeyPath,
    max: u32,
    min: u32,
    reason: &'static str,
    errors: &mut Errors,
) -> Option<u32> {
    let integer = limits::read_bounded_integer(value, field, max, errors)?;
    if integer < min {
        errors.add(field.clone(), Kind::MacosBelowMinimum { min, reason });
    }
    Some(integer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::Profile;
    use crate::validate::json_reader::JsonReader;

    fn field(text: &str) -> Field {
        Field::Value(crate::json::parse(text.as_bytes()).unwrap())
    }

    fn read<T>(
        reader: fn(&Field, &KeyPath, &mut Errors) -> Option<T>,
        field: &Field,
    ) -> (Option<T>, Vec<String>) {
        let mut errors = Errors::default();
        let path = KeyPath::root().key(Key::Repositories).index(2);
        let read = reader(field, &path, &mut errors);
        (read, errors.render(&JsonReader::new(Profile::Axmc)))
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
        let (cpus, errors) = read(read_cpus, &field("1"));
        assert_eq!(cpus, Some(1.0));
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
    fn a_fractional_cpu_count_is_a_type_error_not_a_minimum() {
        let (cpus, errors) = read(read_cpus, &field("2.5"));
        assert_eq!(cpus, None);
        assert_eq!(
            errors,
            ["repositories[2].cpus: must be an integer from 1 to 64"]
        );
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
