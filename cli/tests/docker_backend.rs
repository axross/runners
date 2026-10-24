//! The container backend, run as real processes against the fake `docker` client.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod support;

use std::rc::Rc;
use std::time::{Duration, Instant};

use runners_cli::profile::Profile;
use runners_cli::runtime::backend::{Backend, ContainerSpec, CpuRequest, Mount, StopRequest};
use runners_cli::runtime::docker::DockerLimits;
use runners_cli::runtime::fake::FakeRegistrar;
use runners_cli::runtime::log::Log;
use runners_cli::runtime::pool::{pools_from_plan, prepare_pools};
use runners_cli::runtime::secret::Secret;
use runners_cli::runtime::worker::{run_slot, stop_channel};
use runners_cli::validate::{PlannedVolume, validate};
use support::docker::Fake;
use support::{TempDir, repository_root};

const PROBE_READER: &str = "while read -r key value; do if [ \"$key\" = Cpus_allowed_list: ]; then printf \"%s\\n\" \"$value\"; exit; fi; done < /proc/self/status; exit 1";

/// The launcher script, written out here independently of the code under test.
const LAUNCHER: &str = r#"set -eu
expected=$1
shift
allowed=
if [ ! -r /proc/self/status ]; then
  printf 'CPU affinity readback unavailable.\n' >&2
  exit 125
fi
while read -r key value; do
  if [ "$key" = Cpus_allowed_list: ]; then allowed=$value; break; fi
done < /proc/self/status
if [ "$allowed" != "$expected" ]; then
  printf 'CPU affinity readback mismatch; runner startup refused.\n' >&2
  exit 125
fi
printf 'CPU affinity verified: %s\n' "$allowed"
exec "$@""#;

fn words(text: &str) -> Vec<String> {
    text.split_whitespace().map(str::to_string).collect()
}

fn example_pools(scratch: &TempDir) -> Vec<runners_cli::runtime::pool::PoolSpec> {
    let path = repository_root().join("hosts/windows-docker-desktop/runner-host.example.json");
    let bytes = std::fs::read(path).unwrap();
    let plan = validate(&bytes, Profile::Axpc, None).outcome.unwrap();
    let mut pools = pools_from_plan(&plan).unwrap();
    for pool in &mut pools {
        pool.token_path = scratch.write(&format!("{}.token", pool.name), b"ghp_exampletoken\n");
    }
    pools
}

fn probe_calls(probe: &str) -> Vec<Vec<String>> {
    vec![
        words(&format!(
            "ps -a --filter name=^{probe}$ --filter label=runners.cpu-probe=1 --format {{{{.Names}}}}"
        )),
        {
            let mut run = words(&format!(
                "run --rm --pull never --network none --name {probe} --label runners.cpu-probe=1 actions-runner:local bash -c"
            ));
            run.push(PROBE_READER.to_string());
            run
        },
        words(&format!(
            "ps -a --filter name=^{probe}$ --filter label=runners.cpu-probe=1 --format {{{{.Names}}}}"
        )),
    ]
}

fn job_call(
    name: &str,
    cpus: &str,
    cpuset: &str,
    memory: u32,
    mounts: &[(&str, &str)],
    image: &str,
) -> Vec<String> {
    let mut call = words(&format!(
        "run --rm --pull never --name {name} --cpus {cpus} --cpuset-cpus {cpuset} --memory {memory}g --memory-swap {memory}g"
    ));
    for (volume, target) in mounts {
        call.push("--mount".to_string());
        call.push(format!("type=volume,source={volume},target={target}"));
    }
    call.extend(words(&format!(
        "-e ACTIONS_RUNNER_INPUT_JITCONFIG {image} bash -c"
    )));
    call.push(LAUNCHER.to_string());
    call.extend(words(&format!("-- {cpuset} /home/runner/run.sh")));
    call
}

