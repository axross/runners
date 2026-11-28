//! the feed check's default embedded release-verification trust, built from isolated
//! source copies so a rotation never changes the checkout's app or release keys. no
//! signer is needed: the throwaway fixture keys' private halves were not kept.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

use marshal_feed::{release_address, sha256_hex};

const MSI: &str = "marshal-1.4.0-windows-x64.msi";

#[test]
fn the_release_trust_source_pins_the_release_signing_public_key() {
    assert_eq!(
        include_str!("../release-public-key.txt").trim_end(),
        "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDZGMzlFMzEzMUUyMUUxRTQKUldUazRTRWVFK001Yjl3MllpMWlKbzhpUGsyRTd3YW1NY04xTEl4RStZVDlYN25nMEtPbUhhZ3UK"
    );
}

struct Sandbox(PathBuf);

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// fixture text suitable for embedding in a feed or compiled trust source.
fn fixture(name: &str) -> String {
    std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name),
    )
    .unwrap()
    .trim_end()
    .to_string()
}

/// an isolated build of the feed check whose embedded trust comes from `root`, not the
/// checkout.
fn build(root: &Path) -> Output {
    Command::new(env!("CARGO"))
        .args([
            "build",
            "--offline",
            "--package",
            "marshal-feed",
            "--bin",
            "marshal-feed-check",
        ])
        .current_dir(root)
        .env("CARGO_TARGET_DIR", root.join("target"))
        .output()
        .unwrap()
}

/// keeps compiler failures distinct from the feed refusals under test.
fn assert_build(root: &Path) {
    let output = build(root);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// checks the publication decision and diagnostic through default trust, with no key override.
fn checked(root: &Path, signature: &str, address: &str, accepts: bool, diagnostic: &str) {
    let feed = serde_json::json!({
        "version": "1.4.0",
        "platforms": { "windows-x86_64": { "url": address, "signature": fixture(signature) } }
    });
    std::fs::write(root.join("latest.json"), feed.to_string()).unwrap();
    let output = Command::new(root.join("target/debug").join(format!(
        "marshal-feed-check{}",
        std::env::consts::EXE_SUFFIX
    )))
    .args([
        "latest.json",
        "1.4.0",
        "SHA256SUMS",
        &format!("windows-x86_64={MSI}"),
    ])
    .current_dir(root)
    .output()
    .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        output.status.code(),
        Some(if accepts { 0 } else { 1 }),
        "{stdout}\n{stderr}"
    );
    let text = if accepts { &stdout } else { &stderr };
    assert!(text.contains(diagnostic), "{text}");
}

/// a transition changes app trust before release trust; neither stage may relax
/// the other publication checks.
#[test]
fn the_default_release_trust_rotates_independently_of_the_app_key() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let sandbox = Sandbox(std::env::temp_dir().join(format!(
        "marshal-release-trust-{}-{nonce}",
        std::process::id()
    )));
    let root = &sandbox.0;
    let source = Path::new(env!("CARGO_MANIFEST_DIR"));
    let feed = root.join("crates/feed");
    std::fs::create_dir_all(feed.join("src")).unwrap();
    std::fs::create_dir_all(root.join("crates/app")).unwrap();
    let workspace = std::fs::read_to_string(source.join("../../Cargo.toml")).unwrap();
    let workspace = workspace
        .lines()
        .map(|line| {
            if line.starts_with("members =") {
                "members = [\"crates/feed\"]"
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(root.join("Cargo.toml"), workspace).unwrap();
    for name in ["Cargo.lock", "rust-toolchain.toml"] {
        std::fs::copy(source.join("../..").join(name), root.join(name)).unwrap();
    }
    for name in ["Cargo.toml", "src/main.rs", "src/lib.rs"] {
        std::fs::copy(source.join(name), feed.join(name)).unwrap();
    }
    let mut app: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(source.join("../app/tauri.conf.json")).unwrap(),
    )
    .unwrap();
    app["plugins"]["updater"]["pubkey"] = fixture("rotation-key.pub").into();
    std::fs::write(root.join("crates/app/tauri.conf.json"), app.to_string()).unwrap();
    let bytes = std::fs::read(source.join("tests/fixtures/release-file.bin")).unwrap();
    std::fs::write(root.join(MSI), &bytes).unwrap();
    std::fs::write(
        root.join("SHA256SUMS"),
        format!("{}  {MSI}\n", sha256_hex(&bytes)),
    )
    .unwrap();
    let trust = feed.join("release-public-key.txt");
    std::fs::write(&trust, fixture("key.pub")).unwrap();
    let address = release_address("1.4.0", MSI);

    assert_build(root);
    checked(
        root,
        "signed-1.4.0.sig",
        &address,
        true,
        "with the release-verification key",
    );
    checked(
        root,
        "rotation-1.4.0.sig",
        &address,
        false,
        "the signature does not verify",
    );
    checked(
        root,
        "unrelated-1.4.0.sig",
        &address,
        false,
        "the signature does not verify",
    );
    checked(
        root,
        "signed-1.3.0.sig",
        &address,
        false,
        "the signature names version 1.3.0",
    );
    checked(
        root,
        "signed-1.4.0.sig",
        "https://example.invalid/update.msi",
        false,
        "entry's address",
    );
    std::fs::write(root.join(MSI), b"changed").unwrap();
    checked(
        root,
        "signed-1.4.0.sig",
        &address,
        false,
        "the build recorded",
    );
    checked(
        root,
        "signed-1.4.0.sig",
        &address,
        false,
        "the signature does not verify",
    );
    std::fs::write(root.join(MSI), &bytes).unwrap();

    std::fs::write(&trust, fixture("rotation-key.pub")).unwrap();
    assert_build(root);
    checked(
        root,
        "rotation-1.4.0.sig",
        &address,
        true,
        "with the release-verification key",
    );
    checked(
        root,
        "signed-1.4.0.sig",
        &address,
        false,
        "the signature does not verify",
    );

    checked(
        root,
        "unrelated-1.4.0.sig",
        &address,
        false,
        "the signature does not verify",
    );
    std::fs::write(&trust, fixture("other-key.pub")).unwrap();
    assert_build(root);
    for signature in [
        "signed-1.4.0.sig",
        "rotation-1.4.0.sig",
        "unrelated-1.4.0.sig",
    ] {
        checked(
            root,
            signature,
            &address,
            false,
            "the signature does not verify",
        );
    }
    for malformed in ["", "not base64", "bm90IGEgbWluaXNpZ24ga2V5"] {
        std::fs::write(&trust, malformed).unwrap();
        assert_build(root);
        checked(
            root,
            "rotation-1.4.0.sig",
            &address,
            false,
            "the public key",
        );
    }
    std::fs::remove_file(&trust).unwrap();
    let output = build(root);
    assert!(
        !output.status.success(),
        "a missing release trust source must fail the build"
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("release-public-key.txt"));
}
