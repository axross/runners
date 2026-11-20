//! what the Tart backend does differently for a macOS pool, as pure logic: which
//! golden macOS VM a job VM is cloned from, how Tart's VM-limit refusal is told
//! apart from another boot failure, and what the probe of a booted job VM must show;
//! `docs/operations/cli.md` describes the macOS slot cycle.

use std::time::Duration;

use crate::macos::name::{GOLDEN_MACOS_VM_VERSION, GoldenMacosName, newest_golden};

use super::list::{VmEntry, VmSource};

/// how long a macOS job VM's guest agent has to answer after `tart run`.
pub const MACOS_BOOT_DEADLINE: Duration = Duration::from_secs(180);

/// how often a slot waiting for a place under the limit counts again, and a failed
/// deletion is tried again.
pub const PLACE_POLL: Duration = Duration::from_secs(5);

/// the text Tart 2.41.1 prints when Virtualization.framework refuses another VM.
pub const VM_LIMIT_ERROR: &str = "The number of VMs exceeds the system limit";

const BYTES_PER_GB: u64 = 1_000_000_000;

/// the newest local golden macOS VM in a listing, with its parsed name, whatever its
/// golden VM version.
#[must_use]
pub fn newest_local_golden(entries: &[VmEntry]) -> Option<(String, GoldenMacosName)> {
    let names = entries
        .iter()
        .filter(|entry| entry.source == VmSource::Local)
        .map(|entry| entry.name.as_str());
    newest_golden(names).map(|(name, parsed)| (name.to_string(), parsed))
}

/// the golden macOS VM a job VM is cloned from: the newest local one, refused with
/// the Linux golden VM's messages when there is none or its golden VM version is not
/// this CLI's.
pub fn macos_clone_source(entries: &[VmEntry]) -> Result<(String, GoldenMacosName), String> {
    let Some((name, parsed)) = newest_local_golden(entries) else {
        return Err(format!(
            "the golden macOS VM is missing; this CLI needs one with golden VM version \
             {GOLDEN_MACOS_VM_VERSION}"
        ));
    };
    if parsed.version() != GOLDEN_MACOS_VM_VERSION {
        return Err(format!(
            "the golden macOS VM {name} has golden VM version {}, but this CLI needs golden \
             VM version {GOLDEN_MACOS_VM_VERSION}",
            parsed.version()
        ));
    }
    Ok((name, parsed))
}

/// whether the output of a `tart run` that ended is Tart's VM-limit refusal.
#[must_use]
pub fn is_vm_limit_refusal(output: &str) -> bool {
    output.contains(VM_LIMIT_ERROR)
}

/// what a slot logs when macOS refuses another VM.
#[must_use]
pub fn vm_limit_message(error: &str) -> String {
    format!(
        "macOS refused another macOS VM: {error}; if no other macOS VM is running, restart \
         the Mac to release the place"
    )
}

/// what the probe of a booted macOS job VM printed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Probe {
    pub launcher: bool,
    pub runner: String,
    pub disk_bytes: u64,
    pub partition_bytes: u64,
}

/// reads the probe's four lines; `None` for any other output.
#[must_use]
pub fn read_probe(output: &str) -> Option<Probe> {
    let mut lines = output.lines();
    let mut field = |key: &str| lines.next()?.strip_prefix(key)?.strip_prefix(' ');
    let launcher = match field("launcher")? {
        "yes" => true,
        "no" => false,
        _ => return None,
    };
    let runner = field("runner")?.to_string();
    let disk_bytes = field("disk")?.parse().ok()?;
    let partition_bytes = field("partitions")?.parse().ok()?;
    Some(Probe {
        launcher,
        runner,
        disk_bytes,
        partition_bytes,
    })
}

/// what a probe says about the job VM.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Ready,
    /// the disk has not grown to the pool's size yet; the guest agent grows it soon
    /// after boot, so the probe is asked again.
    NotGrownYet,
    Refused(String),
}

