//! the update feed: the rules that decide whether the app takes a release, and the
//! check the release run makes of `latest.json` before it publishes it. The app
//! downloads and verifies through Tauri's updater plugin; this crate holds what the
//! plugin does not, the download address rule and the committed public key, and the same
//! signature rules the plugin applies, so the feed check accepts exactly what the app
//! would.

use std::collections::BTreeMap;
use std::fmt;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use minisign_verify::{PublicKey, Signature};
use semver::Version;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use url::Url;

/// where the app reads the feed: `latest.json` on the repository's latest release.
pub const ENDPOINT: &str = "https://github.com/axross/runners/releases/latest/download/latest.json";

/// the only host the app downloads an update from; GitHub's redirects from it are
/// followed.
pub const DOWNLOAD_HOST: &str = "github.com";

/// the path every download address starts with: a file of one of this repository's
/// releases.
pub const DOWNLOAD_PATH: &str = "/axross/runners/releases/download/";

/// the feed's entry the Windows app reads.
pub const WINDOWS: &str = "windows-x86_64";

/// the version a build carries before a release stamps its own; such a build never
/// checks for an update.
pub const DEVELOPMENT_VERSION: &str = "0.0.0-dev";

/// the app's configuration, which holds the committed public key the app verifies with.
const CONFIGURATION: &str = include_str!("../../app/tauri.conf.json");

/// why a release, or a feed, is not taken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// a version is not a version number.
    Version(String),
    /// the download address is not a file of this repository's releases.
    Address(String),
    /// the signature does not verify against the public key, or either cannot be read.
    Signature(String),
    /// the signature names no version, or another one than the feed.
    SignedVersion {
        signed: Option<String>,
        announced: String,
    },
    /// the feed itself is not what the release run wrote.
    Feed(String),
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Version(version) => write!(f, "'{version}' is not a version number"),
            Self::Address(address) => write!(
                f,
                "the download address {address} is not an https address on {DOWNLOAD_HOST} under {DOWNLOAD_PATH}"
            ),
            Self::Signature(why) => write!(f, "the signature does not verify: {why}"),
            Self::SignedVersion {
                signed: Some(signed),
                announced,
            } => write!(
                f,
                "the signature names version {signed}, and the feed names {announced}"
            ),
            Self::SignedVersion {
                signed: None,
                announced,
            } => write!(
                f,
                "the signature names no version, and the feed names {announced}"
            ),
            Self::Feed(why) => write!(f, "{why}"),
        }
    }
}

impl std::error::Error for Refusal {}

/// whether `offered` is newer than `own`, compared as the updater compares them.
///
/// # Errors
/// Either is not a version number.
pub fn newer(own: &str, offered: &str) -> Result<bool, Refusal> {
    let own = version(own)?;
    let offered = version(offered)?;
    Ok(offered > own)
}

fn version(text: &str) -> Result<Version, Refusal> {
    Version::parse(text).map_err(|_| Refusal::Version(text.to_string()))
}

/// refuses a download address that is not `https`, on `github.com` with no port of its
/// own and no credentials, under this repository's release files.
///
/// # Errors
/// The address is anything else.
pub fn download_allowed(address: &Url) -> Result<(), Refusal> {
    let allowed = address.scheme() == "https"
        && address.host_str() == Some(DOWNLOAD_HOST)
        && address.port().is_none()
        && address.username().is_empty()
        && address.password().is_none()
        && address.path().starts_with(DOWNLOAD_PATH);
    if allowed {
        Ok(())
    } else {
        Err(Refusal::Address(address.to_string()))
    }
}

/// verifies `bytes` against the feed's `signature` and `public_key`, both in the base64
/// form Tauri's signer writes, and requires the signature's trusted comment to name
/// `version`, as the updater does with `requireSignedVersion`.
///
/// # Errors
/// The key or signature cannot be read, the signature does not verify, or it names no
/// version or another one.
pub fn verify(
    bytes: &[u8],
    signature: &str,
    public_key: &str,
    version: &str,
) -> Result<(), Refusal> {
    let key = PublicKey::decode(&decoded(public_key, "public key")?)
        .map_err(|error| Refusal::Signature(format!("the public key: {error}")))?;
    let signature = Signature::decode(&decoded(signature, "signature")?)
        .map_err(|error| Refusal::Signature(format!("the signature: {error}")))?;
    key.verify(bytes, &signature, true)
        .map_err(|error| Refusal::Signature(error.to_string()))?;
    // the trusted comment is covered by the global signature `verify` checked
    let signed = signature
        .trusted_comment()
        .split('\t')
        .find_map(|field| field.strip_prefix("version:"));
    let same = match signed {
        Some(signed) => match (
            Version::parse(signed.trim_start_matches('v')),
            Version::parse(version.trim_start_matches('v')),
        ) {
            (Ok(signed), Ok(announced)) => signed == announced,
            _ => signed == version,
        },
        None => false,
    };
    if same {
        Ok(())
    } else {
        Err(Refusal::SignedVersion {
            signed: signed.map(str::to_string),
            announced: version.to_string(),
        })
    }
}

