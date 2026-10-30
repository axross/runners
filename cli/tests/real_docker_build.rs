//! `build` against a real Docker daemon on Linux, with the debug-only definition in
//! `tests/build-definition`, which is only the runner image's base and contract
//! labels, so it shows what a real builder does with the context and labels without
//! building the whole image; then the container backend's check of the built image.
//! `#[ignore]`d so a plain `cargo test` never needs a daemon, and it fails when
//! Docker is missing, so a step that cannot run the test does not look as if it passed.
#![cfg(target_os = "linux")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod support;

use std::collections::BTreeMap;
use std::path::Path;
use std::process::{Command, Stdio};
use std::rc::Rc;

use marshal_cli::image::check::{ImageCheck, ImageLabels};
use marshal_cli::image::contract::CONTRACT_VERSION;
use marshal_cli::runtime::backend::Backend;
use marshal_cli::runtime::clock::SystemClock;
use marshal_cli::runtime::docker::{DockerBackend, DockerConfig};
use marshal_cli::runtime::log::{Log, StderrLog};
use support::{BINARY, TempDir};

const REPOSITORY: &str = "marshal-build-test";

const LABEL_PREFIX: &str = "com.github.axross.runners.";

/// the labels the definition's Dockerfile declares, keyed without
/// [`LABEL_PREFIX`].
fn declared_labels(definition: &Path) -> BTreeMap<String, String> {
    let text = std::fs::read_to_string(definition.join("Dockerfile")).unwrap();
    let declaration = text
        .lines()
        .find_map(|line| line.strip_prefix("LABEL "))
        .expect("the test definition declares no labels");
    declaration
        .split_whitespace()
        .filter_map(|pair| pair.split_once('='))
        .map(|(key, value)| {
            (
                key.strip_prefix(LABEL_PREFIX).unwrap_or(key).to_string(),
                value.trim_matches('"').to_string(),
            )
        })
        .collect()
}

struct Cleanup;

impl Cleanup {
    /// the repository's images as `repository:tag`, or `None` when `docker image ls`
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
fn build_tags_the_name_with_all_five_labels_leaves_no_temporary_tag_and_the_check_matches() {
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
        .env("MARSHAL_TEST_ROOT", root.path())
        .env("MARSHAL_TEST_BUILD_DEFINITION", &definition)
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
        .map(|key| format!("{{{{index .Config.Labels \"{LABEL_PREFIX}{key}\"}}}}\n"))
        .collect();
    let inspect = Command::new("docker")
        .args(["image", "inspect", "--format", &format, &image])
        .output()
        .unwrap();
    assert!(inspect.status.success(), "{image} was not built");
    let inspected = String::from_utf8_lossy(&inspect.stdout);
    let labels: BTreeMap<&str, &str> = keys.into_iter().zip(inspected.lines()).collect();
    let label = |key: &str| labels.get(key).copied();
    let declared = declared_labels(&definition);
    for key in ["contract", "runner-version", "runner-released"] {
        assert_eq!(
            label(key),
            declared.get(key).map(String::as_str),
            "{key}: {labels:?}"
        );
    }
    assert_eq!(label("image"), Some("actions-runner"), "{labels:?}");
    let built: u64 = label("built")
        .unwrap()
        .parse()
        .expect("the built label is not a time");
    assert!(built >= started, "built {built}, started {started}");

    assert_eq!(
        Cleanup::images(),
        std::slice::from_ref(&image),
        "a temporary tag remains"
    );

    let backend = DockerBackend::new(
        DockerConfig::default(),
        SystemClock,
        Rc::new(StderrLog::default()) as Rc<dyn Log>,
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    assert_eq!(
        runtime.block_on(backend.check_image(&image)),
        ImageCheck::Matching(ImageLabels {
            contract: Some(CONTRACT_VERSION.to_string()),
            built: Some(built.to_string()),
            runner_version: declared.get("runner-version").cloned(),
            runner_released: declared.get("runner-released").cloned(),
        })
    );
    assert_eq!(
        runtime.block_on(backend.check_image(&format!("{REPOSITORY}:absent"))),
        ImageCheck::Missing
    );
}
