//! the check of the golden VM from `tart list` alone: its name records its golden VM
//! version, image contract version, runner version and build time, so nothing boots.

use crate::image::check::{GoldenCheck, ImageCheck, ImageLabels};
use crate::image::contract::{CONTRACT_VERSION, RUNNER_RELEASED_LABEL, RUNNER_VERSION_LABEL};
use crate::image::definition::declared_label;

use super::list::VmEntry;
use super::names::{GOLDEN_VM_VERSION, GoldenVmName, newest_golden_vm};

/// what the name of a golden VM tells of its image, as the labels of an image would:
/// the contract and build time it records, and the runner version with its release
/// day when that version is the one this CLI's image definition declares.
fn labels_of(name: &GoldenVmName) -> ImageLabels {
    let declared = declared_label(RUNNER_VERSION_LABEL);
    let released = (declared == Some(name.runner_version.as_str()))
        .then(|| declared_label(RUNNER_RELEASED_LABEL))
        .flatten();
    ImageLabels {
        contract: Some(name.contract_version.to_string()),
        built: Some(name.built_seconds.to_string()),
        runner_version: Some(name.runner_version.clone()),
        runner_released: released.map(str::to_string),
    }
}

fn runs_here(name: &GoldenVmName) -> bool {
    name.golden_version == GOLDEN_VM_VERSION && name.contract_version == CONTRACT_VERSION
}

/// checks the newest golden VM among `entries`, the one built last whatever its
/// versions: it matches when its golden VM version and image contract version are
/// this CLI's, and is mismatched otherwise; no golden VM at all is missing. A runner
/// version that differs is not a mismatch, as an image built with another runner is
/// not one; the freshness warnings read it.
#[must_use]
pub fn check_golden(entries: &[VmEntry]) -> ImageCheck {
    let Some((entry, name)) = newest_golden_vm(entries) else {
        return ImageCheck::Golden(GoldenCheck::Missing);
    };
    let labels = labels_of(&name);
    let vm = entry.name.clone();
    ImageCheck::Golden(if runs_here(&name) {
        GoldenCheck::Matching { name: vm, labels }
    } else {
        GoldenCheck::Mismatched {
            name: vm,
            golden_version: name.golden_version,
            contract_version: name.contract_version,
            labels,
        }
    })
}

/// the name of the golden VM a job VM is cloned from: the newest, when it runs here.
#[must_use]
pub fn clone_source(entries: &[VmEntry]) -> Option<&str> {
    let (entry, name) = newest_golden_vm(entries)?;
    runs_here(&name).then_some(entry.name.as_str())
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use crate::runtime::tart::list::{VmSource, VmState};

    use super::*;

    fn entry(name: &str) -> VmEntry {
        VmEntry {
            name: name.to_string(),
            source: VmSource::Local,
            state: VmState::Stopped,
        }
    }

    fn golden(golden: u32, contract: u32, built: u64) -> String {
        GoldenVmName {
            golden_version: golden,
            contract_version: contract,
            runner_version: "2.337.0".to_string(),
            built_seconds: built,
        }
        .to_string()
    }

    #[test]
    fn no_golden_vm_is_missing() {
        assert_eq!(
            check_golden(&[entry("marshal-job-a-1-20240305060708009")]),
            ImageCheck::Golden(GoldenCheck::Missing)
        );
        assert_eq!(clone_source(&[]), None);
    }

    #[test]
    fn the_newest_golden_vm_decides_and_is_the_clone_source() {
        let old = golden(GOLDEN_VM_VERSION + 1, CONTRACT_VERSION, 1_000);
        let new = golden(GOLDEN_VM_VERSION, CONTRACT_VERSION, 2_000);
        let entries = [entry(&old), entry(&new)];
        assert!(matches!(
            check_golden(&entries),
            ImageCheck::Golden(GoldenCheck::Matching { .. })
        ));
        assert_eq!(clone_source(&entries), Some(new.as_str()));
        let entries = [entry(&new), entry(&golden(GOLDEN_VM_VERSION + 1, 1, 3_000))];
        assert!(matches!(
            check_golden(&entries),
            ImageCheck::Golden(GoldenCheck::Mismatched { .. })
        ));
        assert_eq!(clone_source(&entries), None);
    }

    #[test]
    fn another_golden_vm_version_or_image_contract_is_mismatched_and_says_so() {
        for (version, contract) in [
            (GOLDEN_VM_VERSION + 1, CONTRACT_VERSION),
            (GOLDEN_VM_VERSION, CONTRACT_VERSION + 1),
        ] {
            let name = golden(version, contract, 1_000);
            let check = check_golden(&[entry(&name)]);
            assert_eq!(
                check.refusal("actions-runner:local").unwrap(),
                format!(
                    "the golden VM {name} has golden VM version {version} and image contract {contract}, \
                     but this CLI needs golden VM version {GOLDEN_VM_VERSION} and image contract {CONTRACT_VERSION}"
                )
            );
        }
    }

    #[test]
    fn a_missing_golden_vm_is_refused_like_a_missing_image() {
        assert_eq!(
            check_golden(&[]).refusal("x").unwrap(),
            format!(
                "the golden VM is missing; this CLI needs one with golden VM version {GOLDEN_VM_VERSION} and image contract {CONTRACT_VERSION}"
            )
        );
    }

    #[test]
    fn a_matching_golden_vm_is_old_by_its_build_time_and_the_warning_names_it() {
        let built = 1_700_000_000;
        let name = golden(GOLDEN_VM_VERSION, CONTRACT_VERSION, built);
        let check = check_golden(&[entry(&name)]);
        assert_eq!(check.refusal("x"), None);
        let now = UNIX_EPOCH + Duration::from_secs(built + 8 * 86_400);
        let warnings = crate::image::check::freshness_warnings(&check, now);
        assert!(
            warnings.contains(&"the golden VM is 8 days old and should be rebuilt".to_string()),
            "{warnings:?}"
        );
        let now = UNIX_EPOCH + Duration::from_secs(built + 22 * 86_400);
        let warnings = crate::image::check::freshness_warnings(&check, now);
        assert!(
            warnings.contains(
                &"the golden VM's operating-system packages are 22 days old; build it again"
                    .to_string()
            ),
            "{warnings:?}"
        );
    }
}
