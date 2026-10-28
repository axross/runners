//! the container backend against a real Docker daemon, on Linux only; the tests are
//! `#[ignore]`d so a plain `cargo test` never needs a daemon, and each fails when
//! Docker or the image `MARSHAL_REAL_DOCKER_IMAGE` names is missing.
#![cfg(target_os = "linux")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::process::Command;
use std::rc::Rc;

use marshal_cli::diagnostics::exporter::COMPLETE_TEXT;
use marshal_cli::diagnostics::identity::IDENTITY_HEADER;
use marshal_cli::runtime::backend::{
    Backend, ContainerSpec, CpuRequest, Deadlines, DiagnosticRecord, ExportOutcome, Mount,
};
use marshal_cli::runtime::clock::SystemClock;
use marshal_cli::runtime::docker::{DockerBackend, DockerConfig};
use marshal_cli::runtime::log::StderrLog;
use marshal_cli::runtime::secret::Secret;
use marshal_cli::validate::PlannedVolume;

struct Cleanup {
    pool: String,
    volume: String,
}

impl Cleanup {
    /// the pool's containers, or `None` when `docker ps` could not run or failed.
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

/// the image `MARSHAL_REAL_DOCKER_IMAGE` names, once Docker answers and holds it.
fn required_image() -> String {
    let image = std::env::var("MARSHAL_REAL_DOCKER_IMAGE")
        .expect("MARSHAL_REAL_DOCKER_IMAGE must name a local image, such as the runner base stage");
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
    image
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "needs a Docker daemon and MARSHAL_REAL_DOCKER_IMAGE; CI runs it in its own step"]
async fn a_container_and_the_cpu_probe_run_through_the_backend_and_leave_nothing_behind() {
    let image = required_image();

    let pool = format!("marshal-it-{}", std::process::id());
    let volume = format!("{pool}-cache");
    let cleanup = Cleanup {
        pool: pool.clone(),
        volume: volume.clone(),
    };
    let backend = DockerBackend::new(
        DockerConfig::default(),
        SystemClock,
        Rc::new(StderrLog::default()),
    );

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
        diagnostic: None,
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

/// the container writes a small `metrics.txt` where the collector writes it.
#[tokio::test(flavor = "current_thread")]
#[ignore = "needs a Docker daemon and MARSHAL_REAL_DOCKER_IMAGE; CI runs it in its own step"]
async fn a_real_copy_stream_is_exported_into_a_bundle_and_the_container_removed() {
    use std::os::unix::fs::PermissionsExt;

    let image = required_image();
    let pool = format!("marshal-it-export-{}", std::process::id());
    let cleanup = Cleanup {
        pool: pool.clone(),
        volume: format!("{pool}-unused"),
    };
    let name = format!("{pool}-1-20240305060708009");
    let ran = Command::new("docker")
        .args([
            "run",
            "--name",
            &name,
            "--label",
            "runners.diagnostic-lifecycle=1",
            "--cpus",
            "1",
            "--memory",
            "1g",
            "--memory-swap",
            "1g",
            &image,
            "sh",
            "-c",
            "mkdir -p /tmp/runner-diagnostics && printf 'sample=1\\nfinal=observed\\n' > /tmp/runner-diagnostics/metrics.txt",
        ])
        .output()
        .unwrap();
    assert!(
        ran.status.success(),
        "{}",
        String::from_utf8_lossy(&ran.stderr)
    );

    let sink = std::env::temp_dir().join(format!("{pool}-sink"));
    std::fs::create_dir(&sink).unwrap();
    std::fs::set_permissions(&sink, std::fs::Permissions::from_mode(0o700)).unwrap();
    let sink = std::fs::canonicalize(sink).unwrap();
    let backend = DockerBackend::new(
        DockerConfig {
            exporter: Some(env!("CARGO_BIN_EXE_marshal").into()),
            ..DockerConfig::default()
        },
        SystemClock,
        Rc::new(StderrLog::default()),
    );
    let record = DiagnosticRecord {
        pool: pool.clone(),
        directory: sink.to_str().unwrap().to_string(),
        raw_records: false,
    };
    backend.retained().adopt(&name, &record);
    let claim = backend.retained().claim(&name).unwrap();
    let finalized = marshal_cli::runtime::diagnostic::finalize(
        &backend,
        &claim,
        Deadlines::from(tokio::time::Instant::now()),
    )
    .await;
    drop(claim);
    let bundles: Vec<std::path::PathBuf> = std::fs::read_dir(&sink)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    let read = |file: &str| std::fs::read_to_string(bundles[0].join(file)).unwrap();
    let outcome = (finalized.export, finalized.removed, bundles.len());
    let (identity, metrics, complete) = if bundles.is_empty() {
        Default::default()
    } else {
        (
            read("identity.txt"),
            read("metrics.txt"),
            read("complete.txt"),
        )
    };
    let _ = std::fs::remove_dir_all(&sink);
    assert_eq!(outcome, (ExportOutcome::Complete, true, 1));
    assert_eq!(metrics, "sample=1\nfinal=observed\n");
    assert_eq!(complete, COMPLETE_TEXT);
    let lines: Vec<&str> = identity.lines().collect();
    assert_eq!(lines.len(), 2, "{identity}");
    assert_eq!(lines[0], IDENTITY_HEADER);
    assert!(lines[1].starts_with("sha256:"), "{identity}");
    assert!(
        lines[1].contains("|1000000000|1073741824|1073741824|"),
        "{identity}"
    );
    assert_eq!(
        cleanup.leftovers(),
        Vec::<String>::new(),
        "the exported container was not removed"
    );
}
