//! The container backend against a real Docker daemon, on Linux only.
//!
//! The test is `#[ignore]`d so a plain `cargo test` never needs a daemon. CI runs it
//! in its own step, `cargo test --locked --test real_docker -- --ignored`, after
//! building the runner image's base stage and naming it in
//! `RUNNERS_REAL_DOCKER_IMAGE`. The test fails when Docker or that image is
//! missing: a step that cannot run the test must not look as if it passed.
#![cfg(target_os = "linux")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::process::Command;
use std::rc::Rc;

use runners_cli::runtime::backend::{Backend, ContainerSpec, CpuRequest, Mount};
use runners_cli::runtime::clock::SystemClock;
use runners_cli::runtime::docker::{DockerBackend, DockerConfig};
use runners_cli::runtime::log::StderrLog;
use runners_cli::runtime::secret::Secret;
use runners_cli::validate::PlannedVolume;

/// Removes everything the test may have created, whatever way it ends.
struct Cleanup {
    pool: String,
    volume: String,
}

impl Cleanup {
    /// The pool's containers, or `None` when `docker ps` could not run or failed.
    fn try_leftovers(&self) -> Option<Vec<String>> {
        let output = Command::new("docker")
            .args([
                "ps",
                "-a",
                "--filter",
                &format!("name={}", self.pool),
                "--format",
                "{{.Names}}",
            ])
            .output()
            .ok()?;
        output.status.success().then(|| {
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .map(str::to_string)
                .collect()
        })
    }

    fn leftovers(&self) -> Vec<String> {
        self.try_leftovers().expect("docker ps failed")
    }
}

impl Drop for Cleanup {
    // a panic here while another unwinds would abort, so a failed listing is empty.
    fn drop(&mut self) {
        let names = self.try_leftovers().unwrap_or_default();
        if !names.is_empty() {
            let _ = Command::new("docker")
                .arg("rm")
                .arg("-f")
                .args(&names)
                .output();
        }
        let _ = Command::new("docker")
            .args(["volume", "rm", "-f", &self.volume])
            .output();
    }
}

fn cpu_list_is_well_formed(list: &str) -> bool {
    !list.is_empty()
        && list.split(',').all(|part| {
            part.split('-').count() <= 2
                && part
                    .split('-')
                    .all(|id| !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()))
        })
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "needs a Docker daemon and RUNNERS_REAL_DOCKER_IMAGE; CI runs it in its own step"]
async fn a_container_and_the_cpu_probe_run_through_the_backend_and_leave_nothing_behind() {
    let image = std::env::var("RUNNERS_REAL_DOCKER_IMAGE")
        .expect("RUNNERS_REAL_DOCKER_IMAGE must name a local image, such as the runner base stage");
    let info = Command::new("docker")
        .arg("info")
        .output()
        .expect("the docker client is required");
    assert!(
        info.status.success(),
        "a running Docker daemon is required: {}",
        String::from_utf8_lossy(&info.stderr)
    );
    let present = Command::new("docker")
        .args(["image", "inspect", &image])
        .output()
        .unwrap();
    assert!(
        present.status.success(),
        "the image {image} is not present locally"
    );

    let pool = format!("runners-it-{}", std::process::id());
    let volume = format!("{pool}-cache");
    let cleanup = Cleanup {
        pool: pool.clone(),
        volume: volume.clone(),
    };
    let backend = DockerBackend::new(DockerConfig::default(), SystemClock, Rc::new(StderrLog));

    backend.wait_until_available().await;
    let listing = backend.list_pool_containers(&pool).await.unwrap();
    assert!(listing.names.is_empty() && listing.diagnostic.is_empty());

    backend
        .prepare_volumes(
            &image,
            &[PlannedVolume {
                name: volume.clone(),
                mount_path: "/home/runner/.cache".to_string(),
            }],
        )
        .await
        .unwrap();

    // a leftover probe of the same name and label is removed before the new one runs.
    let probe = format!("{pool}-cpu-probe-1");
    let created = Command::new("docker")
        .args([
            "create",
            "--name",
            &probe,
            "--label",
            "runners.cpu-probe=1",
            &image,
            "true",
        ])
        .output()
        .unwrap();
    assert!(
        created.status.success(),
        "{}",
        String::from_utf8_lossy(&created.stderr)
    );
    let cpuset = backend
        .select_cpuset(&CpuRequest {
            probe_name: probe.clone(),
            image: image.clone(),
            count: 1,
            offset: 0,
        })
        .await
        .unwrap();
    assert!(cpu_list_is_well_formed(&cpuset), "{cpuset}");
    assert!(
        cleanup.leftovers().is_empty(),
        "the probe was not removed: {:?}",
        cleanup.leftovers()
    );

    // the launcher verifies the CPU set, so a container that starts proves the guard
    // saw the set it was given; its command stands in for the runner.
    let jit = Secret::new("not-a-real-jit-configuration".to_string());
    let mut spec = ContainerSpec {
        name: format!("{pool}-1-20240305060708009"),
        cpus: "1".to_string(),
        cpuset,
        memory_gb: 1,
        mounts: vec![Mount {
            volume,
            target: "/home/runner/.cache".to_string(),
        }],
        image,
        run_command: "true".to_string(),
    };
    assert_eq!(backend.run_container(&spec, &jit).await.unwrap(), 0);
    spec.name = format!("{pool}-1-20240305060709009");
    spec.run_command = "false".to_string();
    assert_eq!(backend.run_container(&spec, &jit).await.unwrap(), 1);
    assert!(backend.running_containers().is_empty());

    assert_eq!(
        cleanup.leftovers(),
        Vec::<String>::new(),
        "a container with the test's names remains"
    );
}