/// judges a probe against the pool's disk and the runner the golden VM's name
/// records: the launcher is there, the runner is that version, and the disk is
/// within 1 GB of `disk_gb` with less than 1 GB of it outside any partition.
#[must_use]
pub fn judge_probe(probe: &Probe, disk_gb: u32, runner: &str) -> Verdict {
    if !probe.launcher {
        return Verdict::Refused("the job VM has no launcher".to_string());
    }
    if probe.runner != runner {
        return Verdict::Refused(format!(
            "the job VM's runner is {}, not the {runner} its golden VM's name records",
            probe.runner
        ));
    }
    let wanted = u64::from(disk_gb) * BYTES_PER_GB;
    if probe.disk_bytes.abs_diff(wanted) >= BYTES_PER_GB {
        return Verdict::Refused(format!(
            "the job VM's disk is {} bytes, not within 1 GB of the pool's {disk_gb} GB",
            probe.disk_bytes
        ));
    }
    if probe.disk_bytes.saturating_sub(probe.partition_bytes) >= BYTES_PER_GB {
        return Verdict::NotGrownYet;
    }
    Verdict::Ready
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::tart::list::VmState;

    fn local(name: &str) -> VmEntry {
        VmEntry {
            name: name.to_string(),
            source: VmSource::Local,
            state: VmState::Stopped,
        }
    }

    #[test]
    fn the_newest_golden_macos_vm_of_this_version_is_the_source() {
        let entries = [
            local("marshal-golden-macos-g1-r2.336.0-20260101000000"),
            local("marshal-golden-macos-g1-r2.337.0-20261101000000"),
            local("marshal-golden-linux-g2-c2-r2.337.0-20261112070106"),
        ];
        assert_eq!(
            macos_clone_source(&entries).unwrap().0,
            "marshal-golden-macos-g1-r2.337.0-20261101000000"
        );
        let newer_version = [local("marshal-golden-macos-g2-r2.337.0-20261101000000")];
        assert!(
            macos_clone_source(&newer_version)
                .unwrap_err()
                .contains("has golden VM version 2, but this CLI needs golden VM version 1")
        );
        let oci = [VmEntry {
            source: VmSource::Oci,
            ..local("marshal-golden-macos-g1-r2.337.0-20261101000000")
        }];
        assert!(
            macos_clone_source(&oci)
                .unwrap_err()
                .starts_with("the golden macOS VM is missing")
        );
    }

    #[test]
    fn only_tarts_vm_limit_text_is_a_limit_refusal() {
        assert!(is_vm_limit_refusal(
            "The number of VMs exceeds the system limit (other running VMs: b, a)"
        ));
        assert!(!is_vm_limit_refusal("Unable to create an additional VM."));
        assert_eq!(
            vm_limit_message("X"),
            "macOS refused another macOS VM: X; if no other macOS VM is running, restart the \
             Mac to release the place"
        );
    }

    fn probe(disk: u64, partitions: u64) -> Probe {
        Probe {
            launcher: true,
            runner: "2.337.0".to_string(),
            disk_bytes: disk,
            partition_bytes: partitions,
        }
    }

    #[test]
    fn the_probe_reads_its_four_lines_only() {
        assert_eq!(
            read_probe("launcher yes\nrunner 2.337.0\ndisk 80000000000\npartitions 80000000000\n"),
            Some(probe(80_000_000_000, 80_000_000_000))
        );
        assert_eq!(
            read_probe("launcher maybe\nrunner x\ndisk 1\npartitions 1\n"),
            None
        );
        assert_eq!(read_probe("launcher yes\nrunner x\ndisk 1\n"), None);
        assert_eq!(
            read_probe("runner x\nlauncher yes\ndisk 1\npartitions 1\n"),
            None
        );
    }

    #[test]
    fn a_grown_disk_of_the_pools_size_is_ready_and_an_ungrown_one_is_asked_again() {
        // the first Mac check: an 80 GB disk with a 524.3 MB and a 79.5 GB partition
        let grown = probe(80_000_000_000, 524_300_000 + 79_475_671_040);
        assert_eq!(judge_probe(&grown, 80, "2.337.0"), Verdict::Ready);
        let ungrown = probe(80_000_000_000, 50_000_000_000);
        assert_eq!(judge_probe(&ungrown, 80, "2.337.0"), Verdict::NotGrownYet);
        let at_default = probe(50_000_000_000, 49_800_000_000);
        assert_eq!(judge_probe(&at_default, 50, "2.337.0"), Verdict::Ready);
    }

    #[test]
    fn a_wrong_disk_size_runner_or_missing_launcher_is_refused() {
        let unresized = probe(50_000_000_000, 49_800_000_000);
        assert!(matches!(
            judge_probe(&unresized, 80, "2.337.0"),
            Verdict::Refused(_)
        ));
        let edge = probe(81_000_000_000, 81_000_000_000);
        assert!(matches!(
            judge_probe(&edge, 80, "2.337.0"),
            Verdict::Refused(_)
        ));
        let close = probe(80_999_999_999, 80_999_999_999);
        assert_eq!(judge_probe(&close, 80, "2.337.0"), Verdict::Ready);
        let old_runner = probe(80_000_000_000, 80_000_000_000);
        assert!(matches!(
            judge_probe(&old_runner, 80, "2.338.0"),
            Verdict::Refused(_)
        ));
        let no_launcher = Probe {
            launcher: false,
            ..probe(80_000_000_000, 80_000_000_000)
        };
        assert_eq!(
            judge_probe(&no_launcher, 80, "2.337.0"),
            Verdict::Refused("the job VM has no launcher".to_string())
        );
    }
}
