//! the count of macOS VMs against Apple's limit of two, as pure logic over Tart's
//! VM list: which VMs count and why, and which VMs the OS lookup, `tart get`, must
//! be asked about. The rule is the counting table in `docs/operations/cli.md`.

use std::future::Future;

use crate::profile::Os;
use crate::validate::{MAX_MACOS_VMS, Plan};

use crate::runtime::tart::TartClient;
use crate::runtime::tart::list::{self, VmEntry, VmSource};
use crate::runtime::tart::names::{GoldenVmName, parse_build_vm_name, parse_job_vm_name};

use super::name::{GoldenMacosName, is_build_name};

/// a VM's state as `tart list` reports it; a VM that is stopping is still running.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmState {
    Running,
    Suspended,
    Stopped,
}

impl VmState {
    /// whether the VM occupies memory and a place under macOS's own limit.
    fn is_booted(self) -> bool {
        self != Self::Stopped
    }
}

/// one row of `tart list`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedVm {
    pub name: String,
    pub state: VmState,
}

/// a configured pool, as far as the count needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfiguredPool {
    pub name: String,
    pub os: Os,
}

impl ConfiguredPool {
    /// the pools of `plan`, each with its operating system; a pool without `os` is a
    /// Linux pool.
    #[must_use]
    pub fn of_plan(plan: &Plan) -> Vec<Self> {
        plan.entries
            .iter()
            .map(|entry| Self {
                name: entry.name.clone(),
                os: entry.os.unwrap_or(Os::Linux),
            })
            .collect()
    }
}

/// what `tart get` says a VM's operating system is; a failed call and an answer the
/// reader does not know are both [`GuestOs::Unreadable`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuestOs {
    Macos,
    Linux,
    Unreadable,
}

/// stands in for `tart get`, asked about one VM at a time.
pub trait GuestOsLookup {
    fn guest_os(&self, name: &str) -> impl Future<Output = GuestOs>;
}

/// why a VM counts toward the two.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// a job VM of a configured macOS pool, in any state.
    ConfiguredMacosJob,
    /// a macOS build VM, in any state.
    MacosBuild,
    /// a Marshal job VM of a pool that is no longer configured, reported as macOS.
    LeftoverJobReportedMacos,
    /// a job VM of a configured Linux pool that the lookup reports as macOS, such as
    /// one left from before the pool's `os` changed.
    LinuxPoolJobReportedMacos,
    /// any other running or suspended VM reported as macOS, such as a golden macOS
    /// VM someone booted by hand.
    ReportedMacos,
    /// a VM whose operating system could not be read: any Marshal job VM of an
    /// unconfigured pool, or any other running or suspended VM.
    UnreadableOs,
}

/// a VM that counts, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CountedVm {
    pub name: String,
    pub reason: Reason,
    /// whether the name is a Marshal job VM's of a pool that is not configured.
    pub leftover_job: bool,
}

/// the macOS VMs that count, from one reading of Tart's list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacosVmCount {
    counted: Vec<CountedVm>,
}

impl MacosVmCount {
    /// how many macOS VMs count.
    #[must_use]
    pub fn total(&self) -> usize {
        self.counted.len()
    }

    /// each VM that counts, in the order Tart listed them.
    #[must_use]
    pub fn counted(&self) -> &[CountedVm] {
        &self.counted
    }

    /// whether another macOS VM may be created without exceeding the limit.
    #[must_use]
    pub fn has_free_place(&self) -> bool {
        self.total() < MAX_MACOS_VMS as usize
    }

    /// the counted job VMs of pools that are no longer configured, which `start`
    /// does not delete, so the log and `status` name them for the maintainer.
    #[must_use]
    pub fn leftover_jobs(&self) -> Vec<&str> {
        self.counted
            .iter()
            .filter(|vm| vm.leftover_job)
            .map(|vm| vm.name.as_str())
            .collect()
    }
}

/// which VMs the name leaves open, which decides what an answer of the lookup means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Open {
    /// a Marshal job VM of a pool that is not configured.
    LeftoverJob,
    /// a job VM of a configured Linux pool: Linux unless the lookup says macOS.
    LinuxPoolJob,
    /// any other VM that is running or suspended.
    Foreign,
}

