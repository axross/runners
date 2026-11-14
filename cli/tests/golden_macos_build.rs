//! `build --profile axmc` with a macOS pool, which makes the golden macOS VM: the
//! library build against the fake `tart` and `softnet`, for its sequence, its
//! refusals and its clean-up.
#![cfg(unix)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod support;

use std::time::{Duration, UNIX_EPOCH};

use marshal_cli::build::golden_macos::GoldenMacosBuild;
use marshal_cli::golden::macos::LAUNCHER;
use marshal_cli::image::macos_base::MACOS_BASE;
use marshal_cli::image::runner::DeclaredRunner;
use marshal_cli::macos::count::ConfiguredPool;
use marshal_cli::macos::name::{GOLDEN_MACOS_VM_VERSION, GoldenMacosName};
use marshal_cli::macos::softnet::OwnerMode;
use marshal_cli::profile::Os;
use marshal_cli::runtime::fake::MemoryLog;
use marshal_cli::runtime::tart::VmLauncher;
use support::golden::{describe, quick_timings};
use support::softnet::Host as SoftnetHost;
use support::tart::{Fake, invocations};

const LINUX_GOLDEN: &str = "marshal-golden-linux-g2-c2-r2.337.0-20261112070106";
const UBUNTU: &str = "ghcr.io/cirruslabs/ubuntu@sha256:fb693c6ba8df94b4d89fa1d66b5eeb53a2bbcd66f8437d0b635099cd8906282d";
const LEFTOVER: &str = "marshal-build-macos-20240101000000000";
const SHA: &str = "5a2cd92908a93d7276a194e1de6008099f3e7946f3f8e14aa7a1a7b4a31fdec2";

fn old_golden() -> String {
    GoldenMacosName::at(
        1,
        "2.336.0",
        UNIX_EPOCH + Duration::from_secs(1_704_067_200),
    )
    .unwrap()
    .to_string()
}

struct Rig {
    fake: Fake,
    _softnet: SoftnetHost,
    log: MemoryLog,
    host_macos: String,
}

impl Rig {
    fn new() -> Self {
        let softnet = SoftnetHost::new();
        softnet.link("bin");
        let path = softnet.path_with_system(&["bin"]);
        let fake = Fake::with_config(|config| config.path = Some(path.clone()));
        fake.seed(&[
            (&old_golden(), "stopped", "local"),
            (LINUX_GOLDEN, "stopped", "local"),
            (UBUNTU, "stopped", "OCI"),
        ]);
        Self {
            fake,
            _softnet: softnet,
            log: MemoryLog::default(),
            host_macos: "27.0".to_string(),
        }
    }

    async fn run(&self) -> Result<(), String> {
        let (_stop, stop) = tokio::sync::watch::channel(None);
        Box::pin(
            GoldenMacosBuild {
                client: self.fake.client.clone(),
                launcher: VmLauncher::Direct,
                base: MACOS_BASE,
                runner: DeclaredRunner {
                    version: "2.337.0".to_string(),
                    osx_arm64_sha256: SHA.to_string(),
                },
                pools: vec![ConfiguredPool {
                    name: "mac".to_string(),
                    os: Os::Macos,
                }],
                host_macos: self.host_macos.clone(),
                log: &self.log,
                timings: quick_timings(),
                softnet_owner_mode: Some(OwnerMode {
                    uid: 0,
                    mode: 0o4755,
                }),
                stop,
            }
            .run(),
        )
        .await
    }

    fn calls(&self) -> Vec<String> {
        invocations(self.fake.dir.path())
            .iter()
            .map(|call| describe(&call.arguments))
            .collect()
    }

    fn local_vms(&self) -> Vec<String> {
        self.fake
            .vms()
            .into_iter()
            .filter(|fields| fields[2] == "local")
            .map(|fields| fields[0].clone())
            .collect()
    }
}

/// runs the build to its end on a current-thread runtime and returns its outcome.
fn build(rig: &Rig) -> Result<(), String> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(rig.run())
}

