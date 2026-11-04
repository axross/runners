//! `build --profile axmc`, which makes the golden VM: the library build against the fake
//! `tart`, `softnet` and `docker` for the sequence and the failure and stop matrix,
//! and `marshal build` as a real process for the refusals, the VM shim and the
//! signals.
#![cfg(unix)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod support;

use std::time::{Duration, Instant};

use marshal_cli::golden::pins::{BASE_REFERENCE, DOCKER_KEY_FINGERPRINT, RETIRED_BASE_PINS};
use marshal_cli::golden::steps::LAUNCHER;
use marshal_cli::image::contract::BUILT_LABEL;
use marshal_cli::image::contract::CONTRACT_VERSION;
use marshal_cli::runtime::tart::VmLauncher;
use marshal_cli::runtime::tart::names::{GoldenVmName, parse_build_vm_name};
use support::golden::{IMAGE, OLD_BUILT_SECONDS, REAL_LISTING, Rig, big_stack, old_golden};
use support::host::{send_signal, texts};

const LEFTOVER: &str = "marshal-build-linux-20240101000000000";
const RETIRED: &str = "ghcr.io/cirruslabs/ubuntu@sha256:0000000000000000000000000000000000000000000000000000000000000001";
const UNRELATED: &str =
    "ghcr.io/other/image@sha256:0000000000000000000000000000000000000000000000000000000000000002";

/// asserts that `calls` has a call starting with each of `prefixes`, in that order.
fn assert_in_order(calls: &[String], prefixes: &[&str]) {
    let mut from = 0;
    for prefix in prefixes {
        let found = calls[from..]
            .iter()
            .position(|call| call.starts_with(prefix))
            .unwrap_or_else(|| {
                panic!("no call starting {prefix:?} after {from}; the calls: {calls:#?}")
            });
        from += found + 1;
    }
}

fn build_vm_of(rig: &Rig) -> String {
    rig.calls_starting("clone ")[0]
        .split(' ')
        .nth(2)
        .unwrap()
        .to_string()
}

#[test]
fn the_build_runs_its_calls_in_order_and_commits_by_renaming() {
    big_stack(|| async {
        let rig = Rig::new();
        rig.run().await.unwrap();
        let calls = rig.calls();
        assert_in_order(
            &calls,
            &[
                "--version",
                "list ",
                &format!("pull {BASE_REFERENCE}"),
                &format!("clone {BASE_REFERENCE} marshal-build-linux-"),
                "exec marshal-prepare",
                "exec marshal-fetch-key",
                "exec marshal-list-key",
                "exec marshal-install-docker",
                "exec marshal-install-launcher",
                "exec marshal-disable-ssh",
                "docker info",
                "docker build",
                "docker image inspect",
                "docker tag",
                "docker image rm",
                "stop ",
                "rename ",
                "list ",
                &format!("delete {}", old_golden()),
            ],
        );
        // the held `tart run` and the first guest-agent probe are spawned back to back, so
        // which fake process claims its number first is the scheduler's choice
        let first = |prefix: &str| {
            calls
                .iter()
                .position(|call| call.starts_with(prefix))
                .unwrap_or_else(|| panic!("no call starting {prefix:?}; the calls: {calls:#?}"))
        };
        let (cloned, prepared) = (first("clone "), first("exec marshal-prepare"));
        for boot in ["run ", "exec ask"] {
            let at = first(boot);
            assert!(
                cloned < at && at < prepared,
                "{boot:?} at {at} is not between the clone at {cloned} and the preparation at {prepared}; the calls: {calls:#?}"
            );
        }
        let build_vm = build_vm_of(&rig);
        assert!(parse_build_vm_name(&build_vm).is_some(), "{build_vm}");
        let renamed = rig.calls_starting("rename ");
        assert_eq!(renamed.len(), 1);
        let target = renamed[0]
            .strip_prefix(&format!("rename {build_vm} "))
            .unwrap();
        let golden = GoldenVmName::parse(target).unwrap();
        assert!(golden.matches_this_cli(), "{target}");
        assert_eq!(rig.local_vms(), [target.to_string()]);
        assert_eq!(rig.build_vms(), Vec::<String>::new());
        let log = rig.log.lines().join("\n");
        assert!(
            log.contains(&format!(
                "The golden VM {target} is built, with {IMAGE} inside."
            )),
            "{log}"
        );
    });
}

