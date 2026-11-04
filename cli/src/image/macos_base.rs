//! the macOS base the golden macOS VM is built from: its digest-pinned reference,
//! its macOS version, its publication date and the digests retired from Tart's
//! cache, and the warnings about its age; see `docs/operations/cli.md`.

use std::time::SystemTime;

use super::check::{age_days, date_seconds};

/// the image the base is pulled from; its digest is the only fixed reference, because
/// the registry tags only `latest`.
pub const BASE_REPOSITORY: &str = "ghcr.io/cirruslabs/macos-tahoe-base";

const FIRST_WARNING_DAYS: u64 = 30;

const SECOND_WARNING_DAYS: u64 = 60;

/// the pinned base.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MacosBase {
    /// the manifest digest, as `sha256:<64 lowercase hexadecimal digits>`.
    pub digest: &'static str,
    /// the macOS version the base ships, when it is known.
    pub macos_version: Option<&'static str>,
    /// the day the base was published, as `YYYY-MM-DD`, read as 00:00 UTC.
    pub published: &'static str,
    /// the digests of earlier pins, the only bases a build removes from Tart's cache.
    pub retired: &'static [&'static str],
}

impl MacosBase {
    /// the reference `tart pull` takes: the repository at the pinned digest.
    #[must_use]
    pub fn reference(&self) -> String {
        format!("{BASE_REPOSITORY}@{}", self.digest)
    }
}

// TODO(#80): the first Mac check records the base's macOS version here, and the
// test that pins this placeholder is updated with it.
const BASE_MACOS_VERSION: Option<&str> = None;

/// the base the CLI pins; a base bump records its digest, macOS version and
/// publication date together and adds the old digest to `retired`.
pub const MACOS_BASE: MacosBase = MacosBase {
    digest: "sha256:4aaa751fa7538902d73c767f27a066fc87e83f183069da4e4b5022863abd4c77",
    macos_version: BASE_MACOS_VERSION,
    published: "2026-10-15",
    retired: &[],
};

/// the warnings about the base's age as of `now`: one from 30 days after its
/// publication, and a stronger one from 60 days, both inclusive.
#[must_use]
pub fn base_age_warnings(base: &MacosBase, now: SystemTime) -> Vec<String> {
    let Some(published) = date_seconds(base.published) else {
        return Vec::new();
    };
    let days = age_days(now, published);
    if days >= SECOND_WARNING_DAYS {
        vec![format!(
            "the pinned macOS base is {days} days old, so the guest's macOS fixes are far behind; \
             bump the base pin and rebuild the golden macOS VM"
        )]
    } else if days >= FIRST_WARNING_DAYS {
        vec![format!(
            "the pinned macOS base is {days} days old; bump the base pin and rebuild the golden \
             macOS VM"
        )]
    } else {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    const SECONDS_PER_DAY: u64 = 86_400;

    fn is_digest(text: &str) -> bool {
        text.strip_prefix("sha256:").is_some_and(|hex| {
            hex.len() == 64
                && hex
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
    }

    /// the moment `days` whole days, and `extra` seconds, after the base was published.
    fn after_publication(days: u64, extra: u64) -> SystemTime {
        let published = date_seconds(MACOS_BASE.published).unwrap();
        SystemTime::UNIX_EPOCH + Duration::from_secs(published + days * SECONDS_PER_DAY + extra)
    }

    #[test]
    fn the_record_holds_a_pinned_reference_and_a_valid_date() {
        assert!(is_digest(MACOS_BASE.digest), "{}", MACOS_BASE.digest);
        assert!(date_seconds(MACOS_BASE.published).is_some());
        assert!(MACOS_BASE.retired.iter().all(|digest| is_digest(digest)));
        assert_eq!(
            MACOS_BASE.reference(),
            "ghcr.io/cirruslabs/macos-tahoe-base@sha256:4aaa751fa7538902d73c767f27a066fc87e83f183069da4e4b5022863abd4c77"
        );
        assert_eq!(MACOS_BASE.published, "2026-10-15");
        assert!(MACOS_BASE.retired.is_empty());
    }

    #[test]
    fn the_base_macos_version_is_still_the_placeholder() {
        assert_eq!(
            MACOS_BASE.macos_version, None,
            "the first Mac check on #80 recorded the base's macOS version: set BASE_MACOS_VERSION \
             and update this test to pin it"
        );
    }

    #[test]
    fn no_warning_before_30_days_and_the_first_at_30() {
        for (days, extra) in [(0, 0), (29, 0), (29, SECONDS_PER_DAY - 1)] {
            assert_eq!(
                base_age_warnings(&MACOS_BASE, after_publication(days, extra)),
                Vec::<String>::new(),
                "{days} days and {extra} seconds"
            );
        }
        let warnings = base_age_warnings(&MACOS_BASE, after_publication(30, 0));
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(
            warnings[0].contains("30 days old")
                && warnings[0].contains("bump the base pin and rebuild"),
            "{warnings:?}"
        );
        assert_eq!(
            base_age_warnings(&MACOS_BASE, after_publication(59, SECONDS_PER_DAY - 1)),
            base_age_warnings(&MACOS_BASE, after_publication(59, 0))
        );
        assert!(
            base_age_warnings(&MACOS_BASE, after_publication(59, 0))[0].contains("59 days old")
        );
    }

    #[test]
    fn the_second_warning_replaces_the_first_at_60_days() {
        let first = base_age_warnings(&MACOS_BASE, after_publication(59, 0));
        let second = base_age_warnings(&MACOS_BASE, after_publication(60, 0));
        assert_eq!(second.len(), 1, "{second:?}");
        assert_ne!(first, second);
        assert!(
            second[0].contains("60 days old")
                && second[0].contains("far behind")
                && second[0].contains("bump the base pin and rebuild"),
            "{second:?}"
        );
        assert!(base_age_warnings(&MACOS_BASE, after_publication(400, 0))[0].contains("400 days"));
    }

    #[test]
    fn a_clock_before_the_publication_gives_no_warning() {
        assert!(base_age_warnings(&MACOS_BASE, SystemTime::UNIX_EPOCH).is_empty());
    }
}