/// what the name and the configured pools settle about one VM.
enum Verdict {
    Counts(Reason),
    DoesNotCount,
    /// the name does not decide it, so `tart get` is asked.
    Ask(Open),
}

fn judge_by_name(vm: &ListedVm, pools: &[ConfiguredPool]) -> Verdict {
    if is_build_name(&vm.name) {
        return Verdict::Counts(Reason::MacosBuild);
    }
    if let Some(job) = parse_job_vm_name(&vm.name) {
        return match pools.iter().find(|pool| pool.name == job.pool) {
            Some(pool) if pool.os == Os::Macos => Verdict::Counts(Reason::ConfiguredMacosJob),
            Some(_) => Verdict::Ask(Open::LinuxPoolJob),
            None => Verdict::Ask(Open::LeftoverJob),
        };
    }
    let is_linux_form =
        GoldenVmName::parse(&vm.name).is_some() || parse_build_vm_name(&vm.name).is_some();
    let is_stopped_golden =
        vm.state == VmState::Stopped && GoldenMacosName::parse(&vm.name).is_some();
    if is_linux_form || is_stopped_golden || !vm.state.is_booted() {
        Verdict::DoesNotCount
    } else {
        Verdict::Ask(Open::Foreign)
    }
}

/// what the answer of `tart get` settles about a VM the name left open. Only an
/// explicit macOS answer counts a job VM of a configured Linux pool, so a lookup
/// that fails never blocks macOS jobs because of Linux VMs.
fn judge_by_os(vm: &ListedVm, open: Open, os: GuestOs) -> Option<Reason> {
    match (os, open) {
        (GuestOs::Macos, Open::LeftoverJob) => Some(Reason::LeftoverJobReportedMacos),
        (GuestOs::Macos, Open::LinuxPoolJob) => Some(Reason::LinuxPoolJobReportedMacos),
        (GuestOs::Macos, Open::Foreign) => Some(Reason::ReportedMacos),
        (GuestOs::Unreadable, Open::LeftoverJob) => Some(Reason::UnreadableOs),
        (GuestOs::Unreadable, Open::Foreign) if vm.state.is_booted() => Some(Reason::UnreadableOs),
        (GuestOs::Linux, _) | (GuestOs::Unreadable, _) => None,
    }
}

/// the local VMs of a `tart list` reading, as the count reads them; Tart's cached
/// images are stored, not booted, so they never count.
#[must_use]
pub fn listed_vms(entries: &[VmEntry]) -> Vec<ListedVm> {
    entries
        .iter()
        .filter(|entry| entry.source == VmSource::Local)
        .map(|entry| ListedVm {
            name: entry.name.clone(),
            state: match entry.state {
                list::VmState::Running => VmState::Running,
                list::VmState::Suspended => VmState::Suspended,
                list::VmState::Stopped => VmState::Stopped,
            },
        })
        .collect()
}

/// reads a VM's operating system from `tart get`, whose `OS` field is `darwin` for a
/// macOS VM.
impl GuestOsLookup for TartClient {
    async fn guest_os(&self, name: &str) -> GuestOs {
        match self.os_of(name).await.as_deref() {
            Some("darwin") => GuestOs::Macos,
            Some("linux") => GuestOs::Linux,
            _ => GuestOs::Unreadable,
        }
    }
}

/// counts the macOS VMs in `vms`, a single reading of `tart list`.
///
/// `lookup` is called once for each VM the name and `pools` leave open, and for
/// no other: a Marshal job VM in any state whose pool is not configured as a macOS
/// pool, and any other running or suspended VM that is not a Linux golden or build
/// VM.
pub async fn count_macos_vms(
    vms: &[ListedVm],
    pools: &[ConfiguredPool],
    lookup: &impl GuestOsLookup,
) -> MacosVmCount {
    let mut counted = Vec::new();
    for vm in vms {
        let (reason, leftover_job) = match judge_by_name(vm, pools) {
            Verdict::Counts(reason) => (Some(reason), false),
            Verdict::DoesNotCount => (None, false),
            Verdict::Ask(open) => {
                let os = lookup.guest_os(&vm.name).await;
                (judge_by_os(vm, open, os), open == Open::LeftoverJob)
            }
        };
        if let Some(reason) = reason {
            counted.push(CountedVm {
                name: vm.name.clone(),
                reason,
                leftover_job,
            });
        }
    }
    MacosVmCount { counted }
}
