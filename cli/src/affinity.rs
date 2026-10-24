//! Where each slot's CPUs start, and which of the CPUs a container may use it takes.
//!
//! The offsets are shared by the plan summary `validate` prints and the backend, so
//! the two agree. The selection reproduces the PowerShell supervisor's
//! `Select-CpuAffinity`: a slot starts at its offset modulo the available CPUs and
//! takes consecutive IDs from the sorted inventory, wrapping at the end.

use std::fmt;

use crate::validate::Entry;

/// The longest allowed-CPU list the selection reads, which is also the most the
/// backend accepts from the probe.
pub const MAX_CPU_LIST_BYTES: usize = 64 * 1024;

/// The CPUs a slot's affinity selects: the quota rounded up.
#[must_use]
pub fn affinity_count(entry: &Entry) -> u64 {
    entry.cpus.ceil() as u64
}

/// Where the entry's first slot starts among the affinity positions, after every
/// preceding entry's slots.
#[must_use]
pub fn affinity_offset(entries: &[Entry], index: usize) -> u64 {
    entries
        .iter()
        .take(index)
        .map(|preceding| u64::from(preceding.slots) * affinity_count(preceding))
        .sum()
}

/// Where a slot starts, counting slots from 1: its entry's offset, then the CPUs
/// of the entry's earlier slots.
#[must_use]
pub fn slot_offset(entries: &[Entry], index: usize, slot: u32) -> u64 {
    let preceding_slots = u64::from(slot.saturating_sub(1));
    let count = entries.get(index).map_or(0, affinity_count);
    affinity_offset(entries, index) + preceding_slots * count
}

/// Why an allowed-CPU list or a request against it was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CpuError {
    InvalidList,
    UnrepresentableId,
    NotOrderedAndDistinct,
    CountOutOfRange,
    CountExceedsAvailable,
}

impl fmt::Display for CpuError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidList => "CPU discovery: invalid allowed CPU list.",
            Self::UnrepresentableId => "CPU discovery: unrepresentable CPU ID.",
            Self::NotOrderedAndDistinct => {
                "CPU discovery: allowed CPU list is not ordered and distinct."
            }
            Self::CountOutOfRange => "CPU discovery: affinity cardinality is not from 1 to 64.",
            Self::CountExceedsAvailable => {
                "CPU discovery: affinity cardinality exceeds available CPUs."
            }
        })
    }
}

impl std::error::Error for CpuError {}

/// The largest affinity cardinality, which is the largest `cpus` a pool may ask for.
const MAX_COUNT: u64 = 64;

/// One range of the allowed list, both ends included.
#[derive(Clone, Copy)]
struct Span {
    first: i32,
    last: i32,
}

impl Span {
    fn len(self) -> u64 {
        u64::try_from(i64::from(self.last) - i64::from(self.first)).map_or(0, |gap| gap + 1)
    }
}

/// Selects `count` CPU IDs from the allowed list `available` (the text of
/// `Cpus_allowed_list`), starting at `offset` modulo the number of allowed CPUs,
/// and returns them in canonical `a,b-c` form.
///
/// # Errors
///
/// The list must be ordered, distinct ranges of IDs that fit an `i32`, with no
/// space, sign or leading zero, at most [`MAX_CPU_LIST_BYTES`] long; `count` must
/// be from 1 to 64 and no larger than the number of allowed CPUs.
pub fn select_cpus(available: &str, count: u64, offset: u64) -> Result<String, CpuError> {
    if !(1..=MAX_COUNT).contains(&count) {
        return Err(CpuError::CountOutOfRange);
    }
    let spans = parse_list(available)?;
    let total: u64 = spans.iter().map(|span| span.len()).sum();
    if count > total {
        return Err(CpuError::CountExceedsAvailable);
    }
    let start = offset % total;
    let mut selected: Vec<i32> = (0..count)
        .filter_map(|index| id_at((start + index) % total, &spans))
        .collect();
    selected.sort_unstable();
    Ok(canonical(&selected))
}