#[test]
fn every_tart_call_has_no_auto_prune_and_the_path() {
    big_stack(|| async {
        let rig = Rig::new();
        rig.run().await.unwrap();
        let path = rig.path.to_string_lossy().into_owned();
        let invocations = rig.fake.invocations();
        assert!(invocations.len() > 20, "{}", invocations.len());
        for call in &invocations {
            assert_eq!(
                call.no_auto_prune.as_deref(),
                Some("1"),
                "{:?}",
                call.arguments
            );
            assert_eq!(
                call.path.as_deref(),
                Some(path.as_str()),
                "{:?}",
                call.arguments
            );
            assert!(!call.jit_present);
        }
    });
}

#[test]
fn the_provisioning_calls_carry_their_scripts_and_the_launcher_as_input() {
    big_stack(|| async {
        let rig = Rig::new();
        rig.run().await.unwrap();
        let launcher_call = rig
            .fake
            .invocations()
            .into_iter()
            .find(|call| {
                call.arguments
                    .iter()
                    .any(|word| word == "marshal-install-launcher")
            })
            .unwrap();
        assert_eq!(launcher_call.stdin.as_deref(), Some(LAUNCHER.as_bytes()));
    });
}

#[test]
fn the_image_is_built_in_the_vm_as_root_under_a_temporary_tag_and_then_tagged() {
    big_stack(|| async {
        let rig = Rig::new();
        rig.run().await.unwrap();
        let invocations = rig.fake.invocations();
        let docker: Vec<&Vec<String>> = invocations
            .iter()
            .map(|call| &call.arguments)
            .filter(|arguments| arguments.iter().any(|word| word == "docker"))
            .collect();
        for arguments in &docker {
            let at = arguments.iter().position(|word| word == "sudo").unwrap();
            assert_eq!(arguments[at + 1], "-n");
            assert_eq!(arguments[at + 2], "docker");
        }
        let build = invocations
            .iter()
            .find(|call| {
                call.arguments.iter().any(|word| word == "build")
                    && call.arguments.iter().any(|word| word == "docker")
            })
            .unwrap();
        assert_eq!(build.stdin.as_deref(), Some(&b"the build context"[..]));
        let words = &build.arguments;
        let built = words.iter().position(|word| word == "build").unwrap();
        let temporary = words[words.iter().position(|word| word == "--tag").unwrap() + 1].clone();
        assert!(
            temporary.starts_with("actions-runner:marshal-build-"),
            "{temporary}"
        );
        assert_eq!(
            &words[built + 1..built + 4],
            ["--pull", "--no-cache", "--label"]
        );
        assert!(words.contains(&"com.github.axross.runners.image=actions-runner".to_string()));
        assert_eq!(words.last().map(String::as_str), Some("-"));
        let tag = docker
            .iter()
            .find(|arguments| arguments.iter().any(|word| word == "tag"))
            .unwrap();
        assert_eq!(
            &tag[tag.len() - 2..],
            [temporary.clone(), IMAGE.to_string()]
        );
        let removed = docker
            .iter()
            .find(|arguments| arguments.iter().any(|word| word == "rm"))
            .unwrap();
        assert_eq!(removed.last(), Some(&temporary));
        assert!(
            rig.log
                .lines()
                .iter()
                .any(|line| line.contains("fake build output")),
            "{:?}",
            rig.log.lines()
        );
    });
}

#[test]
fn the_build_vm_is_not_resized() {
    big_stack(|| async {
        let rig = Rig::new();
        rig.run().await.unwrap();
        assert!(rig.position("set ").is_none(), "{:?}", rig.calls());
    });
}

