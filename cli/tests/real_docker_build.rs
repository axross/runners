//! `build` against a real Docker daemon, on Linux only.
//!
//! The test is `#[ignore]`d so a plain `cargo test` never needs a daemon. CI runs it
//! in its own step, `cargo test --locked --test real_docker_build -- --ignored`. It
//! builds the debug-only test definition, `tests/build-definition`, which is the
//! runner image's base and contract labels and nothing else, so the step shows
//! what a real builder does with `build`'s context and labels without building the
//! whole image. The test fails when Docker is missing: a step that cannot run the
//! test must not look as if it passed.
#![cfg(target_os = "linux")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod support;

use std::collections::BTreeMap;
use std::process::{Command, Stdio};

use support::{BINARY, TempDir};

const REPOSITORY: &str = "runners-build-test";

/// Removes every image of the test's repository, whatever way the test ends.
struct Cleanup;

impl Cleanup {
    /// The repository's images as `repository:tag`, or `None` when `docker image ls`
    /// could not run or failed.
    fn try_images() -> Option<Vec<String>> {
        let output = Command::new("docker")
            .args([
                "image",
                "ls",
                "--filter",
                &format!("reference={REPOSITORY}"),
                "--format",
                "{{.Repository}}:{{.Tag}}",
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

    fn images() -> Vec<String> {
        Self::try_images().expect("docker image ls failed")
    }
}

impl Drop for Cleanup {
    // a panic here while another unwinds would abort, so a failed listing is empty.
    fn drop(&mut self) {
        let images = Self::try_images().unwrap_or_default();
        if !images.is_empty() {
            let _ = Command::new("docker")
                .args(["image", "rm", "--force"])
                .args(&images)
                .output();
        }
    }
}

#[test]
#[ignore = "needs a Docker daemon; CI runs it on Linux"]
fn build_tags_the_name_with_all_five_labels_and_leaves_no_temporary_tag() {
    let info = Command::new("docker")
        .arg("info")
        .output()
        .expect("docker is not installed");
    assert!(
        info.status.success(),
        "docker info failed: {}",
        String::from_utf8_lossy(&info.stderr)
    );
    let _cleanup = Cleanup;
    assert_eq!(
        Cleanup::images(),
        Vec::<String>::new(),
        "an earlier run left images"
    );

    let image = format!("{REPOSITORY}:ci");
    let root = TempDir::new();
    let config = root.write(
        "host.json",
        format!(
            r#"{{"imageName":"{image}","repositories":[{{"owner":"o","repository":"r",
            "name":"r","slots":1,"tokenPath":"C:\\r.token","volumes":[]}}]}}"#
        )
        .as_bytes(),
    );
    let definition =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/build-definition");
    let started = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let output = Command::new(BINARY)
        .args(["build", "--profile", "axpc", "--config"])
        .arg(&config)
        .current_dir(root.path())
        .env("RUNNERS_TEST_ROOT", root.path())
        .env("RUNNERS_TEST_BUILD_DEFINITION", &definition)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(0), "{stderr}");

    let keys = [
        "contract",
        "runner-version",
        "runner-released",
        "image",
        "built",
    ];
    let format: String = keys
        .iter()
        .map(|key| format!("{{{{index .Config.Labels \"com.github.axross.runners.{key}\"}}}}\n"))
        .collect();
    let inspect = Command::new("docker")
        .args(["image", "inspect", "--format", &format, &image])
        .output()
        .unwrap();
    assert!(inspect.status.success(), "{image} was not built");
    let inspected = String::from_utf8_lossy(&inspect.stdout);
    let labels: BTreeMap<&str, &str> = keys.into_iter().zip(inspected.lines()).collect();
    let label = |key: &str| labels.get(key).copied();
    assert_eq!(label("contract"), Some("1"), "{labels:?}");
    assert_eq!(label("runner-version"), Some("2.337.0"), "{labels:?}");
    assert_eq!(label("runner-released"), Some("2026-08-26"), "{labels:?}");
    assert_eq!(label("image"), Some("actions-runner"), "{labels:?}");
    let built: u64 = label("built")
        .unwrap()
        .parse()
        .expect("the built label is not a time");
    assert!(built >= started, "built {built}, started {started}");

    assert_eq!(Cleanup::images(), [image], "a temporary tag remains");
}
