//! the names of the macOS VMs Marshal makes: the golden macOS VM and a macOS build
//! VM. a job VM's name is the same for both OSs, and M1's `runtime::tart::names`
//! writes and reads it with the Linux forms.

use std::time::SystemTime;

use crate::image::runner::is_version;
use crate::runtime::name::{DIGITS_IN_SECONDS_STAMP, DIGITS_IN_STAMP, stamp};

/// the version of the golden macOS VM this CLI builds and accepts; it goes up
/// whenever the base digest, the provisioning steps or the launcher change.
pub const GOLDEN_MACOS_VM_VERSION: u32 = 1;

const GOLDEN_PREFIX: &str = "marshal-golden-macos-g";
const BUILD_PREFIX: &str = "marshal-build-macos-";

/// what a golden macOS VM's name records, so `start` and `status` read it from
/// Tart's list without booting the VM. A value always writes a name that
/// [`GoldenMacosName::parse`] reads back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoldenMacosName {
    version: u32,
    runner_version: String,
    built: String,
}

impl GoldenMacosName {
    /// the name of a golden macOS VM built at `built`; none when `version` is 0 or
    /// `runner_version` is not three dot-separated numbers.
    #[must_use]
    pub fn at(version: u32, runner_version: &str, built: SystemTime) -> Option<Self> {
        if version == 0 || !is_version(runner_version) {
            return None;
        }
        Some(Self {
            version,
            runner_version: runner_version.to_string(),
            built: second_stamp(built),
        })
    }

    /// the golden VM version.
    #[must_use]
    pub fn version(&self) -> u32 {
        self.version
    }

    /// the runner version the golden VM holds.
    #[must_use]
    pub fn runner_version(&self) -> &str {
        &self.runner_version
    }

    /// the build time as `yyyyMMddHHmmss` in UTC.
    #[must_use]
    pub fn built(&self) -> &str {
        &self.built
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
        if !is_version(runner_version) || !is_digits(built, DIGITS_IN_SECONDS_STAMP) {
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

/// writes `time` as `yyyyMMddHHmmss` in UTC.
fn second_stamp(time: SystemTime) -> String {
    let mut text = stamp(time);
    text.truncate(DIGITS_IN_SECONDS_STAMP);
    text
}

/// whether `text` is exactly `count` ASCII digits.
fn is_digits(text: &str, count: usize) -> bool {
    text.len() == count && text.bytes().all(|byte| byte.is_ascii_digit())
}

/// reads a positive integer written without a leading zero or a sign.
fn positive_integer(text: &str) -> Option<u32> {
    if text.starts_with('0') || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}