fn decoded(text: &str, what: &str) -> Result<String, Refusal> {
    let bytes = STANDARD
        .decode(text)
        .map_err(|_| Refusal::Signature(format!("the {what} is not base64")))?;
    String::from_utf8(bytes).map_err(|_| Refusal::Signature(format!("the {what} is not text")))
}

#[derive(Deserialize)]
struct Configuration {
    plugins: Plugins,
}

#[derive(Deserialize)]
struct Plugins {
    updater: UpdaterConfiguration,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdaterConfiguration {
    pubkey: String,
}

/// the update-feed public key committed in the app's `tauri.conf.json`, which the
/// updater verifies every update with.
///
/// # Errors
/// The configuration holds no updater key.
pub fn committed_public_key() -> Result<String, Refusal> {
    let configuration: Configuration = serde_json::from_str(CONFIGURATION)
        .map_err(|error| Refusal::Feed(format!("the app's configuration: {error}")))?;
    Ok(configuration.plugins.updater.pubkey)
}

/// `latest.json` as the release run writes it.
#[derive(Debug, Deserialize)]
pub struct Feed {
    pub version: String,
    pub platforms: BTreeMap<String, Entry>,
}

/// one platform's file in the feed.
#[derive(Debug, Deserialize)]
pub struct Entry {
    pub url: String,
    pub signature: String,
}

/// a release file the feed check is given: the feed entry it is for, its name, its
/// bytes, and the SHA-256 the build recorded for it.
pub struct Released<'a> {
    pub platform: &'a str,
    pub name: &'a str,
    pub bytes: &'a [u8],
    pub recorded_sha256: &'a str,
}

/// the download address of a release file.
#[must_use]
pub fn release_address(version: &str, name: &str) -> String {
    format!("https://{DOWNLOAD_HOST}{DOWNLOAD_PATH}v{version}/{name}")
}

/// the SHA-256 of `bytes`, as lower-case hex.
#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// the release run's check of the feed it wrote, before anything is published: the feed
/// names `version` and exactly the platforms of `files`, and each entry's address is its
/// file under this release's tag, its file's SHA-256 is the one the build recorded, and
/// its signature verifies against `public_key` and names `version`. Every problem is
/// listed, not only the first.
///
/// # Errors
/// What does not hold, one entry each.
pub fn check_feed(
    feed: &str,
    version: &str,
    files: &[Released<'_>],
    public_key: &str,
) -> Result<(), Vec<Refusal>> {
    let feed: Feed = serde_json::from_str(feed)
        .map_err(|error| vec![Refusal::Feed(format!("the feed is not a feed: {error}"))])?;
    let mut refusals = Vec::new();
    if Version::parse(version).is_err() {
        refusals.push(Refusal::Version(version.to_string()));
    }
    if feed.version != version {
        refusals.push(Refusal::Feed(format!(
            "the feed names version {}, and the release is {version}",
            feed.version
        )));
    }
    let given: Vec<&str> = files.iter().map(|file| file.platform).collect();
    for platform in feed.platforms.keys() {
        if !given.contains(&platform.as_str()) {
            refusals.push(Refusal::Feed(format!(
                "the feed has an entry for {platform}, which the release run did not sign"
            )));
        }
    }
    for file in files {
        let Some(entry) = feed.platforms.get(file.platform) else {
            refusals.push(Refusal::Feed(format!(
                "the feed has no entry for {}",
                file.platform
            )));
            continue;
        };
        let expected = release_address(version, file.name);
        if entry.url != expected {
            refusals.push(Refusal::Feed(format!(
                "the {} entry's address is {}, not {expected}",
                file.platform, entry.url
            )));
        }
        match Url::parse(&entry.url) {
            Ok(address) => {
                if let Err(refusal) = download_allowed(&address) {
                    refusals.push(refusal);
                }
            }
            Err(_) => refusals.push(Refusal::Address(entry.url.clone())),
        }
        let actual = sha256_hex(file.bytes);
        if !actual.eq_ignore_ascii_case(file.recorded_sha256.trim()) {
            refusals.push(Refusal::Feed(format!(
                "{}'s SHA-256 is {actual}, and the build recorded {}",
                file.name,
                file.recorded_sha256.trim()
            )));
        }
        if let Err(refusal) = verify(file.bytes, &entry.signature, public_key, version) {
            refusals.push(refusal);
        }
    }
    if refusals.is_empty() {
        Ok(())
    } else {
        Err(refusals)
    }
}

/// the SHA-256 a `SHA256SUMS` file records for `name`, in `sha256sum`'s text or binary
/// form.
#[must_use]
pub fn recorded_sha256<'a>(sums: &'a str, name: &str) -> Option<&'a str> {
    sums.lines().find_map(|line| {
        let (hash, file) = line.split_once(' ')?;
        let file = file.strip_prefix([' ', '*']).unwrap_or(file);
        (file == name).then_some(hash)
    })
}