fn parse_list(available: &str) -> Result<Vec<Span>, CpuError> {
    if available.len() > MAX_CPU_LIST_BYTES {
        return Err(CpuError::InvalidList);
    }
    // The whole list's syntax is judged before any ID's range or order, as the
    // PowerShell `Select-CpuAffinity` does, so a list with several defects gets the
    // same refusal from both.
    if !available.split(',').all(is_well_formed_range) {
        return Err(CpuError::InvalidList);
    }
    let mut spans: Vec<Span> = Vec::new();
    let mut previous_last: Option<i32> = None;
    for part in available.split(',') {
        let (first_text, last_text) = match part.split_once('-') {
            Some((first, last)) => (first, Some(last)),
            None => (part, None),
        };
        let first = parse_id(first_text)?;
        let last = match last_text {
            Some(text) => parse_id(text)?,
            None => first,
        };
        if previous_last.is_some_and(|previous| first <= previous) || last < first {
            return Err(CpuError::NotOrderedAndDistinct);
        }
        previous_last = Some(last);
        spans.push(Span { first, last });
    }
    Ok(spans)
}

/// An ID or an `ID-ID` range, with at most one `-`.
fn is_well_formed_range(part: &str) -> bool {
    match part.split_once('-') {
        Some((first, last)) => is_well_formed_id(first) && is_well_formed_id(last),
        None => is_well_formed_id(part),
    }
}

/// Decimal digits with no sign, space, or leading zero (a lone `0` is fine).
fn is_well_formed_id(text: &str) -> bool {
    let digits = text.as_bytes();
    !digits.is_empty()
        && digits.iter().all(u8::is_ascii_digit)
        && (digits.len() == 1 || digits.first() != Some(&b'0'))
}

/// Parses an ID that [`is_well_formed_id`] accepted; one beyond `i32` is refused.
fn parse_id(text: &str) -> Result<i32, CpuError> {
    text.parse().map_err(|_| CpuError::UnrepresentableId)
}

/// Returns the ID at `position` among the allowed CPUs counted in order.
fn id_at(mut position: u64, spans: &[Span]) -> Option<i32> {
    for span in spans {
        if position < span.len() {
            return i32::try_from(u64::try_from(span.first).ok()? + position).ok();
        }
        position -= span.len();
    }
    None
}