/// what a failure at one step leaves: an error naming the step, no build VM, the
/// old golden VM, no rename but the rename's own, and no deletion of anything but the
/// build VM.
async fn fails(configure: impl FnOnce(&mut Rig), expected: &str) -> Rig {
    let mut rig = Rig::new();
    configure(&mut rig);
    let error = rig.run().await.unwrap_err();
    assert!(error.contains(expected), "{error}");
    assert!(error.ends_with("so the golden VM is unchanged"), "{error}");
    assert_eq!(rig.build_vms(), Vec::<String>::new(), "{error}");
    assert_eq!(rig.golden_vms(), [old_golden()], "{error}");
    for call in rig.calls_starting("delete ") {
        assert!(call.starts_with("delete marshal-build-linux-"), "{call}");
    }
    rig
}

#[test]
fn a_failed_provisioning_call_deletes_the_build_vm_and_keeps_the_old_golden_vm() {
    big_stack(|| async {
        let steps = [
            ("marshal-prepare", "upgrading the Ubuntu packages failed"),
            (
                "marshal-fetch-key",
                "downloading Docker's signing key failed",
            ),
            ("marshal-list-key", "checking Docker's signing key failed"),
            ("marshal-install-docker", "installing Docker Engine failed"),
            ("marshal-install-launcher", "installing the launcher failed"),
            ("marshal-disable-ssh", "disabling the SSH login failed"),
        ];
        for (name, expected) in steps {
            let rig = Box::pin(fails(
                |rig| rig.fake.script(&format!("exec_exit-{name}"), "3"),
                expected,
            ))
            .await;
            assert!(rig.position("rename ").is_none(), "{name}");
        }
    });
}

#[test]
fn a_failed_pull_or_clone_deletes_the_build_vm_and_keeps_the_old_golden_vm() {
    big_stack(|| async {
        let rig = fails(
            |rig| rig.fake.script("fail-pull", "pull went wrong"),
            "pull went wrong",
        )
        .await;
        assert!(rig.position("clone ").is_none());
        let rig = fails(
            |rig| rig.fake.script("fail-clone", "clone went wrong"),
            "clone went wrong",
        )
        .await;
        assert!(rig.position("run ").is_none());
    });
}

#[test]
fn a_failed_boot_deletes_the_build_vm_and_keeps_the_old_golden_vm() {
    big_stack(|| async {
        fails(
            |rig| {
                rig.fake.script("run_mode", "exit");
                rig.fake.script("run_error", "the keychain is locked");
            },
            "boot failure: tart run for the build VM",
        )
        .await;
        fails(
            |rig| {
                rig.fake.script("fail-exec", "no agent");
                rig.timings.agent.deadline = Duration::from_millis(300);
            },
            "the guest agent of the build VM",
        )
        .await;
    });
}

#[test]
fn a_shim_that_cannot_be_started_is_a_start_failure_not_an_unanswered_agent() {
    big_stack(|| async {
        let rig = fails(
            |rig| rig.launcher = VmLauncher::Shim("/nonexistent/marshal-test-shim".into()),
            "boot failure: tart run for the build VM",
        )
        .await;
        let error = rig.run().await.unwrap_err();
        assert!(error.contains("could not be started"), "{error}");
        assert!(!error.contains("did not answer"), "{error}");
        assert!(rig.position("exec ").is_none(), "{:?}", rig.calls());
    });
}

#[test]
fn a_failed_image_build_deletes_the_build_vm_and_keeps_the_old_golden_vm() {
    big_stack(|| async {
        fails(
            |rig| rig.docker_script("info_failures", "1000"),
            "did not answer within",
        )
        .await;
        let rig = fails(
            |rig| rig.docker_script("build_fails", ""),
            "docker build exited with code 1",
        )
        .await;
        assert!(rig.position("rename ").is_none());
        fails(
            |rig| rig.docker_script("build_contract", "999"),
            "image contract 999",
        )
        .await;
        fails(
            |rig| rig.docker_script("tag_fails", ""),
            "tagging the build as actions-runner:local",
        )
        .await;
    });
}

