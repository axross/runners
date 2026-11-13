//! the rules the app takes a release by, and the release run's feed check, against
//! throwaway keys and signatures (see `fixtures/README.md`).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use marshal_feed::{
    Refusal, Released, check_feed, download_allowed, newer, recorded_sha256, release_address,
    sha256_hex, verify,
};
use url::Url;

fn fixture(name: &str) -> String {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(path)
        .expect("a fixture")
        .trim()
        .to_string()
}

fn release_file() -> Vec<u8> {
    let path = format!(
        "{}/tests/fixtures/release-file.bin",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read(path).expect("the release file")
}

const MSI: &str = "marshal-1.4.0-windows-x64.msi";

#[test]
fn a_signature_made_for_the_version_verifies() {
    let bytes = release_file();
    assert_eq!(
        verify(
            &bytes,
            &fixture("signed-1.4.0.sig"),
            &fixture("key.pub"),
            "1.4.0"
        ),
        Ok(())
    );
    // the plugin compares the signed version as semver, so a leading v matches
    assert_eq!(
        verify(
            &bytes,
            &fixture("signed-1.4.0.sig"),
            &fixture("key.pub"),
            "v1.4.0"
        ),
        Ok(())
    );
}

#[test]
fn a_changed_file_or_another_key_does_not_verify() {
    let mut changed = release_file();
    if let Some(byte) = changed.last_mut() {
        *byte ^= 1;
    }
    assert!(matches!(
        verify(
            &changed,
            &fixture("signed-1.4.0.sig"),
            &fixture("key.pub"),
            "1.4.0"
        ),
        Err(Refusal::Signature(_))
    ));
    assert!(matches!(
        verify(
            &release_file(),
            &fixture("signed-1.4.0.sig"),
            &fixture("other-key.pub"),
            "1.4.0"
        ),
        Err(Refusal::Signature(_))
    ));
    // an empty key, as a configuration with no committed key holds
    assert!(matches!(
        verify(&release_file(), &fixture("signed-1.4.0.sig"), "", "1.4.0"),
        Err(Refusal::Signature(_))
    ));
    // the committed key reads as a minisign key, so a throwaway key's signature fails
    // on the key, not on reading it
    let committed = marshal_feed::committed_public_key().unwrap();
    match verify(
        &release_file(),
        &fixture("signed-1.4.0.sig"),
        &committed,
        "1.4.0",
    ) {
        Err(Refusal::Signature(why)) => assert!(!why.starts_with("the public key"), "{why}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn whitespace_around_the_key_or_signature_is_refused_as_the_plugin_refuses_it() {
    let bytes = release_file();
    let key = fixture("key.pub");
    let signature = fixture("signed-1.4.0.sig");
    assert!(matches!(
        verify(&bytes, &signature, &format!("{key}\n"), "1.4.0"),
        Err(Refusal::Signature(_))
    ));
    assert!(matches!(
        verify(&bytes, &format!(" {signature}"), &key, "1.4.0"),
        Err(Refusal::Signature(_))
    ));
}

#[test]
fn the_signature_must_name_the_feeds_version() {
    let bytes = release_file();
    // a validly signed older release offered under a newer version number
    assert_eq!(
        verify(
            &bytes,
            &fixture("signed-1.3.0.sig"),
            &fixture("key.pub"),
            "1.4.0"
        ),
        Err(Refusal::SignedVersion {
            signed: Some("1.3.0".into()),
            announced: "1.4.0".into()
        })
    );
    assert_eq!(
        verify(
            &bytes,
            &fixture("unversioned.sig"),
            &fixture("key.pub"),
            "1.4.0"
        ),
        Err(Refusal::SignedVersion {
            signed: None,
            announced: "1.4.0".into()
        })
    );
}

#[test]
fn only_this_repositorys_release_files_are_downloaded() {
    let allowed = |address: &str| download_allowed(&Url::parse(address).expect("an address"));
    assert_eq!(
        allowed("https://github.com/axross/runners/releases/download/v1.4.0/marshal.msi"),
        Ok(())
    );
    for refused in [
        "http://github.com/axross/runners/releases/download/v1.4.0/marshal.msi",
        "https://github.com.example.invalid/axross/runners/releases/download/v1.4.0/m.msi",
        "https://objects.githubusercontent.com/axross/runners/releases/download/v1/m.msi",
        "https://github.com:8443/axross/runners/releases/download/v1.4.0/marshal.msi",
        "https://someone@github.com/axross/runners/releases/download/v1.4.0/marshal.msi",
        "https://github.com/axross/runners-fork/releases/download/v1.4.0/marshal.msi",
        "https://github.com/axross/runners/releases/downloads/v1.4.0/marshal.msi",
        "https://github.com/axross/runners/archive/refs/tags/v1.4.0.zip",
        // dot segments are resolved before the path is read
        "https://github.com/axross/runners/releases/download/../../../other/r/m.msi",
    ] {
        assert!(
            matches!(allowed(refused), Err(Refusal::Address(_))),
            "{refused} was allowed"
        );
    }
}

#[test]
fn only_a_newer_version_is_an_update() {
    assert_eq!(newer("1.3.9", "1.4.0"), Ok(true));
    assert_eq!(newer("1.4.0", "1.4.0"), Ok(false));
    assert_eq!(newer("1.10.0", "1.9.0"), Ok(false));
    assert_eq!(newer("0.0.0-dev", "0.0.0"), Ok(true));
    assert!(matches!(newer("1.4", "1.5.0"), Err(Refusal::Version(_))));
}

fn feed(version: &str, url: &str, signature: &str) -> String {
    serde_json::json!({
        "version": version,
        "pub_date": "2026-12-01T18:00:00Z",
        "platforms": { "windows-x86_64": { "url": url, "signature": signature } }
    })
    .to_string()
}

fn checked(feed: &str, version: &str, recorded: &str, key: &str) -> Result<(), Vec<Refusal>> {
    let bytes = release_file();
    check_feed(
        feed,
        version,
        &[Released {
            platform: "windows-x86_64",
            name: MSI,
            bytes: &bytes,
            recorded_sha256: recorded,
        }],
        &fixture(key),
    )
}

#[test]
fn the_feed_check_passes_the_feed_the_release_run_writes() {
    let sha = sha256_hex(&release_file());
    let good = feed(
        "1.4.0",
        &release_address("1.4.0", MSI),
        &fixture("signed-1.4.0.sig"),
    );
    assert_eq!(checked(&good, "1.4.0", &sha, "key.pub"), Ok(()));
    assert_eq!(
        release_address("1.4.0", MSI),
        "https://github.com/axross/runners/releases/download/v1.4.0/marshal-1.4.0-windows-x64.msi"
    );
}

#[test]
fn the_feed_check_lists_every_problem() {
    let sha = sha256_hex(&release_file());
    let wrong = feed(
        "1.3.0",
        &release_address("1.3.0", MSI),
        &fixture("signed-1.3.0.sig"),
    );
    let refusals = checked(&wrong, "1.4.0", &"0".repeat(64), "key.pub").expect_err("refused");
    let text: Vec<String> = refusals.iter().map(ToString::to_string).collect();
    assert_eq!(refusals.len(), 4, "{text:#?}");
    assert!(
        text.iter()
            .any(|line| line.contains("names version 1.3.0, and the release is 1.4.0"))
    );
    assert!(
        text.iter()
            .any(|line| line.contains("/v1.3.0/") && line.contains("not https"))
    );
    assert!(
        text.iter()
            .any(|line| line.contains("the build recorded 0000"))
    );
    assert!(refusals.contains(&Refusal::SignedVersion {
        signed: Some("1.3.0".into()),
        announced: "1.4.0".into()
    }));

    let good = feed(
        "1.4.0",
        &release_address("1.4.0", MSI),
        &fixture("signed-1.4.0.sig"),
    );
    let refusals = checked(&good, "1.4.0", &sha, "other-key.pub").expect_err("refused");
    assert!(matches!(refusals.as_slice(), [Refusal::Signature(_)]));
}

#[test]
fn the_feed_check_wants_exactly_the_signed_platforms() {
    let sha = sha256_hex(&release_file());
    let entry = serde_json::json!({
        "url": release_address("1.4.0", MSI),
        "signature": fixture("signed-1.4.0.sig"),
    });
    let extra = serde_json::json!({
        "version": "1.4.0",
        "platforms": { "windows-x86_64": entry.clone(), "darwin-aarch64": entry }
    })
    .to_string();
    let refusals = checked(&extra, "1.4.0", &sha, "key.pub").expect_err("refused");
    assert_eq!(refusals.len(), 1);
    assert!(refusals[0].to_string().contains("darwin-aarch64"));

    let none = serde_json::json!({ "version": "1.4.0", "platforms": {} }).to_string();
    let refusals = checked(&none, "1.4.0", &sha, "key.pub").expect_err("refused");
    assert_eq!(
        refusals,
        vec![Refusal::Feed(
            "the feed has no entry for windows-x86_64".into()
        )]
    );
    assert!(checked("{}", "1.4.0", &sha, "key.pub").is_err());
}

#[test]
fn a_recorded_sum_is_found_in_either_form() {
    let sums = format!("{} *a.msi\n{}  {MSI}\n", "a".repeat(64), "b".repeat(64));
    assert_eq!(recorded_sha256(&sums, MSI), Some("b".repeat(64).as_str()));
    assert_eq!(
        recorded_sha256(&sums, "a.msi"),
        Some("a".repeat(64).as_str())
    );
    assert_eq!(recorded_sha256(&sums, "c.msi"), None);
}

/// the app's configuration file, relative to the app crate.
fn app_json(relative: &str) -> serde_json::Value {
    let path = format!("{}/../app/{relative}", env!("CARGO_MANIFEST_DIR"));
    serde_json::from_str(&std::fs::read_to_string(path).expect("the file")).expect("JSON")
}

#[test]
fn the_updater_is_configured_as_the_plan_says_with_no_dangerous_option() {
    let configuration = app_json("tauri.conf.json");
    let updater = &configuration["plugins"]["updater"];
    let keys: Vec<&str> = updater
        .as_object()
        .expect("an updater configuration")
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        ["endpoints", "pubkey", "requireSignedVersion", "windows"],
        "no dangerous option, no downgrades, no other key"
    );
    assert_eq!(
        updater["endpoints"],
        serde_json::json!([marshal_feed::ENDPOINT])
    );
    assert_eq!(updater["requireSignedVersion"], true);
    assert_eq!(
        updater["windows"],
        serde_json::json!({ "installMode": "passive" })
    );
    assert_eq!(
        marshal_feed::committed_public_key().ok().as_deref(),
        updater["pubkey"].as_str()
    );
    // the maintainer's update-feed key, minisign key ID 6F39E3131E21E1E4; a replaced key
    // strands every installed copy
    assert_eq!(
        updater["pubkey"],
        "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDZGMzlFMzEzMUUyMUUxRTQKUldUazRTRWVFK001Yjl3MllpMWlKbzhpUGsyRTd3YW1NY04xTEl4RStZVDlYN25nMEtPbUhhZ3UK"
    );
    let plugins: Vec<&String> = configuration["plugins"]
        .as_object()
        .expect("plugins")
        .keys()
        .collect();
    assert_eq!(plugins, ["updater"]);
}

#[test]
fn no_window_is_granted_an_updater_command() {
    for capability in ["main-window", "confirmation-window"] {
        let granted =
            app_json(&format!("capabilities/{capability}.json"))["permissions"].to_string();
        assert!(
            !granted.contains("updater"),
            "{capability} grants an updater permission: {granted}"
        );
    }
}