#[tokio::test(flavor = "current_thread")]
async fn the_example_configuration_runs_the_powershell_contract() {
    let scratch = TempDir::new();
    let fake = Fake::new();
    let pools = example_pools(&scratch);
    // the PowerShell golden inventory: six CPUs, so the five positions wrap.
    fake.script("probe_cpus", "2,5-6,11,14-15");
    fake.script("run_exit", "1");

    let prepared = prepare_pools(&fake.backend, pools, &*fake.log)
        .await
        .unwrap();
    assert_eq!(prepared.len(), 2);

    let registrar = FakeRegistrar::default();
    for pool in &prepared {
        for slot in pool.slot_specs() {
            let (stop, receiver) = stop_channel();
            fake.clock.stop_after(fake.clock.sleeps().len() + 1, stop);
            run_slot(
                &slot,
                &fake.backend,
                &fake.clock,
                &registrar,
                &*fake.log,
                receiver,
            )
            .await;
        }
    }

    #[cfg(unix)]
    for invocation in fake.invocations() {
        assert_eq!(
            invocation.pgid,
            Some(invocation.pid),
            "{:?} is not its own process group leader",
            invocation.arguments
        );
        assert_ne!(invocation.pgid, Some(process_group_of(std::process::id())));
    }
    let calls: Vec<Vec<String>> = fake
        .invocations()
        .into_iter()
        .map(|invocation| invocation.arguments)
        .collect();
    let toolcache = |pool: &str| (format!("{pool}-toolcache"), "/opt/hostedtoolcache");
    let npm = |pool: &str| (format!("{pool}-npm"), "/home/runner/.npm");
    let one = [
        toolcache("example-repo-one"),
        ("example-repo-one-cargo".to_string(), "/home/runner/.cargo"),
        (
            "example-repo-one-rustup".to_string(),
            "/home/runner/.rustup",
        ),
        npm("example-repo-one"),
    ];
    let two = [toolcache("example-repo-two"), npm("example-repo-two")];

    let mut expected: Vec<Vec<String>> = vec![words("info")];
    for (pool, mounts) in [
        ("example-repo-one", &one[..]),
        ("example-repo-two", &two[..]),
    ] {
        expected.push(words(&format!(
            "ps -a --filter name={pool}- --format {{{{.Names}}}}"
        )));
        expected.push(words(&format!(
            "ps -a --filter name={pool}- --filter label=runners.diagnostic-lifecycle=1 --format {{{{.Names}}}}"
        )));
        for (volume, _) in mounts {
            expected.push(words(&format!("volume create {volume}")));
        }
        let mut chown = words("run --rm --pull never --user root");
        for (volume, target) in mounts {
            chown.push("--mount".to_string());
            chown.push(format!("type=volume,source={volume},target={target}"));
        }
        chown.extend(words("actions-runner:local chown runner:docker"));
        chown.extend(mounts.iter().map(|(_, target)| (*target).to_string()));
        expected.push(chown);
    }
    let mounts_of = |mounts: &[(String, &str)]| -> Vec<(String, String)> {
        mounts
            .iter()
            .map(|(volume, target)| (volume.clone(), (*target).to_string()))
            .collect()
    };
    let (one_mounts, two_mounts) = (mounts_of(&one), mounts_of(&two));
    let slots = [
        (
            "example-repo-one",
            1,
            "20240305060708009",
            "2",
            "2,5",
            8,
            &one_mounts,
        ),
        (
            "example-repo-one",
            2,
            "20240305060713009",
            "2",
            "6,11",
            8,
            &one_mounts,
        ),
        (
            "example-repo-two",
            1,
            "20240305060718009",
            "4",
            "2,5,14-15",
            16,
            &two_mounts,
        ),
    ];
    for (pool, slot, stamp, cpus, cpuset, memory, mounts) in slots {
        expected.extend(probe_calls(&format!("{pool}-cpu-probe-{slot}")));
        let refs: Vec<(&str, &str)> = mounts
            .iter()
            .map(|(v, t)| (v.as_str(), t.as_str()))
            .collect();
        expected.push(job_call(
            &format!("{pool}-{slot}-{stamp}"),
            cpus,
            cpuset,
            memory,
            &refs,
            "actions-runner:local",
        ));
    }
    assert_eq!(calls.len(), expected.len(), "{calls:#?}");
    for (index, (got, want)) in calls.iter().zip(&expected).enumerate() {
        assert_eq!(got, want, "call {index}");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn the_jit_configuration_reaches_only_the_run_client_through_its_environment() {
    let scratch = TempDir::new();
    let fake = Fake::new();
    let pools = example_pools(&scratch);
    fake.script("run_exit", "1");
    let registrar = FakeRegistrar::default();
    registrar
        .results
        .borrow_mut()
        .push_back(Ok("super-secret-jit-config".to_string()));
    let prepared = prepare_pools(&fake.backend, pools, &*fake.log)
        .await
        .unwrap();
    let slot = prepared[0].slot_specs().remove(0);
    let (stop, receiver) = stop_channel();
    fake.clock.stop_after(1, stop);
    run_slot(
        &slot,
        &fake.backend,
        &fake.clock,
        &registrar,
        &*fake.log,
        receiver,
    )
    .await;

    let invocations = fake.invocations();
    let with_jit: Vec<_> = invocations.iter().filter(|i| i.jit.is_some()).collect();
    assert_eq!(with_jit.len(), 1);
    assert_eq!(with_jit[0].jit.as_deref(), Some("super-secret-jit-config"));
    assert_eq!(with_jit[0].arguments[0], "run");
    for invocation in &invocations {
        for argument in &invocation.arguments {
            assert!(!argument.contains("super-secret-jit-config"));
            assert!(!argument.contains("ghp_exampletoken"));
        }
    }
    let seen = registrar.seen.borrow();
    assert_eq!(
        seen[0].token, "ghp_exampletoken",
        "the token was read from its file"
    );
    let lines = fake.log.lines().join("\n");
    assert!(!lines.contains("super-secret-jit-config") && !lines.contains("ghp_exampletoken"));
}

#[cfg(unix)]
fn process_group_of(pid: u32) -> u32 {
    let output = std::process::Command::new("ps")
        .args(["-o", "pgid=", "-p", &pid.to_string()])
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .unwrap()
}

#[cfg(unix)]
#[tokio::test(flavor = "current_thread")]
async fn a_held_container_client_runs_in_its_own_process_group() {
    let fake = Fake::new();
    fake.script("run_mode", "hold");
    let spec = ContainerSpec {
        name: "pg-2-20240305060708009".to_string(),
        cpus: "1".to_string(),
        cpuset: "0".to_string(),
        memory_gb: 1,
        mounts: Vec::new(),
        image: "img:tag".to_string(),
        run_command: "/home/runner/run.sh".to_string(),
    };
    let jit = Secret::new("jit".to_string());
    let held = fake.backend.run_container(&spec, &jit);
    let observe = async {
        let started = Instant::now();
        let pid = loop {
            if let Some(found) = fake.invocations().into_iter().find(|i| i.pid != 0) {
                break found.pid;
            }
            assert!(started.elapsed() < Duration::from_secs(20));
            tokio::time::sleep(Duration::from_millis(20)).await;
        };
        let group = process_group_of(pid);
        assert_eq!(
            fake.backend.running_containers(),
            std::slice::from_ref(&spec.name)
        );
        fake.backend
            .stop_containers(&StopRequest {
                names: vec![spec.name.clone()],
                grace: Duration::from_secs(15),
                budget: Duration::from_secs(20),
            })
            .await
            .unwrap();
        (pid, group)
    };
    let (_, (pid, group)) = tokio::join!(held, observe);
    assert_eq!(
        group, pid,
        "process_group(0) makes the client its own group leader"
    );
    assert_ne!(group, process_group_of(std::process::id()));
    assert!(fake.backend.running_containers().is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn container_output_is_forwarded_and_a_stop_is_one_call_with_the_grace() {
    let fake = Fake::new();
    let spec = ContainerSpec {
        name: "out-1-20240305060708009".to_string(),
        cpus: "1".to_string(),
        cpuset: "0".to_string(),
        memory_gb: 1,
        mounts: vec![Mount {
            volume: "v".to_string(),
            target: "/t".to_string(),
        }],
        image: "img:tag".to_string(),
        run_command: "/home/runner/run.sh".to_string(),
    };
    fake.script("run_exit", "7");
    let code = fake
        .backend
        .run_container(&spec, &Secret::new("jit".to_string()))
        .await
        .unwrap();
    assert_eq!(code, 7);
    let lines = fake.log.lines();
    assert!(
        lines.contains(&"INFO: fake runner started".to_string()),
        "{lines:?}"
    );
    assert!(lines.contains(&"INFO: fake runner warning".to_string()));

    fake.backend
        .stop_containers(&StopRequest {
            names: vec!["a".to_string(), "b".to_string()],
            grace: Duration::from_secs(15),
            budget: Duration::from_secs(20),
        })
        .await
        .unwrap();
    fake.backend
        .remove_containers(&["a".to_string(), "b".to_string()], Duration::from_secs(6))
        .await
        .unwrap();
    let calls = fake.invocations();
    assert_eq!(
        calls[calls.len() - 2].arguments,
        words("stop --time 15 a b")
    );
    assert_eq!(calls[calls.len() - 1].arguments, words("rm -f a b"));
    assert!(
        calls[calls.len() - 2..]
            .iter()
            .all(|call| call.jit.is_none())
    );
}

fn request(name: &str) -> CpuRequest {
    CpuRequest {
        probe_name: format!("{name}-cpu-probe-1"),
        image: "actions-runner:local".to_string(),
        count: 2,
        offset: 2,
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_leftover_probe_is_removed_first_and_the_probe_is_removed_after() {
    let fake = Fake::new();
    fake.script("containers", "p-cpu-probe-1\trunners.cpu-probe=1");
    fake.script("probe_cpus", "2,5-6,11,14-15");
    let set = fake.backend.select_cpuset(&request("p")).await.unwrap();
    assert_eq!(set, "6,11");
    let calls: Vec<Vec<String>> = fake
        .invocations()
        .into_iter()
        .map(|i| i.arguments)
        .collect();
    let mut expected = vec![
        probe_calls("p-cpu-probe-1")[0].clone(),
        words("rm -f p-cpu-probe-1"),
    ];
    expected.extend(probe_calls("p-cpu-probe-1").into_iter().skip(1));
    assert_eq!(calls, expected);
    assert!(fake.containers().is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn a_container_of_the_probes_name_without_the_label_is_never_removed() {
    let fake = Fake::new();
    fake.script("containers", "p-cpu-probe-1\tother=1");
    let error = fake.backend.select_cpuset(&request("p")).await;
    // the exact-name and label filter finds nothing, so nothing is removed and the
    // probe's own `docker run` is what fails on the name clash in a real engine.
    assert!(error.is_ok());
    assert!(fake.invocations().iter().all(|i| i.arguments[0] != "rm"));
    assert_eq!(fake.containers(), ["p-cpu-probe-1"]);
}

#[tokio::test(flavor = "current_thread")]
async fn probe_failures_never_yield_an_unrestricted_set() {
    for mode in ["fail", "garbage", "flood"] {
        let fake = Fake::new();
        fake.script("probe_mode", mode);
        assert!(
            fake.backend.select_cpuset(&request("p")).await.is_err(),
            "{mode}"
        );
        // the cleanup still ran after the probe.
        let calls = fake.invocations();
        assert_eq!(
            calls.last().unwrap().arguments,
            probe_calls("p-cpu-probe-1")[0],
            "{mode}"
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_probe_that_hangs_is_ended_at_its_budget_and_its_client_is_gone() {
    let limits = DockerLimits {
        probe_budget: Duration::from_millis(1500),
        client_termination: Duration::from_millis(500),
        ..DockerLimits::default()
    };
    let fake = Fake::with_limits(limits);
    fake.script("probe_mode", "hang");
    let started = Instant::now();
    assert!(fake.backend.select_cpuset(&request("p")).await.is_err());
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "{:?}",
        started.elapsed()
    );
    #[cfg(unix)]
    {
        let hung = fake
            .invocations()
            .into_iter()
            .find(|i| i.arguments.first().is_some_and(|a| a == "run"))
            .unwrap();
        let alive = || {
            std::process::Command::new("ps")
                .args(["-o", "stat=", "-p", &hung.pid.to_string()])
                .output()
                .map(|o| {
                    let stat = String::from_utf8_lossy(&o.stdout).trim().to_string();
                    !stat.is_empty() && !stat.starts_with('Z')
                })
                .unwrap()
        };
        let waited = Instant::now();
        while alive() && waited.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(!alive(), "the hung probe client was not ended");
    }
}

#[test]
fn the_default_probe_limits_are_thirty_seconds_five_seconds_and_sixty_four_kibibytes() {
    let limits = DockerLimits::default();
    assert_eq!(limits.probe_budget, Duration::from_secs(30));
    assert_eq!(limits.probe_cleanup_budget, Duration::from_secs(5));
    assert_eq!(limits.client_termination, Duration::from_secs(1));
    assert_eq!(limits.probe_output_bytes, 65_536);
}

#[tokio::test(flavor = "current_thread")]
async fn waiting_for_docker_polls_every_five_seconds_with_a_warning() {
    let fake = Fake::new();
    fake.script("info_failures", "2");
    fake.backend.wait_until_available().await;
    assert_eq!(
        fake.clock.sleeps(),
        [Duration::from_secs(5), Duration::from_secs(5)]
    );
    let calls = fake.invocations();
    assert_eq!(calls.len(), 3);
    assert!(calls.iter().all(|call| call.arguments == ["info"]));
    assert_eq!(
        fake.log.lines(),
        vec![
            "WARN: Docker Desktop is not responding yet (still starting, or not running) - waiting.";
            2
        ]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_missing_docker_client_counts_as_not_available_and_a_run_fails_cleanly() {
    let fake = Fake::new();
    let missing = {
        let config = runners_cli::runtime::docker::DockerConfig {
            program: "this-docker-client-does-not-exist".into(),
            ..Default::default()
        };
        runners_cli::runtime::docker::DockerBackend::new(
            config,
            fake.clock.clone(),
            fake.log.clone() as Rc<dyn Log>,
        )
    };
    let spec = ContainerSpec {
        name: "n".to_string(),
        cpus: "1".to_string(),
        cpuset: "0".to_string(),
        memory_gb: 1,
        mounts: Vec::new(),
        image: "i".to_string(),
        run_command: "c".to_string(),
    };
    let error = missing
        .run_container(&spec, &Secret::new("jit".to_string()))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("could not start"), "{error}");
    assert!(missing.running_containers().is_empty());
    assert!(missing.select_cpuset(&request("p")).await.is_err());
}

#[tokio::test(flavor = "current_thread")]
async fn volume_failures_are_reported_with_the_volume_name() {
    let fake = Fake::new();
    fake.script("volume_fails", "1");
    let volume = PlannedVolume {
        name: "pool-npm".to_string(),
        mount_path: "/home/runner/.npm".to_string(),
    };
    let error = fake
        .backend
        .prepare_volumes("img:tag", &[volume])
        .await
        .unwrap_err();
    assert!(
        error
            .to_string()
            .starts_with("Failed to create Docker volume 'pool-npm'"),
        "{error}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn stale_cleanup_through_docker_removes_only_own_unlabelled_containers() {
    let scratch = TempDir::new();
    let fake = Fake::new();
    let mut pools = example_pools(&scratch);
    pools.truncate(1);
    pools[0].volumes.clear();
    let own = "example-repo-one-1-20240101000000000";
    let evidence = "example-repo-one-2-20240101000000000";
    fake.script(
        "containers",
        &format!(
            "{own}\t\n{evidence}\trunners.diagnostic-lifecycle=1\nexample-repo-one-cpu-probe-1\trunners.cpu-probe=1\nexample-repo-two-1-20240101000000000\t\n"
        ),
    );
    let result = prepare_pools(&fake.backend, pools.clone(), &*fake.log).await;
    // the labelled container refuses the pool, so nothing can start.
    assert!(result.is_err());
    assert_eq!(
        fake.containers(),
        [
            evidence,
            "example-repo-one-cpu-probe-1",
            "example-repo-two-1-20240101000000000"
        ]
    );
    let removals: Vec<_> = fake
        .invocations()
        .into_iter()
        .filter(|i| i.arguments.first().is_some_and(|a| a == "rm"))
        .map(|i| i.arguments)
        .collect();
    assert_eq!(removals, [words(&format!("rm -f {own}"))]);

    // with the labelled container gone, the pool starts.
    fake.script("containers", &format!("{own}\t\n"));
    let prepared = prepare_pools(&fake.backend, pools, &*fake.log)
        .await
        .unwrap();
    assert_eq!(prepared.len(), 1);
    assert!(fake.containers().is_empty());
}
