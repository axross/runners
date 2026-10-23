//! The host-wide budget of an axmc configuration.

use crate::json::Object;
use crate::profile::Os;

use super::fields::{self, Errors};
use super::limits::{self, MAX_BUDGET_MEMORY_GB, MAX_DISK_GB};
use super::plan::{Budget, Entry};

const BUDGET_FIELDS: [&str; 3] = ["cpus", "memoryGb", "diskGb"];

/// The most macOS virtual machines the host runs at once.
pub const MAX_MACOS_VMS: u32 = 2;

/// Reads the root `budget` object, or records why it is unusable.
pub fn read_budget(root: &Object, errors: &mut Errors) -> Option<Budget> {
    let value = fields::required(root, "budget", "budget", errors)?;
    let node = fields::object(value, "budget", errors)?;
    fields::unknown_fields(node, &BUDGET_FIELDS, "budget", errors);

    let cpus = fields::required(node, "cpus", "budget.cpus", errors)
        .and_then(|value| limits::cpu_count(value, "budget.cpus", errors));
    let memory_gb =
        fields::required(node, "memoryGb", "budget.memoryGb", errors).and_then(|value| {
            limits::read_bounded_integer(value, "budget.memoryGb", MAX_BUDGET_MEMORY_GB, errors)
        });
    let disk_gb = fields::required(node, "diskGb", "budget.diskGb", errors).and_then(|value| {
        limits::read_bounded_integer(value, "budget.diskGb", MAX_DISK_GB, errors)
    });
    Some(Budget {
        cpus: cpus?,
        memory_gb: memory_gb?,
        disk_gb: disk_gb?,
    })
}

/// What the pools use together: the sum over the pools of slots times the limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Usage {
    pub macos_vms: u32,
    pub cpu_nanos: u128,
    pub memory_gb: u64,
    pub disk_gb: u64,
}

#[must_use]
pub fn usage(entries: &[Entry]) -> Usage {
    let mut total = Usage {
        macos_vms: 0,
        cpu_nanos: 0,
        memory_gb: 0,
        disk_gb: 0,
    };
    for entry in entries {
        let slots = u64::from(entry.slots);
        if entry.os == Some(Os::Macos) {
            total.macos_vms += entry.slots;
        }
        total.cpu_nanos += u128::from(entry.slots) * limits::nanos(entry.cpus);
        total.memory_gb += slots * u64::from(entry.memory_gb);
        total.disk_gb += slots * u64::from(entry.disk_gb.unwrap_or(0));
    }
    total
}

/// Records each limit the pools together exceed, at path `budget`. The macOS limit
/// is fixed by the host, so it is checked even when the budget itself is unusable.
pub fn check_budget(entries: &[Entry], budget: Option<&Budget>, errors: &mut Errors) {
    let used = usage(entries);
    if used.macos_vms > MAX_MACOS_VMS {
        errors.add(format!(
            "budget: macOS pools total {} VMs (the sum of their slots), above the limit of {MAX_MACOS_VMS}",
            used.macos_vms
        ));
    }
    let Some(budget) = budget else {
        return;
    };
    let budget_nanos = limits::nanos(budget.cpus);
    if used.cpu_nanos > budget_nanos {
        errors.add(format!(
            "budget: pools total {} CPUs (the sum of slots times cpus), above the budget of {}",
            limits::format_nanos(used.cpu_nanos),
            limits::format_nanos(budget_nanos)
        ));
    }
    if used.memory_gb > u64::from(budget.memory_gb) {
        errors.add(format!(
            "budget: pools total {} GB of memory (the sum of slots times memoryGb), above the budget of {} GB",
            used.memory_gb, budget.memory_gb
        ));
    }
    if used.disk_gb > u64::from(budget.disk_gb) {
        errors.add(format!(
            "budget: pools total {} GB of disk (the sum of slots times diskGb), above the budget of {} GB",
            used.disk_gb, budget.disk_gb
        ));
    }
}
