//! the count of macOS VMs against the limit of two, over a fake Tart: every row of
//! the counting table, the VMs the OS lookup is and is not asked about, and two
//! slots racing for the last place.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::cell::RefCell;
use std::collections::BTreeMap;

use marshal_cli::macos::count::{
    ConfiguredPool, CountedVm, GuestOs, GuestOsLookup, ListedVm, MacosVmCount, Reason, VmState,
    count_macos_vms,
};
use marshal_cli::macos::place::PlaceLock;
use marshal_cli::profile::Os;
use tokio::task::yield_now;

const STAMP: &str = "20240305060708009";
const ALL_STATES: [VmState; 3] = [VmState::Running, VmState::Suspended, VmState::Stopped];
const BOOTED: [VmState; 2] = [VmState::Running, VmState::Suspended];

/// a Tart that lists VMs, answers `tart get` from a table, and records each
/// question and each clone or deletion.
#[derive(Default)]
struct FakeTart {
    vms: RefCell<Vec<ListedVm>>,
    os: RefCell<BTreeMap<String, GuestOs>>,
    asked: RefCell<Vec<String>>,
    failing_deletes: RefCell<u32>,
    cloned: RefCell<Vec<String>>,
}

impl FakeTart {
    fn with(vms: &[(&str, VmState)]) -> Self {
        let tart = Self::default();
        for (name, state) in vms {
            tart.add(name, *state);
        }
        tart
    }

    fn add(&self, name: &str, state: VmState) {
        self.vms.borrow_mut().push(ListedVm {
            name: name.to_string(),
            state,
        });
    }

    fn reports(&self, name: &str, os: GuestOs) -> &Self {
        self.os.borrow_mut().insert(name.to_string(), os);
        self
    }

    async fn list(&self) -> Vec<ListedVm> {
        yield_now().await;
        self.vms.borrow().clone()
    }

    /// a clone takes time, then is listed, stopped.
    async fn clone_vm(&self, name: &str) {
        yield_now().await;
        yield_now().await;
        self.add(name, VmState::Stopped);
        self.cloned.borrow_mut().push(name.to_string());
    }

    /// a deletion that fails leaves the VM listed.
    async fn delete(&self, name: &str) -> Result<(), String> {
        yield_now().await;
        let mut failing = self.failing_deletes.borrow_mut();
        if *failing > 0 {
            *failing -= 1;
            return Err(format!("could not delete {name}"));
        }
        self.vms.borrow_mut().retain(|vm| vm.name != name);
        Ok(())
    }

    fn asked(&self) -> Vec<String> {
        let mut asked = self.asked.borrow().clone();
        asked.sort();
        asked
    }
}

impl GuestOsLookup for FakeTart {
    async fn guest_os(&self, name: &str) -> GuestOs {
        yield_now().await;
        self.asked.borrow_mut().push(name.to_string());
        self.os
            .borrow()
            .get(name)
            .copied()
            .unwrap_or(GuestOs::Unreadable)
    }
}

fn pools() -> Vec<ConfiguredPool> {
    [("mac", Os::Macos), ("mac-2", Os::Linux), ("lin", Os::Linux)]
        .into_iter()
        .map(|(name, os)| ConfiguredPool {
            name: name.to_string(),
            os,
        })
        .collect()
}

async fn count(tart: &FakeTart) -> MacosVmCount {
    count_macos_vms(&tart.list().await, &pools(), tart).await
}

fn job(pool: &str, slot: u32) -> String {
    format!("marshal-job-{pool}-{slot}-{STAMP}")
}

const MACOS_GOLDEN: &str = "marshal-golden-macos-g1-r2.337.0-20240305060708";
const MACOS_BUILD: &str = "marshal-build-macos-20240305060708009";
const LINUX_GOLDEN: &str = "marshal-golden-linux-g1-c1-r2.337.0-20240305060708";
const LINUX_BUILD: &str = "marshal-build-linux-20240305060708009";

fn names(count: &MacosVmCount) -> Vec<&str> {
    count.counted().iter().map(|vm| vm.name.as_str()).collect()
}

fn reason_of(count: &MacosVmCount, name: &str) -> Option<Reason> {
    count
        .counted()
        .iter()
        .find(|vm| vm.name == name)
        .map(|vm| vm.reason)
}

