//! the Linux VM name forms that M1's Tart runtime (#74, plan revision 1) defines,
//! written here as a stand-in so the macOS names and the VM count can tell a macOS
//! VM from a Linux one before that code is on `main`. The integration with M1's
//! runtime replaces this module with M1's own forms; nothing else here is M1's.

use std::time::SystemTime;

use crate::image::runner::is_version;

use crate::runtime::name::{DIGITS_IN_STAMP, stamp};

use super::name::{DIGITS_IN_SECOND_STAMP, is_digits, positive_integer, second_stamp};

const GOLDEN_PREFIX: &str = "marshal-golden-linux-g";
const BUILD_PREFIX: &str = "marshal-build-linux-";

/// the name of a Linux golden VM, `marshal-golden-linux-g<golden VM
/// version>-c<image contract version>-r<runner version>-<yyyyMMddHHmmss>`.
#[must_use]
pub fn golden_name(
    golden_version: u32,
    contract_version: u32,
    runner_version: &str,
    built: SystemTime,
) -> String {
    format!(
        "{GOLDEN_PREFIX}{golden_version}-c{contract_version}-r{runner_version}-{}",
        second_stamp(built)
    )
}

/// the name of a Linux build VM, `marshal-build-linux-<yyyyMMddHHmmssfff>`.
#[must_use]
pub fn build_name(now: SystemTime) -> String {
    format!("{BUILD_PREFIX}{}", stamp(now))
}

/// whether `name` is a Linux golden VM's, with nothing before or after the form.
#[must_use]
pub fn is_golden_name(name: &str) -> bool {
    let Some(rest) = name.strip_prefix(GOLDEN_PREFIX) else {
        return false;
    };
    let Some((golden_version, rest)) = rest.split_once("-c") else {
        return false;
    };
    let Some((contract_version, rest)) = rest.split_once("-r") else {
        return false;
    };
    let Some((runner_version, built)) = rest.rsplit_once('-') else {
        return false;
    };
    positive_integer(golden_version).is_some()
        && positive_integer(contract_version).is_some()
        && is_version(runner_version)
        && is_digits(built, DIGITS_IN_SECOND_STAMP)
}

/// whether `name` is a Linux build VM's, with nothing before or after the form.
#[must_use]
pub fn is_build_name(name: &str) -> bool {
    name.strip_prefix(BUILD_PREFIX)
        .is_some_and(|stamp| is_digits(stamp, DIGITS_IN_STAMP))
}

/// the newest Linux golden VM among `names` by the build time in its name, with the
/// greater name breaking a tie; names that are not Linux golden VMs' are ignored.
#[must_use]
pub fn newest_golden<'a>(names: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    names
        .into_iter()
        .filter(|name| is_golden_name(name))
        .max_by_key(|name| (name.rsplit('-').next().unwrap_or_default(), *name))
}
