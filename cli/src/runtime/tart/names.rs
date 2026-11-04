//! the names of Marshal's VMs, and the readers that find them again in Tart's
//! listing; a name is read back strictly, so cleanup never matches a VM Marshal
//! does not own.

use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::image::contract::{CONTRACT_VERSION, RUNNER_VERSION_LABEL};
use crate::image::definition::declared_label;
use crate::runtime::name::{
    DIGITS_IN_SECONDS_STAMP, DIGITS_IN_STAMP, format_stamp, parse_stamp, stamp,
};

use super::list::{VmEntry, VmSource};

/// the number the golden VM's name records; it goes up whenever the base digest,
/// the provisioning steps, the launcher or the Docker Engine versions change.
pub const GOLDEN_VM_VERSION: u32 = 1;

const JOB_PREFIX: &str = "marshal-job-";
const GOLDEN_PREFIX: &str = "marshal-golden-linux-";
const BUILD_PREFIX: &str = "marshal-build-linux-";
const MAX_RUNNER_VERSION_BYTES: usize = 32;

/// the next job VM name of a pool's slot, counting slots from 1:
/// `marshal-job-<pool>-<slot>-<yyyyMMddHHmmssfff>`.
#[must_use]
pub fn job_vm_name(pool: &str, slot: u32, now: SystemTime) -> String {
    format!("{JOB_PREFIX}{pool}-{slot}-{}", stamp(now))
}

/// a build VM name: `marshal-build-linux-<yyyyMMddHHmmssfff>`.
#[must_use]
pub fn build_vm_name(now: SystemTime) -> String {
    format!("{BUILD_PREFIX}{}", stamp(now))
}

/// a job VM name read back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobVmName {
    pub pool: String,
    pub slot: u32,
    /// the name's time, in milliseconds since the Unix epoch.
    pub started_millis: u64,
}

/// reads `marshal-job-<pool>-<slot>-<yyyyMMddHHmmssfff>`; a pool name may hold
/// hyphens, because the slot and the time are read from the end.
#[must_use]
pub fn parse_job_vm_name(name: &str) -> Option<JobVmName> {
    let rest = name.strip_prefix(JOB_PREFIX)?;
    let (head, time) = rest.rsplit_once('-')?;
    let (pool, slot) = head.rsplit_once('-')?;
    if pool.is_empty() || slot.is_empty() || !slot.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    Some(JobVmName {
        pool: pool.to_string(),
        slot: slot.parse().ok()?,
        started_millis: millis_of(time)?,
    })
}

/// reads `marshal-build-linux-<yyyyMMddHHmmssfff>` as the time in it, in
/// milliseconds since the Unix epoch.
#[must_use]
pub fn parse_build_vm_name(name: &str) -> Option<u64> {
    millis_of(name.strip_prefix(BUILD_PREFIX)?)
}

/// a golden VM's name,
/// `marshal-golden-linux-g<golden VM version>-c<image contract version>-r<runner version>-<yyyyMMddHHmmss>`,
/// which is all `start`, `status` and a job launch know of the VM without booting it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoldenVmName {
    pub golden_version: u32,
    pub contract_version: u32,
    pub runner_version: String,
    /// the build time, in whole seconds since the Unix epoch.
    pub built_seconds: u64,
}

impl GoldenVmName {
    /// the name a golden VM built at `now` gets: this CLI's golden VM version, image
    /// contract version and runner version. `None` when the image definition
    /// declares no runner version this reader would accept.
    #[must_use]
    pub fn current(now: SystemTime) -> Option<Self> {
        let runner_version = declared_label(RUNNER_VERSION_LABEL)?;
        if !valid_runner_version(runner_version) {
            return None;
        }
        Some(Self {
            golden_version: GOLDEN_VM_VERSION,
            contract_version: CONTRACT_VERSION,
            runner_version: runner_version.to_string(),
            built_seconds: now.duration_since(UNIX_EPOCH).ok()?.as_secs(),
        })
    }

