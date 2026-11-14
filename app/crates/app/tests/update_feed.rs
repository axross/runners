//! the last step of the feed round trip in merge-checks.yaml's update-feed-app job: the
//! feed and the dry-run MSI that the release run's own signing scripts signed with a
//! throwaway key are accepted by the updater plugin's own verification, under the app's
//! updater configuration with that key, and a changed file, another key's signature and a
//! signature for another version are refused. The job hands the files over in the
//! directory `MARSHAL_UPDATE_FEED_DIR` names, so these tests are ignored by default and
//! fail when run without it.
#![cfg(windows)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::PathBuf;

use serde_json::Value;
use tauri::test::{mock_builder, mock_context, noop_assets};
use tauri_plugin_updater::{Error, Update, UpdaterExt};

/// the platform entry the app reads on Windows.
const PLATFORM: &str = "windows-x86_64";

fn directory() -> PathBuf {
    PathBuf::from(
        std::env::var_os("MARSHAL_UPDATE_FEED_DIR")
            .expect("MARSHAL_UPDATE_FEED_DIR must name the feed round trip's files"),
    )
}

fn read(name: &str) -> Vec<u8> {
    let path = directory().join(name);
    std::fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

/// the update the plugin restores from `feed`, a feed file of the round trip, and the
/// bytes of the MSI its entry names. The app's own updater configuration is used, with
/// the throwaway public key in place of the committed one; any version counts as newer,
/// since the dry run's version is below a mock app's.
fn update(feed: &str) -> (Update, Vec<u8>) {
    let public_key = String::from_utf8(read("update.pub")).unwrap();
    let mut configuration: Value =
        serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
    let mut updater = configuration["plugins"]["updater"].take();
    updater["pubkey"] = Value::from(public_key.trim());
    let mut context = mock_context(noop_assets());
    context
        .config_mut()
        .plugins
        .0
        .insert("updater".to_string(), updater);
    let app = mock_builder()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .build(context)
        .unwrap();
    let feed: Value = serde_json::from_slice(&read(feed)).unwrap();
    let msi = feed["platforms"][PLATFORM]["url"]
        .as_str()
        .and_then(|url| url.rsplit('/').next())
        .unwrap_or_else(|| panic!("{feed} has no {PLATFORM} address"))
        .to_string();
    let update = app
        .updater_builder()
        .version_comparator(|_, _| true)
        .build()
        .unwrap()
        .restore_update(feed)
        .unwrap()
        .expect("the feed offers an update");
    (update, read(&msi))
}

#[test]
#[ignore = "needs the feed round trip's files in MARSHAL_UPDATE_FEED_DIR"]
fn the_signed_feed_and_msi_are_accepted() {
    let (update, msi) = update("latest.json");
    update.verify(&msi).unwrap();
}

#[test]
#[ignore = "needs the feed round trip's files in MARSHAL_UPDATE_FEED_DIR"]
fn a_changed_msi_is_refused() {
    let (update, mut msi) = update("latest.json");
    let last = msi.last_mut().expect("the MSI is not empty");
    *last ^= 1;
    assert!(update.verify(&msi).is_err());
}

#[test]
#[ignore = "needs the feed round trip's files in MARSHAL_UPDATE_FEED_DIR"]
fn another_keys_signature_is_refused() {
    let (update, msi) = update("latest-another-key.json");
    let refusal = update.verify(&msi).unwrap_err();
    assert!(
        !matches!(refusal, Error::SignedVersionMismatch { .. }),
        "{refusal}"
    );
}

#[test]
#[ignore = "needs the feed round trip's files in MARSHAL_UPDATE_FEED_DIR"]
fn a_signature_for_another_version_is_refused() {
    let (update, msi) = update("latest-another-version.json");
    assert!(matches!(
        update.verify(&msi),
        Err(Error::SignedVersionMismatch { .. })
    ));
}