#[test]
fn a_failed_shutdown_or_rename_deletes_the_build_vm_and_keeps_the_old_golden_vm() {
    big_stack(|| async {
        fails(
            |rig| {
                rig.fake.script("stop_ignored", "");
                rig.timings.shutdown_grace = Duration::from_secs(1);
                rig.timings.shutdown_budget = Duration::from_secs(3);
                rig.timings.cleanup = Duration::from_secs(4);
            },
            "shutting the build VM down failed",
        )
        .await;
        let rig = fails(
            |rig| rig.fake.script("fail-rename", "rename went wrong"),
            "rename went wrong",
        )
        .await;
        assert_eq!(rig.calls_starting("rename ").len(), 1);
    });
}

#[test]
fn a_key_with_another_fingerprint_fails_before_anything_is_installed() {
    big_stack(|| async {
        let wrong = REAL_LISTING.replace(
            DOCKER_KEY_FINGERPRINT,
            "0000000000000000000000000000000000000000",
        );
        let rig = fails(
            |rig| {
                rig.fake
                    .script("exec_stdout-marshal-list-key", wrong.trim_end())
            },
            "the Docker signing key has the fingerprint 0000000000000000000000000000000000000000",
        )
        .await;
        assert!(
            rig.position("exec marshal-install-docker").is_none(),
            "{:?}",
            rig.calls()
        );
        assert!(rig.position("exec marshal-install-launcher").is_none());
    });
}

#[test]
fn an_empty_key_listing_fails_before_anything_is_installed() {
    big_stack(|| async {
        let rig = fails(
            |rig| {
                std::fs::remove_file(rig.fake.dir.path().join("exec_stdout-marshal-list-key"))
                    .unwrap()
            },
            "it is empty",
        )
        .await;
        assert!(
            rig.position("exec marshal-install-docker").is_none(),
            "{:?}",
            rig.calls()
        );
    });
}

#[test]
fn a_stop_at_each_point_deletes_the_build_vm_within_the_stop_sequence_and_keeps_the_old_golden_vm()
{
    big_stack(|| async {
        let points = [
            ("hang-pull", None, "pull "),
            ("hang-clone", None, "clone "),
            (
                "hang-exec-marshal-install-launcher",
                None,
                "exec marshal-install-launcher",
            ),
            ("build_hangs", Some(""), "docker build"),
        ];
        for (hang, docker_value, wait_for) in points {
            let rig = Rig::new();
            if docker_value.is_some() {
                rig.docker_script(hang, "");
            } else {
                rig.fake.script(hang, "");
            }
            let (stop, receiver) = tokio::sync::watch::channel(None);
            let started = Instant::now();
            let stopper = async {
                rig.wait_for_call(wait_for, Duration::from_secs(10)).await;
                let stopped = Instant::now();
                stop.send_replace(Some(tokio::time::Instant::now()));
                stopped
            };
            let (outcome, stopped) = tokio::join!(rig.run_until(receiver), stopper);
            let error = outcome.unwrap_err();
            assert!(error.contains("the build was stopped"), "{hang}: {error}");
            assert!(stopped.elapsed() < Duration::from_secs(28), "{hang}");
            assert!(started.elapsed() < Duration::from_secs(40), "{hang}");
            assert_eq!(rig.build_vms(), Vec::<String>::new(), "{hang}");
            assert_eq!(rig.golden_vms(), [old_golden()], "{hang}");
            assert!(rig.position("rename ").is_none(), "{hang}");
        }
    });
}

#[test]
fn a_stop_after_the_rename_keeps_the_new_golden_vm_and_ends_the_cleanup() {
    big_stack(|| async {
        let rig = Rig::new();
        rig.fake.script("hang-delete", "");
        let (stop, receiver) = tokio::sync::watch::channel(None);
        let stopper = async {
            rig.wait_for_call(&format!("delete {}", old_golden()), Duration::from_secs(10))
                .await;
            stop.send_replace(Some(tokio::time::Instant::now()));
        };
        let (outcome, ()) = tokio::join!(rig.run_until(receiver), stopper);
        outcome.unwrap();
        let golden = rig.golden_vms();
        assert_eq!(golden.len(), 2, "{golden:?}");
        assert!(golden.contains(&old_golden()));
        assert!(rig.build_vms().is_empty());
        assert!(
            rig.log
                .lines()
                .iter()
                .any(|line| line.contains("The stop skipped removing the old golden VMs")),
            "{:?}",
            rig.log.lines()
        );
    });
}