    /// reads a golden VM name; `None` for any other text.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        let rest = name.strip_prefix(GOLDEN_PREFIX)?;
        let (versions, built) = rest.rsplit_once('-')?;
        let mut parts = versions.split('-');
        let (golden, contract, runner) = (parts.next()?, parts.next()?, parts.next()?);
        if parts.next().is_some() {
            return None;
        }
        let runner_version = runner.strip_prefix('r')?;
        if !valid_runner_version(runner_version) {
            return None;
        }
        Some(Self {
            golden_version: canonical_number(golden.strip_prefix('g')?)?,
            contract_version: canonical_number(contract.strip_prefix('c')?)?,
            runner_version: runner_version.to_string(),
            built_seconds: seconds_of(built)?,
        })
    }

    /// whether all three versions in the name are this CLI's.
    #[must_use]
    pub fn matches_this_cli(&self) -> bool {
        self.golden_version == GOLDEN_VM_VERSION
            && self.contract_version == CONTRACT_VERSION
            && declared_label(RUNNER_VERSION_LABEL) == Some(self.runner_version.as_str())
    }
}

impl fmt::Display for GoldenVmName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let built: String = format_stamp(self.built_seconds.saturating_mul(1000))
            .chars()
            .take(DIGITS_IN_SECONDS_STAMP)
            .collect();
        write!(
            f,
            "{GOLDEN_PREFIX}g{}-c{}-r{}-{built}",
            self.golden_version, self.contract_version, self.runner_version
        )
    }
}

/// the milliseconds in a 17-digit stamp.
fn millis_of(text: &str) -> Option<u64> {
    if text.len() != DIGITS_IN_STAMP {
        return None;
    }
    parse_stamp(text)
}

/// the whole seconds in a 14-digit stamp.
fn seconds_of(text: &str) -> Option<u64> {
    if text.len() != DIGITS_IN_SECONDS_STAMP {
        return None;
    }
    Some(parse_stamp(text)? / 1000)
}

/// a positive decimal number without a sign or a leading zero.
fn canonical_number(text: &str) -> Option<u32> {
    let number: u32 = text.parse().ok()?;
    (number > 0 && number.to_string() == text).then_some(number)
}

/// dot-separated decimal numbers, such as `2.337.0`.
fn valid_runner_version(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= MAX_RUNNER_VERSION_BYTES
        && text
            .split('.')
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
}

/// whether `name` is a job VM of one of `pools`, and nothing else: not a golden VM,
/// a build VM, a job VM of another pool, or a VM Marshal does not own.
#[must_use]
pub fn is_job_vm_of_pools<S: AsRef<str>>(name: &str, pools: &[S]) -> bool {
    parse_job_vm_name(name).is_some_and(|job| pools.iter().any(|pool| pool.as_ref() == job.pool))
}

fn local(entries: &[VmEntry]) -> impl Iterator<Item = &VmEntry> {
    entries
        .iter()
        .filter(|entry| entry.source == VmSource::Local)
}

/// the local VMs that are job VMs of one of `pools`.
#[must_use]
pub fn leftover_job_vms<'a, S: AsRef<str>>(
    entries: &'a [VmEntry],
    pools: &[S],
) -> Vec<&'a VmEntry> {
    local(entries)
        .filter(|entry| is_job_vm_of_pools(&entry.name, pools))
        .collect()
}

/// the local VMs that are build VMs.
#[must_use]
pub fn leftover_build_vms(entries: &[VmEntry]) -> Vec<&VmEntry> {
    local(entries)
        .filter(|entry| parse_build_vm_name(&entry.name).is_some())
        .collect()
}

/// the local golden VMs, each with the name read back.
#[must_use]
pub fn golden_vms(entries: &[VmEntry]) -> Vec<(&VmEntry, GoldenVmName)> {
    local(entries)
        .filter_map(|entry| Some((entry, GoldenVmName::parse(&entry.name)?)))
        .collect()
}

/// the golden VM built last, whatever its versions are; a tie goes to the name
/// that sorts last.
#[must_use]
pub fn newest_golden_vm(entries: &[VmEntry]) -> Option<(&VmEntry, GoldenVmName)> {
    golden_vms(entries)
        .into_iter()
        .max_by(|(left_entry, left), (right_entry, right)| {
            (left.built_seconds, &left_entry.name).cmp(&(right.built_seconds, &right_entry.name))
        })
}
