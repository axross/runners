//! the versions of Tart and Softnet this marshal was tested with, the readers of what
//! each prints for `--version`, and the sorting of a version into tested, a newer
//! patch, or refused.

use std::fmt;

/// three dot-separated numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl Version {
    #[must_use]
    pub const fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    /// exactly three numbers of ASCII digits with no sign, space or leading zero,
    /// separated by dots.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let mut parts = text.split('.');
        let (major, minor, patch) = (parts.next()?, parts.next()?, parts.next()?);
        if parts.next().is_some() {
            return None;
        }
        Some(Self::new(number(major)?, number(minor)?, number(patch)?))
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

fn number(text: &str) -> Option<u32> {
    let canonical = text == "0" || !text.starts_with('0');
    if !canonical || text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

/// the Tart version the host runs are tested with; `docs/operations/cli.md` and the
/// host document record it too.
pub const TESTED_TART: Version = Version::new(2, 41, 1);

/// the Softnet version the host runs are tested with.
pub const TESTED_SOFTNET: Version = Version::new(0, 25, 0);

const SOFTNET_OUTPUT_PREFIX: &str = "softnet ";
const COMMIT_DIGITS: std::ops::RangeInclusive<usize> = 7..=40;

/// what `softnet --version` prints: the version and the short commit that Softnet's
/// release build appends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SoftnetVersion {
    pub number: Version,
    pub commit: String,
}

impl fmt::Display for SoftnetVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}-{}", self.number, self.commit)
    }
}

fn without_final_newline(output: &str) -> &str {
    output.strip_suffix('\n').unwrap_or(output)
}

/// the version `tart --version` printed: exactly three dot-separated numbers, and
/// one final newline.
#[must_use]
pub fn read_tart_output(output: &str) -> Option<Version> {
    Version::parse(without_final_newline(output))
}

/// the version `softnet --version` printed: `softnet <version>-<short commit>` and
/// one final newline, the form clap prints for Softnet's release builds. A build
/// made without Softnet's release variable prints `softnet unknown-unknown`, which
/// this refuses.
#[must_use]
pub fn read_softnet_output(output: &str) -> Option<SoftnetVersion> {
    let rest = without_final_newline(output).strip_prefix(SOFTNET_OUTPUT_PREFIX)?;
    let (number, commit) = rest.split_once('-')?;
    let hexadecimal = commit
        .bytes()
        .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'));
    if !hexadecimal || !COMMIT_DIGITS.contains(&commit.len()) {
        return None;
    }
    Some(SoftnetVersion {
        number: Version::parse(number)?,
        commit: commit.to_string(),
    })
}

/// how a version that was read compares with the tested one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Standing {
    Tested,
    /// a later patch of the tested minor version.
    NewerPatch,
    /// an earlier patch of the tested minor version.
    OlderPatch,
    /// another minor or major version, earlier or later.
    OtherRelease,
}

impl Standing {
    #[must_use]
    pub fn runs(self) -> bool {
        matches!(self, Self::Tested | Self::NewerPatch)
    }
}