/// runs the build with a `tart rename` that hangs, calls `while_hanging` with the golden
/// name the rename was given, then stops the build, and returns how the build ended.
async fn stopped_during_a_hanging_rename(
    rig: &Rig,
    while_hanging: impl FnOnce(&Rig, &str),
) -> Result<(), String> {
    rig.fake.script("hang-rename", "");
    let (stop, receiver) = tokio::sync::watch::channel(None);
    let stopper = async {
        rig.wait_for_call("rename ", Duration::from_secs(10)).await;
        let target = rig.calls_starting("rename ")[0]
            .split(' ')
            .nth(2)
            .unwrap()
            .to_string();
        while_hanging(rig, &target);
        stop.send_replace(Some(tokio::time::Instant::now()));
    };
    let (outcome, ()) = tokio::join!(rig.run_until(receiver), stopper);
    outcome
}

#[test]
fn a_stop_during_a_rename_that_did_not_take_effect_deletes_the_build_vm_and_keeps_the_old_golden_vm()
 {
    big_stack(|| async {
        let rig = Rig::new();
        let error = stopped_during_a_hanging_rename(&rig, |_, _| {})
            .await
            .unwrap_err();
        assert!(error.contains("the build was stopped"), "{error}");
        assert_eq!(rig.build_vms(), Vec::<String>::new());
        assert_eq!(rig.golden_vms(), [old_golden()]);
    });
}

#[test]
fn a_stop_during_a_rename_that_took_effect_keeps_the_new_golden_vm() {
    big_stack(|| async {
        let rig = Rig::new();
        stopped_during_a_hanging_rename(&rig, |rig, golden| {
            rig.fake.seed(&[
                (&old_golden(), "stopped", "local"),
                (golden, "stopped", "local"),
            ]);
        })
        .await
        .unwrap();
        let golden = rig.golden_vms();
        assert_eq!(golden.len(), 2, "{golden:?}");
        assert_eq!(rig.build_vms(), Vec::<String>::new());
    });
}

#[test]
fn a_stop_during_a_rename_with_no_listing_is_a_commit_unknown_that_keeps_the_build_vm() {
    big_stack(|| async {
        let rig = Rig::new();
        let error = stopped_during_a_hanging_rename(&rig, |rig, _| {
            rig.fake.script("fail-list", "listing went wrong");
        })
        .await
        .unwrap_err();
        assert!(error.starts_with("commit unknown:"), "{error}");
        assert_eq!(rig.build_vms().len(), 1, "{:?}", rig.build_vms());
    });
}

