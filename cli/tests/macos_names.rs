//! the macOS VM name forms: formatting, strict parsing, the newest golden macOS VM,
//! and that no macOS form is read as a Linux one or the reverse.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use marshal_cli::macos::name::{
    GOLDEN_MACOS_VM_VERSION, GoldenMacosName, build_name, is_build_name, newest_golden,
};
use marshal_cli::runtime::tart::list::{VmEntry, VmSource, VmState};
use marshal_cli::runtime::tart::names::{
    GoldenVmName, build_vm_name, newest_golden_vm, parse_build_vm_name, parse_job_vm_name,
};

/// 2024-03-05 06:07:08.009 UTC.
fn pinned() -> SystemTime {
    UNIX_EPOCH + Duration::from_millis(1_709_618_828_009)
}

fn later(seconds: u64) -> SystemTime {
    pinned() + Duration::from_secs(seconds)
}

fn golden(version: u32, runner: &str, built: SystemTime) -> GoldenMacosName {
    GoldenMacosName::at(version, runner, built).expect("a valid golden name")
}

const GOLDEN: &str = "marshal-golden-macos-g1-r2.337.0-20240305060708";
const BUILD: &str = "marshal-build-macos-20240305060708009";

#[test]
fn the_golden_macos_vm_version_starts_at_one() {
    assert_eq!(GOLDEN_MACOS_VM_VERSION, 1);
}

#[test]
fn a_golden_macos_name_is_formatted_and_read_back() {
    let name = golden(GOLDEN_MACOS_VM_VERSION, "2.337.0", pinned());
    assert_eq!(name.to_string(), GOLDEN);
    assert_eq!(GoldenMacosName::parse(GOLDEN), Some(name));
    let parsed = GoldenMacosName::parse("marshal-golden-macos-g12-r10.0.15-20991231235959")
        .expect("a two-digit version");
    assert_eq!(parsed.version(), 12);
    assert_eq!(parsed.runner_version(), "10.0.15");
    assert_eq!(parsed.built(), "20991231235959");
}

#[test]
fn a_golden_macos_name_is_only_made_from_a_version_and_a_runner_that_read_back() {
    assert_eq!(GoldenMacosName::at(0, "2.337.0", pinned()), None);
    for runner in [
        "",
        "2.337",
        "2.337.0.1",
        "2.037.0",
        "v2.337.0",
        "2.337.x",
        "2.337.0-rc1",
    ] {
        assert_eq!(GoldenMacosName::at(1, runner, pinned()), None, "{runner}");
    }
    for (version, runner) in [(1, "2.337.0"), (u32::MAX, "999999999.0.10")] {
        let made = golden(version, runner, pinned());
        assert_eq!(GoldenMacosName::parse(&made.to_string()), Some(made));
    }
}

#[test]
fn a_golden_macos_name_is_read_strictly() {
    for (what, name) in [
        ("a prefix", format!("x{GOLDEN}")),
        ("a suffix", format!("{GOLDEN}0")),
        ("a trailing hyphen", format!("{GOLDEN}-")),
        (
            "no version",
            "marshal-golden-macos-g-r2.337.0-20240305060708".into(),
        ),
        (
            "version zero",
            "marshal-golden-macos-g0-r2.337.0-20240305060708".into(),
        ),
        (
            "a leading zero",
            "marshal-golden-macos-g01-r2.337.0-20240305060708".into(),
        ),
        (
            "a signed version",
            "marshal-golden-macos-g+1-r2.337.0-20240305060708".into(),
        ),
        (
            "a version past u32",
            "marshal-golden-macos-g4294967296-r2.337.0-20240305060708".into(),
        ),
        (
            "no runner marker",
            "marshal-golden-macos-g1-2.337.0-20240305060708".into(),
        ),
        (
            "a two-part runner",
            "marshal-golden-macos-g1-r2.337-20240305060708".into(),
        ),
        (
            "a runner leading zero",
            "marshal-golden-macos-g1-r2.037.0-20240305060708".into(),
        ),
        (
            "a runner suffix",
            "marshal-golden-macos-g1-r2.337.0-rc1-20240305060708".into(),
        ),
        (
            "a 13-digit time",
            "marshal-golden-macos-g1-r2.337.0-2024030506070".into(),
        ),
        (
            "a 15-digit time",
            "marshal-golden-macos-g1-r2.337.0-202403050607080".into(),
        ),
        (
            "a lettered time",
            "marshal-golden-macos-g1-r2.337.0-2024030506070a".into(),
        ),
        (
            "an uppercase prefix",
            "Marshal-golden-macos-g1-r2.337.0-20240305060708".into(),
        ),
        ("an empty name", String::new()),
    ] {
        assert_eq!(GoldenMacosName::parse(&name), None, "{what}: {name}");
    }
}

#[test]
fn a_macos_build_name_is_formatted_and_recognised() {
    assert_eq!(build_name(pinned()), BUILD);
    assert!(is_build_name(BUILD));
}

#[test]
fn a_macos_build_name_is_read_strictly() {
    for (what, name) in [
        ("a 16-digit stamp", "marshal-build-macos-2024030506070800"),
        (
            "an 18-digit stamp",
            "marshal-build-macos-202403050607080099",
        ),
        ("a lettered stamp", "marshal-build-macos-2024030506070800x"),
        ("a prefix", "xmarshal-build-macos-20240305060708009"),
        ("a suffix", "marshal-build-macos-20240305060708009-1"),
        ("no stamp", "marshal-build-macos-"),
        ("the bare prefix", "marshal-build-macos"),
        (
            "a second-resolution stamp",
            "marshal-build-macos-20240305060708",
        ),
    ] {
        assert!(!is_build_name(name), "{what}: {name}");
    }
}