#[tokio::test(flavor = "current_thread")]
async fn a_job_vm_of_a_configured_macos_pool_counts_in_every_state_without_a_lookup() {
    for state in ALL_STATES {
        let name = job("mac", 1);
        let tart = FakeTart::with(&[(&name, state)]);
        let counted = count(&tart).await;
        assert_eq!(counted.total(), 1, "{state:?}");
        assert_eq!(
            reason_of(&counted, &name),
            Some(Reason::ConfiguredMacosJob),
            "{state:?}"
        );
        assert!(counted.leftover_jobs().is_empty());
        assert!(tart.asked().is_empty(), "{state:?}");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_stopped_job_vm_of_a_configured_macos_pool_still_counts() {
    let name = job("mac", 2);
    let tart = FakeTart::with(&[(&name, VmState::Stopped)]);
    assert_eq!(count(&tart).await.total(), 1);
}

#[tokio::test(flavor = "current_thread")]
async fn a_macos_build_vm_counts_in_every_state_without_a_lookup() {
    for state in ALL_STATES {
        let tart = FakeTart::with(&[(MACOS_BUILD, state)]);
        let counted = count(&tart).await;
        assert_eq!(reason_of(&counted, MACOS_BUILD), Some(Reason::MacosBuild));
        assert!(tart.asked().is_empty(), "{state:?}");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_leftover_job_vm_of_an_unconfigured_pool_reported_as_macos_counts_in_every_state() {
    for state in ALL_STATES {
        let name = job("gone", 1);
        let tart = FakeTart::with(&[(&name, state)]);
        tart.reports(&name, GuestOs::Macos);
        let counted = count(&tart).await;
        assert_eq!(
            reason_of(&counted, &name),
            Some(Reason::LeftoverJobReportedMacos),
            "{state:?}"
        );
        assert_eq!(counted.leftover_jobs(), [name.as_str()], "{state:?}");
        assert_eq!(tart.asked(), [name.as_str()], "{state:?}");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_leftover_job_vm_of_an_unconfigured_pool_reported_as_linux_does_not_count() {
    for state in ALL_STATES {
        let name = job("gone", 1);
        let tart = FakeTart::with(&[(&name, state)]);
        tart.reports(&name, GuestOs::Linux);
        let counted = count(&tart).await;
        assert_eq!(counted.total(), 0, "{state:?}");
        assert!(counted.leftover_jobs().is_empty());
        assert_eq!(tart.asked(), [name.as_str()], "{state:?}");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_leftover_job_vm_whose_os_is_unreadable_counts_in_every_state_and_is_named() {
    for state in ALL_STATES {
        let name = job("gone", 1);
        let tart = FakeTart::with(&[(&name, state)]);
        let counted = count(&tart).await;
        assert_eq!(counted.total(), 1, "{state:?}");
        assert_eq!(
            reason_of(&counted, &name),
            Some(Reason::UnreadableOs),
            "{state:?}"
        );
        assert_eq!(counted.leftover_jobs(), [name.as_str()], "{state:?}");
        assert_eq!(tart.asked(), [name.as_str()], "{state:?}");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_running_vm_reported_running_while_it_stops_counts() {
    let tart = FakeTart::with(&[("someone-elses", VmState::Running)]);
    tart.reports("someone-elses", GuestOs::Macos);
    assert_eq!(count(&tart).await.total(), 1);
}

#[tokio::test(flavor = "current_thread")]
async fn a_running_or_suspended_foreign_vm_reported_as_macos_counts() {
    for state in BOOTED {
        let tart = FakeTart::with(&[("foreign", state)]);
        tart.reports("foreign", GuestOs::Macos);
        let counted = count(&tart).await;
        assert_eq!(reason_of(&counted, "foreign"), Some(Reason::ReportedMacos));
        assert!(counted.leftover_jobs().is_empty());
        assert_eq!(tart.asked(), ["foreign"]);
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_running_or_suspended_vm_whose_os_is_unreadable_counts() {
    for state in BOOTED {
        let tart = FakeTart::with(&[("foreign", state)]);
        let counted = count(&tart).await;
        assert_eq!(reason_of(&counted, "foreign"), Some(Reason::UnreadableOs));
        assert_eq!(tart.asked(), ["foreign"]);
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_running_or_suspended_foreign_linux_vm_does_not_count() {
    for state in BOOTED {
        let tart = FakeTart::with(&[("foreign", state)]);
        tart.reports("foreign", GuestOs::Linux);
        assert_eq!(count(&tart).await.total(), 0, "{state:?}");
        assert_eq!(tart.asked(), ["foreign"]);
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_stopped_vm_marshal_did_not_create_does_not_count_and_is_not_asked_about() {
    let tart = FakeTart::with(&[("foreign", VmState::Stopped)]);
    tart.reports("foreign", GuestOs::Macos);
    assert_eq!(count(&tart).await.total(), 0);
    assert!(tart.asked().is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn the_golden_macos_vm_does_not_count_while_stopped_and_is_not_asked_about() {
    let tart = FakeTart::with(&[(MACOS_GOLDEN, VmState::Stopped)]);
    tart.reports(MACOS_GOLDEN, GuestOs::Macos);
    assert_eq!(count(&tart).await.total(), 0);
    assert!(tart.asked().is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn a_golden_macos_vm_booted_by_hand_counts_as_a_reported_macos_vm() {
    for state in BOOTED {
        let tart = FakeTart::with(&[(MACOS_GOLDEN, state)]);
        tart.reports(MACOS_GOLDEN, GuestOs::Macos);
        let counted = count(&tart).await;
        assert_eq!(
            reason_of(&counted, MACOS_GOLDEN),
            Some(Reason::ReportedMacos),
            "{state:?}"
        );
        assert_eq!(tart.asked(), [MACOS_GOLDEN], "{state:?}");
    }
    let tart = FakeTart::with(&[(MACOS_GOLDEN, VmState::Running)]);
    assert_eq!(
        reason_of(&count(&tart).await, MACOS_GOLDEN),
        Some(Reason::UnreadableOs)
    );
}

#[tokio::test(flavor = "current_thread")]
async fn linux_build_and_golden_vms_never_count_and_are_never_asked_about() {
    for state in ALL_STATES {
        let tart = FakeTart::with(&[(LINUX_BUILD, state), (LINUX_GOLDEN, state)]);
        // an answer of macOS would count them, so a lookup would show.
        for name in [LINUX_BUILD, LINUX_GOLDEN] {
            tart.reports(name, GuestOs::Macos);
        }
        assert_eq!(count(&tart).await.total(), 0, "{state:?}");
        assert!(tart.asked().is_empty(), "{state:?}");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_job_vm_of_a_configured_linux_pool_counts_when_the_lookup_reports_macos() {
    for state in ALL_STATES {
        let name = job("lin", 1);
        let tart = FakeTart::with(&[(&name, state)]);
        tart.reports(&name, GuestOs::Macos);
        let counted = count(&tart).await;
        assert_eq!(counted.total(), 1, "{state:?}");
        assert_eq!(
            reason_of(&counted, &name),
            Some(Reason::LinuxPoolJobReportedMacos),
            "{state:?}"
        );
        assert!(counted.leftover_jobs().is_empty(), "{state:?}");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_job_vm_of_a_configured_linux_pool_does_not_count_when_reported_as_linux() {
    for state in ALL_STATES {
        let name = job("lin", 1);
        let tart = FakeTart::with(&[(&name, state)]);
        tart.reports(&name, GuestOs::Linux);
        assert_eq!(count(&tart).await.total(), 0, "{state:?}");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_job_vm_of_a_configured_linux_pool_does_not_count_when_its_os_is_unreadable() {
    for state in ALL_STATES {
        let name = job("lin", 1);
        let tart = FakeTart::with(&[(&name, state)]);
        let counted = count(&tart).await;
        assert_eq!(counted.total(), 0, "{state:?}");
        assert!(counted.leftover_jobs().is_empty(), "{state:?}");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn the_lookup_is_asked_once_about_each_job_vm_of_a_configured_linux_pool() {
    let (first, second, third) = (job("lin", 1), job("lin", 2), job("lin", 3));
    let tart = FakeTart::with(&[
        (&first, VmState::Running),
        (&second, VmState::Stopped),
        (&third, VmState::Suspended),
    ]);
    count(&tart).await;
    let mut expected = [first, second, third];
    expected.sort();
    assert_eq!(tart.asked(), expected);
}

#[tokio::test(flavor = "current_thread")]
async fn a_pool_name_ending_in_digits_is_matched_whole() {
    // `mac-2` is a Linux pool and `mac` a macOS one: slot 1 of the first and slot 2
    // of the second have names that differ only in where the hyphens fall.
    let of_linux_pool = format!("marshal-job-mac-2-1-{STAMP}");
    let of_macos_pool = format!("marshal-job-mac-2-{STAMP}");
    let tart = FakeTart::with(&[
        (&of_linux_pool, VmState::Running),
        (&of_macos_pool, VmState::Running),
    ]);
    let counted = count(&tart).await;
    assert_eq!(names(&counted), [of_macos_pool.as_str()]);
    assert_eq!(tart.asked(), [of_linux_pool.as_str()]);
}

#[tokio::test(flavor = "current_thread")]
async fn a_name_that_only_looks_like_a_marshal_name_is_a_foreign_vm() {
    let lookalikes = [
        format!("marshal-job-mac-1-{}", &STAMP[1..]),
        format!("marshal-job-mac-x-{STAMP}"),
        "marshal-build-macos-123".to_string(),
        "marshal-golden-macos-g1".to_string(),
        format!("x-marshal-build-linux-{STAMP}"),
    ];
    for name in &lookalikes {
        let stopped = FakeTart::with(&[(name, VmState::Stopped)]);
        assert_eq!(count(&stopped).await.total(), 0, "{name}");
        assert!(stopped.asked().is_empty(), "{name}");
        let running = FakeTart::with(&[(name, VmState::Running)]);
        running.reports(name, GuestOs::Macos);
        assert_eq!(count(&running).await.total(), 1, "{name}");
        assert_eq!(running.asked(), [name.as_str()], "{name}");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn each_counted_vm_has_its_reason_and_the_lookup_is_asked_once_for_each_open_vm() {
    let (macos_job, gone, gone_linux, gone_unreadable) = (
        job("mac", 1),
        job("gone", 1),
        job("gone", 2),
        job("gone-too", 1),
    );
    let (linux_job, linux_job_reported_macos) = (job("lin", 1), job("lin", 2));
    let tart = FakeTart::with(&[
        (&macos_job, VmState::Stopped),
        (MACOS_BUILD, VmState::Running),
        (MACOS_GOLDEN, VmState::Stopped),
        (LINUX_GOLDEN, VmState::Running),
        (&linux_job, VmState::Running),
        (&linux_job_reported_macos, VmState::Stopped),
        (&gone, VmState::Stopped),
        (&gone_linux, VmState::Running),
        (&gone_unreadable, VmState::Suspended),
        ("foreign-mac", VmState::Running),
        ("foreign-linux", VmState::Running),
        ("foreign-unknown", VmState::Suspended),
        ("foreign-stopped", VmState::Stopped),
    ]);
    tart.reports(&gone, GuestOs::Macos)
        .reports(&linux_job, GuestOs::Linux)
        .reports(&linux_job_reported_macos, GuestOs::Macos)
        .reports(&gone_linux, GuestOs::Linux)
        .reports("foreign-mac", GuestOs::Macos)
        .reports("foreign-linux", GuestOs::Linux);

    let counted = count(&tart).await;
    assert_eq!(
        counted.counted(),
        [
            CountedVm {
                name: macos_job,
                reason: Reason::ConfiguredMacosJob,
                leftover_job: false
            },
            CountedVm {
                name: MACOS_BUILD.to_string(),
                reason: Reason::MacosBuild,
                leftover_job: false
            },
            CountedVm {
                name: linux_job_reported_macos.clone(),
                reason: Reason::LinuxPoolJobReportedMacos,
                leftover_job: false
            },
            CountedVm {
                name: gone.clone(),
                reason: Reason::LeftoverJobReportedMacos,
                leftover_job: true
            },
            CountedVm {
                name: gone_unreadable.clone(),
                reason: Reason::UnreadableOs,
                leftover_job: true
            },
            CountedVm {
                name: "foreign-mac".to_string(),
                reason: Reason::ReportedMacos,
                leftover_job: false
            },
            CountedVm {
                name: "foreign-unknown".to_string(),
                reason: Reason::UnreadableOs,
                leftover_job: false
            },
        ]
    );
    assert_eq!(counted.total(), 7);
    assert_eq!(
        counted.leftover_jobs(),
        [gone.as_str(), gone_unreadable.as_str()]
    );
    let mut expected_asked = vec![
        linux_job,
        linux_job_reported_macos,
        gone,
        gone_linux,
        gone_unreadable,
        "foreign-mac".to_string(),
        "foreign-linux".to_string(),
        "foreign-unknown".to_string(),
    ];
    expected_asked.sort();
    assert_eq!(tart.asked(), expected_asked);
}

#[tokio::test(flavor = "current_thread")]
async fn a_place_is_free_below_two_and_not_at_two() {
    let first = job("mac", 1);
    let second = job("mac", 2);
    let tart = FakeTart::with(&[(&first, VmState::Running)]);
    assert!(count(&tart).await.has_free_place());
    tart.add(&second, VmState::Stopped);
    assert!(!count(&tart).await.has_free_place());
    tart.add("a-third", VmState::Running);
    tart.reports("a-third", GuestOs::Macos);
    let three = count(&tart).await;
    assert_eq!(three.total(), 3);
    assert!(!three.has_free_place());
}

#[tokio::test(flavor = "current_thread")]
async fn a_failed_deletion_keeps_the_vm_counting_until_a_later_attempt_deletes_it() {
    let (first, second) = (job("mac", 1), job("mac", 2));
    let tart = FakeTart::with(&[(&first, VmState::Stopped), (&second, VmState::Running)]);
    *tart.failing_deletes.borrow_mut() = 2;
    assert_eq!(count(&tart).await.total(), 2);

    assert!(tart.delete(&first).await.is_err());
    assert_eq!(
        count(&tart).await.total(),
        2,
        "still listed after a failure"
    );
    assert!(tart.delete(&first).await.is_err());
    assert_eq!(count(&tart).await.total(), 2, "still listed after another");

    tart.delete(&first).await.unwrap();
    let counted = count(&tart).await;
    assert_eq!(names(&counted), [second.as_str()]);
    assert!(counted.has_free_place());
}

/// what a slot did with its turn at the last place.
#[derive(Debug, PartialEq, Eq)]
enum Turn {
    Cloned,
    SawTwo,
}

/// a slot's "count, then clone", under `lock` when it has one.
async fn take_a_place(tart: &FakeTart, lock: Option<&PlaceLock>, slot: u32) -> Turn {
    let _held = match lock {
        Some(lock) => Some(lock.take().await),
        None => None,
    };
    let counted = count(tart).await;
    if !counted.has_free_place() {
        assert_eq!(counted.total(), 2);
        return Turn::SawTwo;
    }
    tart.clone_vm(&job("mac", slot)).await;
    Turn::Cloned
}

fn one_place_left() -> FakeTart {
    let tart = FakeTart::with(&[("foreign", VmState::Running)]);
    tart.reports("foreign", GuestOs::Macos);
    tart
}

#[tokio::test(flavor = "current_thread")]
async fn two_slots_racing_for_the_last_place_get_it_one_at_a_time() {
    let tart = one_place_left();
    let lock = PlaceLock::new();
    let (first, second) = tokio::join!(
        take_a_place(&tart, Some(&lock), 1),
        take_a_place(&tart, Some(&lock), 2)
    );
    let mut turns = [first, second];
    turns.sort_by_key(|turn| matches!(turn, Turn::SawTwo));
    assert_eq!(turns, [Turn::Cloned, Turn::SawTwo]);
    assert_eq!(tart.cloned.borrow().len(), 1);
    assert_eq!(count(&tart).await.total(), 2);
}

#[tokio::test(flavor = "current_thread")]
async fn without_the_lock_the_same_race_takes_the_place_twice() {
    let tart = one_place_left();
    let (first, second) = tokio::join!(take_a_place(&tart, None, 1), take_a_place(&tart, None, 2));
    assert_eq!((first, second), (Turn::Cloned, Turn::Cloned));
    assert_eq!(count(&tart).await.total(), 3);
}

#[tokio::test(flavor = "current_thread")]
async fn three_slots_racing_for_two_places_clone_twice_and_the_third_waits() {
    let tart = FakeTart::default();
    let lock = PlaceLock::new();
    let (a, b, c) = tokio::join!(
        take_a_place(&tart, Some(&lock), 1),
        take_a_place(&tart, Some(&lock), 2),
        take_a_place(&tart, Some(&lock), 3)
    );
    let turns = [a, b, c];
    assert_eq!(
        turns.iter().filter(|turn| **turn == Turn::Cloned).count(),
        2
    );
    assert_eq!(
        turns.iter().filter(|turn| **turn == Turn::SawTwo).count(),
        1
    );
    assert_eq!(count(&tart).await.total(), 2);
}