#[test]
fn the_rename_client_has_ended_before_the_listing_that_decides_the_commit() {
    big_stack(|| async {
        let rig = Rig::new();
        rig.fake.script("hang-rename", "");
        let (stop, receiver) = tokio::sync::watch::channel(None);
        let stopper = async {
            rig.wait_for_call("rename ", Duration::from_secs(10)).await;
            let rename = rig.position("rename ").unwrap();
            let listings = rig.calls_starting("list ").len();
            rig.fake.script("hang-list", "");
            stop.send_replace(Some(tokio::time::Instant::now()));
            while rig.calls_starting("list ").len() == listings {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            rig.fake.ended_within(rename, Duration::from_secs(2))
        };
        let (outcome, rename_ended) = tokio::join!(rig.run_until(receiver), stopper);
        assert!(
            rename_ended,
            "the rename was still running during the listing"
        );
        assert!(outcome.unwrap_err().starts_with("commit unknown:"));
    });
}

/// the first `docker build` the fake engine in the VMs received, its `built` label.
fn built_label_of(rig: &Rig) -> u64 {
    let directory = rig.fake.dir.path().join("docker");
    let mut number = 0;
    while let Ok(arguments) =
        std::fs::read_to_string(directory.join(format!("inv-{number:06}.argv")))
    {
        let words: Vec<&str> = arguments.split('\0').collect();
        if words.first() == Some(&"build") {
            let prefix = format!("{BUILT_LABEL}=");
            return words
                .iter()
                .find_map(|word| word.strip_prefix(&prefix))
                .unwrap()
                .parse()
                .unwrap();
        }
        number += 1;
    }
    panic!("the engine received no docker build");
}

#[test]
fn the_golden_vm_is_stamped_with_the_time_of_the_image_build_not_of_the_start() {
    big_stack(|| async {
        let mut rig = Rig::new();
        rig.timings.engine_wait = Duration::from_secs(20);
        // the engine does not answer for about 2.5 s, as a slow provisioning does
        rig.docker_script("info_failures", "50");
        let started = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        rig.run().await.unwrap();
        let built = built_label_of(&rig);
        assert!(built >= started + 2, "{built} {started}");
        let golden = rig
            .golden_vms()
            .into_iter()
            .map(|name| GoldenVmName::parse(&name).unwrap())
            .find(|golden| golden.built_seconds != OLD_BUILT_SECONDS)
            .unwrap();
        assert_eq!(golden.built_seconds, built);
    });
}

#[test]
fn a_leftover_build_vm_is_deleted_before_the_pull_and_nothing_else_is() {
    big_stack(|| async {
        let rig = Rig::new();
        rig.fake.seed(&[
            (&old_golden(), "stopped", "local"),
            (LEFTOVER, "stopped", "local"),
            ("marshal-build-linux-20240101000000001", "running", "local"),
            ("marshal-build-linux-not-a-stamp", "stopped", "local"),
            ("marshal-job-pool-1-20240101000000000", "stopped", "local"),
            (UNRELATED, "stopped", "OCI"),
        ]);
        rig.run().await.unwrap();
        let local = rig.local_vms();
        assert!(!local.contains(&LEFTOVER.to_string()));
        assert!(!local.contains(&"marshal-build-linux-20240101000000001".to_string()));
        assert!(local.contains(&"marshal-build-linux-not-a-stamp".to_string()));
        assert!(local.contains(&"marshal-job-pool-1-20240101000000000".to_string()));
        assert!(rig.oci_images().contains(&UNRELATED.to_string()));
        let calls = rig.calls();
        assert_in_order(
            &calls,
            &[
                &format!("delete {LEFTOVER}"),
                &format!("pull {BASE_REFERENCE}"),
                "clone ",
            ],
        );
    });
}

#[test]
fn the_commit_deletes_every_other_golden_vm_and_only_the_retired_pins_from_the_cache() {
    big_stack(|| async {
        let mut rig = Rig::new();
        let other = GoldenVmName {
            golden_version: 2,
            contract_version: CONTRACT_VERSION + 1,
            runner_version: "2.1.0".to_string(),
            built_seconds: 1_800_000_000,
        }
        .to_string();
        rig.fake.seed(&[
            (&old_golden(), "stopped", "local"),
            (&other, "stopped", "local"),
            ("unrelated-vm", "stopped", "local"),
            (RETIRED, "stopped", "OCI"),
            (UNRELATED, "stopped", "OCI"),
            ("ghcr.io/cirruslabs/ubuntu:24.04", "stopped", "OCI"),
        ]);
        // a list that wrongly held the current pin still never deletes it
        rig.retired = vec![RETIRED, BASE_REFERENCE];
        rig.run().await.unwrap();
        let golden = rig.golden_vms();
        assert_eq!(golden.len(), 1, "{golden:?}");
        assert!(GoldenVmName::parse(&golden[0]).unwrap().matches_this_cli());
        assert!(rig.local_vms().contains(&"unrelated-vm".to_string()));
        let mut cache = rig.oci_images();
        cache.sort();
        let mut expected = vec![
            BASE_REFERENCE.to_string(),
            UNRELATED.to_string(),
            "ghcr.io/cirruslabs/ubuntu:24.04".to_string(),
        ];
        expected.sort();
        assert_eq!(cache, expected);
        let calls = rig.calls();
        assert_in_order(
            &calls,
            &[
                "rename ",
                &format!("delete {other}"),
                &format!("delete {RETIRED}"),
            ],
        );
        assert!(!RETIRED_BASE_PINS.contains(&BASE_REFERENCE));
    });
}

#[test]
fn a_failed_deletion_after_the_commit_is_a_warning_and_the_build_still_succeeds() {
    big_stack(|| async {
        let rig = Rig::new();
        rig.fake.script("fail-delete", "cannot delete");
        rig.run().await.unwrap();
        let golden = rig.golden_vms();
        assert_eq!(golden.len(), 2, "{golden:?}");
        let warnings: Vec<String> = rig
            .log
            .lines()
            .into_iter()
            .filter(|line| line.starts_with("WARN: "))
            .collect();
        assert!(
            warnings
                .iter()
                .any(|line| line.contains(&old_golden()) && line.contains("could not be deleted")),
            "{warnings:?}"
        );
    });
}

#[test]
fn a_missing_softnet_is_refused_before_any_vm_exists() {
    big_stack(|| async {
        let rig = Rig::without_softnet();
        let error = rig.run().await.unwrap_err();
        assert!(error.contains("Softnet was not found on PATH"), "{error}");
        assert!(
            error.contains("\"Installing Tart and Softnet\" section"),
            "{error}"
        );
        assert!(!error.contains("golden VM is unchanged"), "{error}");
        for forbidden in ["pull ", "clone ", "run ", "exec "] {
            assert!(
                rig.position(forbidden).is_none(),
                "{forbidden}: {:?}",
                rig.calls()
            );
        }
        assert_eq!(rig.local_vms(), [old_golden()]);
    });
}

#[test]
fn a_missing_tart_and_a_softnet_that_is_not_set_up_are_refused_before_any_vm_exists() {
    big_stack(|| async {
        let mut rig = Rig::new();
        let mut config = rig.fake.client.config().clone();
        config.program = "/nonexistent/marshal-test-tart".into();
        rig.fake.client = marshal_cli::runtime::tart::TartClient::new(config);
        let error = rig.run().await.unwrap_err();
        assert!(error.contains("Tart could not be started"), "{error}");
        assert!(
            error.contains("\"Installing Tart and Softnet\" section"),
            "{error}"
        );

        let mut rig = Rig::new();
        rig.owner_mode = marshal_cli::macos::softnet::OwnerMode {
            uid: 501,
            mode: 0o4755,
        };
        let error = rig.run().await.unwrap_err();
        assert!(error.contains("not root"), "{error}");
        assert!(error.contains("\"Setting Up Softnet\" section"), "{error}");
        assert!(rig.position("run ").is_none());
        assert!(rig.position("clone ").is_none());
    });
}

#[test]
fn the_axmc_profile_with_a_missing_tart_or_softnet_exits_1_before_any_vm_boots() {
    let rig = Rig::new();
    let config = rig.config("axmc-defaults.json");
    let mut missing_tart = rig.command(&["build", "--profile", "axmc", "--config"]);
    let output = missing_tart
        .arg(&config)
        .env("MARSHAL_TEST_TART", "/nonexistent/marshal-test-tart")
        .output()
        .unwrap();
    let (_, stderr) = texts(&output);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains("Tart could not be started"), "{stderr}");
    assert!(
        stderr.contains("\"Installing Tart and Softnet\" section"),
        "{stderr}"
    );

    let bare = support::softnet::Host::new();
    bare.directory("bin");
    let mut missing_softnet = rig.command(&["build", "--profile", "axmc", "--config"]);
    let output = missing_softnet
        .arg(&config)
        .env("PATH", bare.path(&["bin"]))
        .output()
        .unwrap();
    let (_, stderr) = texts(&output);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains("Softnet was not found on PATH"), "{stderr}");
    assert_eq!(rig.calls(), ["--version"], "only the version read ran");
    assert_eq!(rig.local_vms(), [old_golden()]);
}

