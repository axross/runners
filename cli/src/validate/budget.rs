//! the host-wide budget of an axmc configuration.

use serde::Deserialize;

use crate::profile::Os;

use super::fields::{self, Errors, Field};
use super::limits::{self, MAX_BUDGET_MEMORY_GB, MAX_DISK_GB};
use super::plan::{Budget, Entry};
use super::problem::{Key, KeyPath, Kind};
use super::reader::Reader;

pub(super) const BUDGET_FIELDS: [&str; 3] = ["cpus", "memoryGb", "diskGb"];

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct BudgetFields {
    #[serde(default)]
    cpus: Field,
    #[serde(default)]
    memory_gb: Field,
    #[serde(default)]
    disk_gb: Field,
}

/// the most macOS virtual machines the host runs at once.
pub const MAX_MACOS_VMS: u32 = 2;

/// reads the root `budget` object, or records why it is unusable.
pub fn read_budget(budget: &Field, reader: &dyn Reader, errors: &mut Errors) -> Option<Budget> {
    let path = KeyPath::root().key(Key::Budget);
    let value = fields::required(budget, &path, errors)?;
    let node = fields::read_object::<BudgetFields>(value, &path, reader, errors)?;

    let cpus_path = path.key(Key::Cpus);
    let memory_path = path.key(Key::MemoryGb);
    let disk_path = path.key(Key::DiskGb);
    let cpus = fields::required(&node.cpus, &cpus_path, errors)
        .and_then(|value| limits::whole_cpu_count(value, &cpus_path, errors));
    let memory_gb = fields::required(&node.memory_gb, &memory_path, errors).and_then(|value| {
        limits::read_bounded_integer(value, &memory_path, MAX_BUDGET_MEMORY_GB, errors)
    });
    let disk_gb = fields::required(&node.disk_gb, &disk_path, errors)
        .and_then(|value| limits::read_bounded_integer(value, &disk_path, MAX_DISK_GB, errors));
    Some(Budget {
        cpus: cpus?,
        memory_gb: memory_gb?,
        disk_gb: disk_gb?,
    })
}

/// the disk beyond the slot VMs that any Linux pool needs, for the golden VM and Tart's
/// image cache.
pub const LINUX_DISK_RESERVE_GB: u64 = 40;

/// the disk beyond the slot VMs that any macOS pool needs, for the golden macOS VM and
/// its cached base.
pub const MACOS_DISK_RESERVE_GB: u64 = 100;

/// what the pools use together: the sum over the pools of slots times the limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Usage {
    pub macos_vms: u32,
    pub cpu_nanos: u128,
    pub memory_gb: u64,
    /// the slot VMs' disks and both reserves.
    pub disk_gb: u64,
    pub linux_reserve_gb: u64,
    pub macos_reserve_gb: u64,
}

impl Usage {
    /// the disk both reserves add.
    #[must_use]
    pub fn disk_reserve_gb(&self) -> u64 {
        self.linux_reserve_gb + self.macos_reserve_gb
    }
}

#[must_use]
pub fn usage(entries: &[Entry]) -> Usage {
    let mut total = Usage {
        macos_vms: 0,
        cpu_nanos: 0,
        memory_gb: 0,
        disk_gb: 0,
        linux_reserve_gb: 0,
        macos_reserve_gb: 0,
    };
    for entry in entries {
        let slots = u64::from(entry.slots);
        if entry.os == Some(Os::Macos) {
            total.macos_vms += entry.slots;
        }
        total.cpu_nanos += u128::from(entry.slots) * limits::nanos(entry.cpus);
        total.memory_gb += slots * u64::from(entry.memory_gb);
        total.disk_gb += slots * u64::from(entry.disk_gb.unwrap_or(0));
        match entry.os {
            Some(Os::Linux) => total.linux_reserve_gb = LINUX_DISK_RESERVE_GB,
            Some(Os::Macos) => total.macos_reserve_gb = MACOS_DISK_RESERVE_GB,
            None => {}
        }
    }
    total.disk_gb += total.disk_reserve_gb();
    total
}

/// records each limit the pools together exceed, at the budget's path; the macOS
/// limit is fixed by the host, so it is checked even when the budget itself is
/// unusable.
pub fn check_budget(entries: &[Entry], budget: Option<&Budget>, errors: &mut Errors) {
    let path = KeyPath::root().key(Key::Budget);
    let used = usage(entries);
    if used.macos_vms > MAX_MACOS_VMS {
        errors.add(
            path.clone(),
            Kind::MacosVmsOverLimit {
                total: used.macos_vms,
                limit: MAX_MACOS_VMS,
            },
        );
    }
    let Some(budget) = budget else {
        return;
    };
    let budget_nanos = limits::nanos(budget.cpus);
    if used.cpu_nanos > budget_nanos {
        errors.add(
            path.clone(),
            Kind::CpuBudgetExceeded {
                total_nanos: used.cpu_nanos,
                budget_nanos,
            },
        );
    }
    if used.memory_gb > u64::from(budget.memory_gb) {
        errors.add(
            path.clone(),
            Kind::MemoryBudgetExceeded {
                total: used.memory_gb,
                budget: budget.memory_gb,
            },
        );
    }
    if used.disk_gb > u64::from(budget.disk_gb) {
        errors.add(
            path,
            Kind::DiskBudgetExceeded {
                total: used.disk_gb,
                linux_reserve: used.linux_reserve_gb,
                macos_reserve: used.macos_reserve_gb,
                budget: budget.disk_gb,
            },
        );
    }
}