#[must_use]
pub fn standing(found: Version, tested: Version) -> Standing {
    if (found.major, found.minor) != (tested.major, tested.minor) {
        return Standing::OtherRelease;
    }
    match found.patch.cmp(&tested.patch) {
        std::cmp::Ordering::Equal => Standing::Tested,
        std::cmp::Ordering::Greater => Standing::NewerPatch,
        std::cmp::Ordering::Less => Standing::OlderPatch,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tested_versions_are_tart_2_41_1_and_softnet_0_25_0() {
        assert_eq!(TESTED_TART.to_string(), "2.41.1");
        assert_eq!(TESTED_SOFTNET.to_string(), "0.25.0");
    }

    #[test]
    fn tart_prints_exactly_three_numbers_and_may_end_in_one_newline() {
        assert_eq!(read_tart_output("2.41.1\n"), Some(Version::new(2, 41, 1)));
        assert_eq!(read_tart_output("2.41.1"), Some(Version::new(2, 41, 1)));
        assert_eq!(read_tart_output("0.0.0\n"), Some(Version::new(0, 0, 0)));
        assert_eq!(
            read_tart_output("10.200.3000\n"),
            Some(Version::new(10, 200, 3000))
        );
    }

    #[test]
    fn tart_output_of_any_other_form_is_not_a_version() {
        for output in [
            "",
            "\n",
            "2.41\n",
            "2\n",
            "2.41.1.0\n",
            "2.41.1.\n",
            ".2.41.1\n",
            "2..1\n",
            "2.41.1-beta\n",
            "2.41.1+build\n",
            "v2.41.1\n",
            "tart 2.41.1\n",
            "tart version 2.41.1\n",
            "2.41.1 \n",
            " 2.41.1\n",
            "2.41.1\n\n",
            "2.41.1\r\n",
            "2.41.1\n2.41.1\n",
            "2.41.01\n",
            "02.41.1\n",
            "2.41.+1\n",
            "2.-41.1\n",
            "2.41.x\n",
            "2.41.4294967296\n",
            "SNAPSHOT\n",
            "２.41.1\n",
        ] {
            assert_eq!(read_tart_output(output), None, "{output:?}");
        }
    }

    #[test]
    fn softnet_prints_its_name_the_version_and_the_short_commit() {
        let read = read_softnet_output("softnet 0.25.0-0f1e2d3\n").unwrap();
        assert_eq!(read.number, Version::new(0, 25, 0));
        assert_eq!(read.commit, "0f1e2d3");
        assert_eq!(read.to_string(), "0.25.0-0f1e2d3");
        assert!(read_softnet_output("softnet 0.25.0-0f1e2d3").is_some());
        let longest = format!("softnet 0.25.0-{}\n", "a".repeat(40));
        assert!(read_softnet_output(&longest).is_some());
    }

    #[test]
    fn softnet_output_of_any_other_form_is_not_a_version() {
        let too_long = format!("softnet 0.25.0-{}\n", "a".repeat(41));
        for output in [
            "",
            "\n",
            "softnet\n",
            "softnet 0.25.0\n",
            "softnet 0.25.0-\n",
            "0.25.0-0f1e2d3\n",
            "softnet  0.25.0-0f1e2d3\n",
            "Softnet 0.25.0-0f1e2d3\n",
            "softnet v0.25.0-0f1e2d3\n",
            "softnet 0.25-0f1e2d3\n",
            "softnet 0.25.0.1-0f1e2d3\n",
            "softnet 0.25.0-0f1e2d3-dirty\n",
            "softnet 0.25.0-0f1e2d\n",
            "softnet 0.25.0-0F1E2D3\n",
            "softnet 0.25.0-0g1e2d3\n",
            "softnet 0.25.0-0f1e2d3 \n",
            "softnet 0.25.0-0f1e2d3\r\n",
            "softnet 0.25.0-0f1e2d3\n\n",
            "softnet 0.25.0-0f1e2d3\nsoftnet 0.25.0-0f1e2d3\n",
            "softnet unknown-unknown\n",
            "softnet 0.25.0-rc1\n",
            "softnet 0.025.0-0f1e2d3\n",
            too_long.as_str(),
        ] {
            assert_eq!(read_softnet_output(output), None, "{output:?}");
        }
    }

    #[test]
    fn the_tested_version_and_newer_patches_of_its_minor_version_run() {
        let tested = Version::new(2, 41, 1);
        assert_eq!(standing(tested, tested), Standing::Tested);
        assert_eq!(
            standing(Version::new(2, 41, 2), tested),
            Standing::NewerPatch
        );
        assert_eq!(
            standing(Version::new(2, 41, 4_000), tested),
            Standing::NewerPatch
        );
        assert!(Standing::Tested.runs());
        assert!(Standing::NewerPatch.runs());
    }

    #[test]
    fn an_older_patch_and_another_minor_or_major_version_are_refused() {
        let tested = Version::new(2, 41, 1);
        assert_eq!(
            standing(Version::new(2, 41, 0), tested),
            Standing::OlderPatch
        );
        for other in [
            Version::new(2, 40, 9),
            Version::new(2, 42, 0),
            Version::new(2, 42, 1),
            Version::new(1, 41, 1),
            Version::new(3, 41, 1),
            Version::new(3, 0, 0),
        ] {
            assert_eq!(standing(other, tested), Standing::OtherRelease, "{other}");
        }
        assert!(!Standing::OlderPatch.runs());
        assert!(!Standing::OtherRelease.runs());
    }
}