#[test]
fn marshal_build_under_the_vm_shim_makes_the_golden_vm_named_by_the_default_image() {
    let rig = Rig::new();
    let config = rig.config("axmc-defaults.json");
    let mut spawned = rig.spawn_build(&config);
    assert_eq!(
        spawned.finish(Duration::from_secs(60)),
        Some(0),
        "{}",
        spawned.stderr()
    );
    let stderr = spawned.stderr();
    assert!(
        stderr.contains("Building actions-runner:local."),
        "{stderr}"
    );
    let golden = rig.golden_vms();
    assert_eq!(golden.len(), 1, "{golden:?}");
    assert!(GoldenVmName::parse(&golden[0]).unwrap().matches_this_cli());
    assert_eq!(rig.build_vms(), Vec::<String>::new());
    let tag = rig
        .fake
        .invocations()
        .into_iter()
        .map(|call| call.arguments)
        .find(|arguments| arguments.iter().any(|word| word == "tag"))
        .unwrap();
    assert_eq!(tag.last().map(String::as_str), Some("actions-runner:local"));
    let path = rig.path.to_string_lossy().into_owned();
    for call in rig.fake.invocations() {
        assert_eq!(call.no_auto_prune.as_deref(), Some("1"));
        assert_eq!(call.path.as_deref(), Some(path.as_str()));
    }
    let validated = rig
        .command(&["validate", "--profile", "axmc", "--config"])
        .arg(&config)
        .output()
        .unwrap();
    let (stdout, _) = texts(&validated);
    assert!(!stdout.contains("actions-runner:local"), "{stdout}");
}