#[test]
fn the_newest_golden_macos_vm_is_chosen_by_build_time() {
    let old = golden(1, "2.337.0", pinned()).to_string();
    let new = golden(1, "2.337.0", later(1)).to_string();
    let newest = golden(1, "2.337.0", later(3_600)).to_string();
    for order in [
        [&old, &new, &newest],
        [&newest, &old, &new],
        [&new, &newest, &old],
    ] {
        let (name, parts) =
            newest_golden(order.iter().map(|name| name.as_str())).expect("a golden VM");
        assert_eq!(name, newest);
        assert_eq!(parts.built(), "20240305070708");
    }
}

#[test]
fn the_newest_golden_macos_vm_is_found_whatever_its_version_and_among_other_names() {
    let stale = golden(GOLDEN_MACOS_VM_VERSION + 1, "2.338.0", later(10)).to_string();
    let older = golden(GOLDEN_MACOS_VM_VERSION, "2.337.0", pinned()).to_string();
    let names = [
        "marshal-job-pool-1-20240305060708009",
        &older,
        BUILD,
        "ubuntu",
        &stale,
        "marshal-golden-macos-g1-r2.337.0-bad",
    ];
    let (name, parts) = newest_golden(names).expect("a golden VM");
    assert_eq!(name, stale);
    assert_eq!(parts.version(), GOLDEN_MACOS_VM_VERSION + 1);
    assert_eq!(newest_golden(["ubuntu", BUILD]), None);
    assert_eq!(newest_golden([]), None);
}

#[test]
fn golden_macos_vms_built_in_the_same_second_are_ordered_by_version_then_name() {
    let first = "marshal-golden-macos-g1-r2.337.0-20240305060708";
    let second = "marshal-golden-macos-g2-r2.337.0-20240305060708";
    let third = "marshal-golden-macos-g2-r2.338.0-20240305060708";
    assert_eq!(
        newest_golden([first, second]).map(|found| found.0),
        Some(second)
    );
    assert_eq!(
        newest_golden([second, first]).map(|found| found.0),
        Some(second)
    );
    assert_eq!(
        newest_golden([second, third]).map(|found| found.0),
        Some(third)
    );
}

#[test]
fn a_job_vm_of_a_pool_whose_name_ends_in_digits_is_read_as_that_pool() {
    // the count finds a job VM's pool with M1's reader; a pool name may end in
    // `-<digits>`, so the slot is the last number before the stamp.
    for (name, pool, slot) in [
        ("marshal-job-ci-2-1-20240305060708009", "ci-2", 1),
        ("marshal-job-ci-2-12-20240305060708009", "ci-2", 12),
        ("marshal-job-ci-2-20240305060708009", "ci", 2),
        ("marshal-job-a-b-c-3-20240305060708009", "a-b-c", 3),
    ] {
        let job = parse_job_vm_name(name).expect("a job");
        assert_eq!((job.pool.as_str(), job.slot), (pool, slot), "{name}");
    }
}

fn linux_golden(built: SystemTime) -> String {
    let mut name = GoldenVmName::current(built).expect("a declared runner version");
    name.runner_version = "2.337.0".to_string();
    name.to_string()
}

fn local(name: &str) -> VmEntry {
    VmEntry {
        name: name.to_string(),
        source: VmSource::Local,
        state: VmState::Stopped,
    }
}

#[test]
fn no_name_form_of_one_os_matches_the_others() {
    let macos_golden = golden(1, "2.337.0", pinned()).to_string();
    let macos_build = build_name(pinned());
    let linux_golden = linux_golden(pinned());
    let linux_build = build_vm_name(pinned());

    assert!(linux_golden.starts_with("marshal-golden-linux-g"));
    assert_eq!(linux_build, "marshal-build-linux-20240305060708009");

    assert!(GoldenMacosName::parse(&macos_golden).is_some());
    assert!(is_build_name(&macos_build));
    assert!(GoldenVmName::parse(&linux_golden).is_some());
    assert!(parse_build_vm_name(&linux_build).is_some());

    assert!(GoldenMacosName::parse(&linux_golden).is_none());
    assert!(!is_build_name(&linux_build));
    assert!(GoldenVmName::parse(&macos_golden).is_none());
    assert!(parse_build_vm_name(&macos_build).is_none());
    // a name of one OS's form is not the other OS's under the other form either.
    assert!(!is_build_name(&macos_golden) && !is_build_name(&linux_golden));
    assert!(parse_build_vm_name(&linux_golden).is_none());
    assert!(GoldenVmName::parse(&linux_build).is_none());
    assert!(parse_job_vm_name(&macos_build).is_none() && parse_job_vm_name(&linux_build).is_none());
}

#[test]
fn the_macos_selection_never_picks_a_linux_golden_vm_and_the_reverse() {
    let macos_old = golden(1, "2.337.0", pinned()).to_string();
    let linux_new = linux_golden(later(86_400));
    let macos_new = golden(1, "2.337.0", later(172_800)).to_string();
    let linux_old = linux_golden(pinned());
    let all = [
        macos_old.as_str(),
        linux_new.as_str(),
        macos_new.as_str(),
        linux_old.as_str(),
    ];

    assert_eq!(
        newest_golden(all).map(|found| found.0),
        Some(macos_new.as_str())
    );
    let entries: Vec<VmEntry> = all.iter().map(|name| local(name)).collect();
    assert_eq!(
        newest_golden_vm(&entries).map(|found| found.0.name.as_str()),
        Some(linux_new.as_str())
    );

    // with only the other OS's golden VMs present, nothing is found.
    assert_eq!(
        newest_golden([linux_new.as_str(), linux_old.as_str()]),
        None
    );
    assert!(newest_golden_vm(&[local(&macos_old), local(&macos_new)]).is_none());
}
