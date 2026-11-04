//! the names of the macOS VMs Marshal makes, and of the Linux VMs it must tell them
//! from: the golden macOS VM, a macOS build VM, and a job VM.

use std::time::SystemTime;

use crate::image::runner::is_version;
use crate::runtime::name::{DIGITS_IN_STAMP, stamp};

/// the version of the golden macOS VM this CLI builds and accepts; it goes up
/// whenever the base digest, the provisioning steps or the launcher change.
pub const GOLDEN_MACOS_VM_VERSION: u32 = 1;

const GOLDEN_PREFIX: &str = "marshal-golden-macos-g";
const BUILD_PREFIX: &str = "marshal-build-macos-";
const JOB_PREFIX: &str = "marshal-job-";

/// the digits of a golden VM's build time, `yyyyMMddHHmmss`.
pub(super) const DIGITS_IN_SECOND_STAMP: usize = 14;

/// what a golden macOS VM's name records, so `start` and `status` read it from
/// Tart's list without booting the VM.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoldenMacosName {
    pub version: u32,
    pub runner_version: String,
    /// the build time as `yyyyMMddHHmmss` in UTC.
    pub built: String,
}

impl GoldenMacosName {
    /// the name of a golden macOS VM built at `built`.
    #[must_use]
    pub fn at(version: u32, runner_version: &str, built: SystemTime) -> Self {
        Self {
            version,
            runner_version: runner_version.to_string(),
            built: second_stamp(built),
        }
    }

    /// reads a golden macOS VM's name: `marshal-golden-macos-g<golden VM
    /// version>-r<runner version>-<yyyyMMddHHmmss>`, with nothing before or after
    /// it. The version is a positive integer without a leading zero, and the runner
    /// version is three dot-separated numbers, as the runner record writes it.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        let rest = name.strip_prefix(GOLDEN_PREFIX)?;
        let (version, rest) = rest.split_once("-r")?;
        let (runner_version, built) = rest.rsplit_once('-')?;
        if !is_version(runner_version) || !is_digits(built, DIGITS_IN_SECOND_STAMP) {
            return None;
        }
        Some(Self {
            version: positive_integer(version)?,
            runner_version: runner_version.to_string(),
            built: built.to_string(),
        })
    }
}

impl std::fmt::Display for GoldenMacosName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{GOLDEN_PREFIX}{}-r{}-{}",
            self.version, self.runner_version, self.built
        )
    }
}

/// the name of a macOS build VM made at `now`.
#[must_use]
pub fn build_name(now: SystemTime) -> String {
    format!("{BUILD_PREFIX}{}", stamp(now))
}

/// whether `name` is a macOS build VM's: the form and 17 ASCII digits, and nothing
/// else.
#[must_use]
pub fn is_build_name(name: &str) -> bool {
    name.strip_prefix(BUILD_PREFIX)
        .is_some_and(|stamp| is_digits(stamp, DIGITS_IN_STAMP))
}

/// the newest golden macOS VM among `names`, by the build time in its name, with the
/// name of the later version, then the greater name, breaking a tie. Names that are
/// not golden macOS VMs' are ignored, whatever their version, so a stale golden VM
/// is still the one found and the caller refuses it.
#[must_use]
pub fn newest_golden<'a>(
    names: impl IntoIterator<Item = &'a str>,
) -> Option<(&'a str, GoldenMacosName)> {
    names
        .into_iter()
        .filter_map(|name| Some((name, GoldenMacosName::parse(name)?)))
        .max_by(|(left_name, left), (right_name, right)| {
            (&left.built, left.version, left_name).cmp(&(&right.built, right.version, right_name))
        })
}

/// a job VM's name, `marshal-job-<pool>-<slot>-<yyyyMMddHHmmssfff>`, split into its
/// parts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JobName<'a> {
    pub pool: &'a str,
    pub slot: &'a str,
    pub stamp: &'a str,
}

impl<'a> JobName<'a> {
    /// reads a job VM's name. A pool name may contain hyphens and end in
    /// `-<digits>`, so the slot is the last number before the 17-digit stamp, which
    /// leaves one reading of any name; the pool is one or more characters other
    /// than a line break.
    #[must_use]
    pub fn parse(name: &'a str) -> Option<Self> {
        let rest = name.strip_prefix(JOB_PREFIX)?;
        let (rest, stamp) = rest.rsplit_once('-')?;
        let (pool, slot) = rest.rsplit_once('-')?;
        if !is_digits(stamp, DIGITS_IN_STAMP)
            || slot.is_empty()
            || !slot.bytes().all(|byte| byte.is_ascii_digit())
            || pool.is_empty()
            || pool.contains('\n')
        {
            return None;
        }
        Some(Self { pool, slot, stamp })
    }
}

/// writes `time` as `yyyyMMddHHmmss` in UTC.
pub(super) fn second_stamp(time: SystemTime) -> String {
    let mut text = stamp(time);
    text.truncate(DIGITS_IN_SECOND_STAMP);
    text
}

/// whether `text` is exactly `count` ASCII digits.
pub(super) fn is_digits(text: &str, count: usize) -> bool {
    text.len() == count && text.bytes().all(|byte| byte.is_ascii_digit())
}

/// reads a positive integer written without a leading zero or a sign.
pub(super) fn positive_integer(text: &str) -> Option<u32> {
    if text.starts_with('0') || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}