#[test]
fn marshal_build_names_the_image_the_configuration_gives() {
    let rig = Rig::new();
    let config = rig.config("axmc-image-name.json");
    let mut spawned = rig.spawn_build(&config);
    assert_eq!(
        spawned.finish(Duration::from_secs(60)),
        Some(0),
        "{}",
        spawned.stderr()
    );
    let tag = rig
        .fake
        .invocations()
        .into_iter()
        .map(|call| call.arguments)
        .find(|arguments| arguments.iter().any(|word| word == "tag"))
        .unwrap();
    assert_eq!(tag.last().map(String::as_str), Some("example-image:tag"));
}

#[test]
fn an_interrupt_stops_the_build_within_30_seconds_and_keeps_the_old_golden_vm() {
    let rig = Rig::new();
    rig.fake.script("hang-exec-marshal-install-launcher", "");
    let config = rig.config("axmc-defaults.json");
    let mut spawned = rig.spawn_build(&config);
    wait_for_call_blocking(&rig, "exec marshal-install-launcher");
    let sent = Instant::now();
    send_signal(spawned.child.id(), "INT");
    assert_eq!(
        spawned.finish(Duration::from_secs(30)),
        Some(1),
        "{}",
        spawned.stderr()
    );
    assert!(sent.elapsed() < Duration::from_secs(30));
    assert!(
        spawned.stderr().contains("the build was stopped"),
        "{}",
        spawned.stderr()
    );
    assert_eq!(rig.build_vms(), Vec::<String>::new());
    assert_eq!(rig.golden_vms(), [old_golden()]);
}

#[test]
fn a_cli_that_dies_mid_build_leaves_its_build_vm_to_the_shim() {
    let rig = Rig::new();
    rig.fake.script("hang-exec-marshal-install-launcher", "");
    let config = rig.config("axmc-defaults.json");
    let mut spawned = rig.spawn_build(&config);
    wait_for_call_blocking(&rig, "exec marshal-install-launcher");
    assert_eq!(rig.build_vms().len(), 1);
    spawned.child.kill().unwrap();
    let _ = spawned.child.wait();
    let started = Instant::now();
    while !rig.build_vms().is_empty() {
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "the shim left {:?}",
            rig.build_vms()
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(rig.golden_vms(), [old_golden()]);
}

fn wait_for_call_blocking(rig: &Rig, prefix: &str) {
    let started = Instant::now();
    while rig.position(prefix).is_none() {
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "no call starting {prefix:?}; the calls: {:?}",
            rig.calls()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}
