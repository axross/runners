//! the count of macOS VMs against Apple's limit of two, as pure logic over Tart's
//! VM list: which VMs count and why, and which VMs the OS lookup, `tart get`, must
//! be asked about. The rule is the counting table in `docs/operations/cli.md`.

use std::future::Future;

use crate::profile::Os;
use crate::validate::MAX_MACOS_VMS;

use super::linux_forms;
use super::name::{GoldenMacosName, JobName, is_build_name};

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
    /// any other running or suspended VM reported as macOS, such as a golden macOS
    /// VM someone booted by hand.
    ReportedMacos,
    /// a running or suspended VM whose operating system could not be read.
    UnreadableOs,
}

impl Reason {
    /// the reason as the log and `status` word it.
    #[must_use]
    pub fn describe(self) -> &'static str {
        match self {
            Self::ConfiguredMacosJob => "a job VM of a configured macOS pool",
            Self::MacosBuild => "a macOS build VM",
            Self::LeftoverJobReportedMacos => {
                "a leftover job VM of a pool that is no longer configured, reported as macOS"
            }
            Self::ReportedMacos => "a running or suspended VM reported as macOS",
            Self::UnreadableOs => "a running or suspended VM whose operating system is unreadable",
        }
    }
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

/// what the name and the configured pools settle about one VM.
enum Verdict {
    Counts(Reason),
    DoesNotCount,
    /// the name does not decide it, so `tart get` is asked.
    Ask {
        leftover_job: bool,
    },
}

fn judge_by_name(vm: &ListedVm, pools: &[ConfiguredPool]) -> Verdict {
    if is_build_name(&vm.name) {
        return Verdict::Counts(Reason::MacosBuild);
    }
    if let Some(job) = JobName::parse(&vm.name) {
        return match pools.iter().find(|pool| pool.name == job.pool) {
            Some(pool) if pool.os == Os::Macos => Verdict::Counts(Reason::ConfiguredMacosJob),
            Some(_) => Verdict::DoesNotCount,
            None => Verdict::Ask { leftover_job: true },
        };
    }
    let is_linux_form =
        linux_forms::is_golden_name(&vm.name) || linux_forms::is_build_name(&vm.name);
    let is_stopped_golden =
        vm.state == VmState::Stopped && GoldenMacosName::parse(&vm.name).is_some();
    if is_linux_form || is_stopped_golden || !vm.state.is_booted() {
        Verdict::DoesNotCount
    } else {
        Verdict::Ask {
            leftover_job: false,
        }
    }
}

/// what the answer of `tart get` settles about a VM the name left open.
fn judge_by_os(vm: &ListedVm, leftover_job: bool, os: GuestOs) -> Option<Reason> {
    match os {
        GuestOs::Macos if leftover_job => Some(Reason::LeftoverJobReportedMacos),
        GuestOs::Macos => Some(Reason::ReportedMacos),
        GuestOs::Unreadable if vm.state.is_booted() => Some(Reason::UnreadableOs),
        GuestOs::Linux | GuestOs::Unreadable => None,
    }
}

/// counts the macOS VMs in `vms`, a single reading of `tart list`.
///
/// `lookup` is called once for each VM the name and `pools` leave open, and for
/// no other: a Marshal job VM of an unconfigured pool in any state, and any other
/// running or suspended VM that is not a Linux golden or build VM.
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
            Verdict::Ask { leftover_job } => {
                let os = lookup.guest_os(&vm.name).await;
                (judge_by_os(vm, leftover_job, os), leftover_job)
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