#[test]
fn the_build_provisions_the_base_and_commits_the_golden_macos_vm() {
    let rig = Rig::new();
    assert_eq!(build(&rig), Ok(()), "{:?}", rig.log.lines());
    let calls = rig.calls();
    let base = MACOS_BASE.reference();
    let order = [
        format!("pull {base}"),
        format!("clone {base} marshal-build-macos-"),
        "run ".to_string(),
        "exec marshal-remove-runner".to_string(),
        "exec marshal-install-runner".to_string(),
        "exec marshal-install-launcher".to_string(),
        "exec marshal-disable-remote-access".to_string(),
        "exec marshal-disable-updates".to_string(),
        "exec marshal-flush-disk".to_string(),
        "stop marshal-build-macos-".to_string(),
        "rename marshal-build-macos-".to_string(),
    ];
    let mut at = 0;
    for prefix in &order {
        let found = calls[at..]
            .iter()
            .position(|call| call.starts_with(prefix.as_str()))
            .unwrap_or_else(|| panic!("no {prefix:?} after call {at}: {calls:?}"));
        at += found + 1;
    }
    let goldens: Vec<String> = rig
        .local_vms()
        .into_iter()
        .filter(|name| GoldenMacosName::parse(name).is_some())
        .collect();
    assert_eq!(goldens.len(), 1, "{goldens:?}");
    let golden = GoldenMacosName::parse(&goldens[0]).unwrap();
    assert_eq!(golden.version(), GOLDEN_MACOS_VM_VERSION);
    assert_eq!(golden.runner_version(), "2.337.0");
    assert!(
        !rig.local_vms()
            .iter()
            .any(|name| name.starts_with("marshal-build-macos-"))
    );
    assert!(rig.local_vms().contains(&LINUX_GOLDEN.to_string()));
    assert!(rig.fake.vm_names().contains(&UBUNTU.to_string()));
    let launcher_call = rig
        .fake
        .invocations()
        .into_iter()
        .find(|call| describe(&call.arguments) == "exec marshal-install-launcher")
        .unwrap();
    assert_eq!(launcher_call.stdin.as_deref(), Some(LAUNCHER.as_bytes()));
}

#[test]
fn a_leftover_macos_build_vm_is_deleted_first_and_the_linux_vms_are_not() {
    let rig = Rig::new();
    rig.fake.seed(&[
        (&old_golden(), "stopped", "local"),
        (LINUX_GOLDEN, "stopped", "local"),
        (LEFTOVER, "stopped", "local"),
    ]);
    assert_eq!(build(&rig), Ok(()), "{:?}", rig.log.lines());
    let calls = rig.calls();
    let deleted = calls
        .iter()
        .position(|c| c == &format!("delete {LEFTOVER}"))
        .unwrap();
    let pulled = calls.iter().position(|c| c.starts_with("pull ")).unwrap();
    assert!(deleted < pulled, "{calls:?}");
    assert!(
        !calls
            .iter()
            .any(|c| c.starts_with("delete marshal-golden-linux-"))
    );
}

#[test]
fn a_runner_archive_that_fails_its_check_deletes_the_build_vm_and_keeps_the_old_golden_vm() {
    let rig = Rig::new();
    rig.fake.script(
        "fail-exec-marshal-install-runner",
        "error: the runner archive's SHA-256 is 00, not the recorded 5a",
    );
    let outcome = build(&rig).unwrap_err();
    assert!(
        outcome.starts_with("the golden macOS VM build failed (installing runner 2.337.0 failed"),
        "{outcome}"
    );
    assert!(
        outcome.ends_with("so the golden macOS VM is unchanged"),
        "{outcome}"
    );
    assert!(rig.local_vms().contains(&old_golden()));
    assert!(
        !rig.local_vms()
            .iter()
            .any(|name| name.starts_with("marshal-build-macos-"))
    );
    assert!(
        !rig.calls()
            .iter()
            .any(|c| c.starts_with("exec marshal-install-launcher"))
    );
}

#[test]
fn two_counting_macos_vms_refuse_the_build_before_anything_is_pulled() {
    let rig = Rig::new();
    rig.fake.seed(&[
        (&old_golden(), "stopped", "local"),
        ("someone-elses-mac", "running", "local"),
        ("marshal-job-mac-1-20260101000000000", "stopped", "local"),
        ("someone-elses-linux", "running", "local"),
    ]);
    rig.fake.script("os-someone-elses-linux", "linux");
    assert_eq!(
        build(&rig),
        Err("2 macOS VMs already count against the limit of 2 \
             (marshal-job-mac-1-20260101000000000, someone-elses-mac), so no macOS build VM was \
             created; stop or delete one of them and build again"
            .to_string())
    );
    assert!(
        !rig.calls()
            .iter()
            .any(|c| c.starts_with("pull ") || c.starts_with("clone "))
    );
}

#[test]
fn a_running_linux_vm_does_not_count_and_the_build_goes_on() {
    let rig = Rig::new();
    rig.fake.seed(&[
        ("someone-elses-mac", "running", "local"),
        ("someone-elses-linux", "running", "local"),
    ]);
    rig.fake.script("os-someone-elses-linux", "linux");
    assert_eq!(build(&rig), Ok(()), "{:?}", rig.log.lines());
}

#[test]
fn a_mac_older_than_the_base_refuses_the_build_before_anything_is_pulled() {
    let mut rig = Rig::new();
    rig.host_macos = "26.5".to_string();
    assert_eq!(
        build(&rig),
        Err(
            "the pinned macOS base is macOS 26.6.2, newer than this Mac's macOS 26.5; update the \
         Mac first"
                .to_string()
        )
    );
    assert!(!rig.calls().iter().any(|c| c.starts_with("pull ")));
}