/// Writes sorted, distinct IDs as comma-separated IDs and runs: `2,5-6,11`.
fn canonical(sorted: &[i32]) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut ids = sorted.iter().copied().peekable();
    while let Some(first) = ids.next() {
        let mut last = first;
        while let Some(next) = ids.next_if(|next| i64::from(*next) == i64::from(last) + 1) {
            last = next;
        }
        parts.push(if last == first {
            first.to_string()
        } else {
            format!("{first}-{last}")
        });
    }
    parts.join(",")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The inventory and the five slots of the PowerShell golden case in
    /// `test-cpu-affinity.ps1`: entries of 2 CPUs with 2 slots and 1 slot give the
    /// offsets 0, 2 and 4, and the last two offsets (6 and 5) overcommit.
    const INVENTORY: &str = "2,5-6,11,14-15";

    #[test]
    fn the_powershell_golden_case_selects_the_same_sets() {
        let sets: Vec<String> = [0, 2, 4, 6, 5]
            .iter()
            .map(|offset| select_cpus(INVENTORY, 2, *offset).unwrap())
            .collect();
        assert_eq!(sets.join("|"), "2,5|6,11|14-15|2,5|2,15");
    }

    #[test]
    fn the_powershell_rejected_inventories_are_rejected() {
        for inventory in [
            "",
            "0,0",
            "4-2",
            "1, 3",
            "1;exit",
            "2,1",
            "01",
            "2147483648",
            "2",
            "2,5\nextra",
        ] {
            assert!(select_cpus(inventory, 2, 0).is_err(), "{inventory:?}");
        }
    }

    #[test]
    fn the_syntax_of_the_whole_list_is_judged_before_ranges_and_order() {
        // PowerShell matches the whole list against one pattern first, so each of
        // these is an invalid list there, not an unrepresentable or misordered one.
        for list in ["2,1,x", "2147483648,x", "4-2,x", "x,2147483648", "2,1,"] {
            assert_eq!(
                select_cpus(list, 1, 0),
                Err(CpuError::InvalidList),
                "{list}"
            );
        }
    }

    #[test]
    fn a_list_with_one_defect_keeps_its_own_refusal() {
        for (list, error) in [
            ("2,1", CpuError::NotOrderedAndDistinct),
            ("0,0", CpuError::NotOrderedAndDistinct),
            ("4-2", CpuError::NotOrderedAndDistinct),
            ("2147483648", CpuError::UnrepresentableId),
            ("0-2147483648", CpuError::UnrepresentableId),
            ("2147483648,2", CpuError::UnrepresentableId),
            ("x", CpuError::InvalidList),
            ("01", CpuError::InvalidList),
            ("1-2-3", CpuError::InvalidList),
        ] {
            assert_eq!(select_cpus(list, 1, 0), Err(error), "{list}");
        }
    }

    #[test]
    fn an_inventory_over_the_byte_limit_is_rejected() {
        let mut long = String::from("0");
        let mut next = 1;
        while long.len() <= MAX_CPU_LIST_BYTES {
            long.push_str(&format!(",{next}"));
            next += 1;
        }
        assert_eq!(select_cpus(&long, 1, 0), Err(CpuError::InvalidList));
        long.truncate(MAX_CPU_LIST_BYTES - 6);
        long.truncate(long.rfind(',').unwrap());
        assert!(select_cpus(&long, 1, 0).is_ok());
    }

    #[test]
    fn the_count_must_be_from_one_to_sixty_four() {
        assert_eq!(select_cpus("0-99", 0, 0), Err(CpuError::CountOutOfRange));
        assert_eq!(select_cpus("0-99", 65, 0), Err(CpuError::CountOutOfRange));
        assert_eq!(select_cpus("0-99", 64, 0).unwrap(), "0-63");
        assert_eq!(
            select_cpus("0-3", 5, 0),
            Err(CpuError::CountExceedsAvailable)
        );
    }

    #[test]
    fn a_selection_that_wraps_merges_adjacent_ids() {
        assert_eq!(select_cpus("0-3", 3, 3).unwrap(), "0-1,3");
        assert_eq!(select_cpus("4-5,7", 2, 2).unwrap(), "4,7");
        assert!(select_cpus("0-2147483647", 2, u64::MAX).is_ok());
    }

    #[test]
    fn the_largest_representable_id_is_selected_without_overflow() {
        assert_eq!(
            select_cpus("2147483646-2147483647", 2, 0).unwrap(),
            "2147483646-2147483647"
        );
    }

    fn entry(slots: u32, cpus: f64) -> Entry {
        Entry {
            path: String::new(),
            owner: String::new(),
            repository: String::new(),
            name: String::new(),
            slots,
            token_path: String::new(),
            labels: Vec::new(),
            cpus,
            memory_gb: 8,
            volumes: Vec::new(),
            diagnostics: false,
            diagnostic_raw_records: false,
            os: None,
            disk_gb: None,
        }
    }

    #[test]
    fn offsets_count_the_ceiling_of_cpus_for_every_preceding_slot() {
        let entries = [entry(2, 1.5), entry(1, 1.000_000_000_1), entry(3, 4.0)];
        assert_eq!(affinity_count(&entries[1]), 2);
        assert_eq!(affinity_offset(&entries, 0), 0);
        assert_eq!(affinity_offset(&entries, 1), 4);
        assert_eq!(affinity_offset(&entries, 2), 6);
        assert_eq!(slot_offset(&entries, 0, 1), 0);
        assert_eq!(slot_offset(&entries, 0, 2), 2);
        assert_eq!(slot_offset(&entries, 2, 3), 6 + 8);
        assert_eq!(slot_offset(&entries, 9, 1), affinity_offset(&entries, 9));
    }
}
