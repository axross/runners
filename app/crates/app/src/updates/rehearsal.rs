//! the rehearsal build, the `rehearsal` feature: the feed address, the public key and the
//! accepted download prefix of a local test feed, given when the build is made, replace
//! the committed ones, and the feed may be read over plain HTTP. No workflow enables it;
//! see docs/operations/app.md.

use serde_json::{Value, json};
use tauri::{Context, Runtime, Url};

const FEED: &str = env!("MARSHAL_REHEARSAL_FEED");
const PUBLIC_KEY: &str = env!("MARSHAL_REHEARSAL_PUBLIC_KEY");
const DOWNLOAD_PREFIX: &str = env!("MARSHAL_REHEARSAL_DOWNLOAD_PREFIX");

/// points the updater plugin's configuration at the test feed and its key, and allows
/// the feed's plain HTTP; given the context before the app is built, so the plugin reads
/// it.
pub fn configured<R: Runtime>(mut context: Context<R>) -> Context<R> {
    if let Some(updater) = context
        .config_mut()
        .plugins
        .0
        .get_mut("updater")
        .and_then(Value::as_object_mut)
    {
        updater.insert("endpoints".to_string(), json!([FEED]));
        updater.insert("pubkey".to_string(), json!(PUBLIC_KEY));
        updater.insert(
            "dangerousInsecureTransportProtocol".to_string(),
            json!(true),
        );
    }
    context
}

/// the test feed's key, in place of the committed one.
pub(super) fn public_key() -> Result<String, String> {
    Ok(PUBLIC_KEY.to_string())
}

/// refuses a download address outside the test feed's prefix; a prefix that does not end
/// in `/` accepts nothing, so it cannot let another host or port through.
pub(super) fn download_allowed(address: &Url) -> Result<(), String> {
    if DOWNLOAD_PREFIX.ends_with('/') && address.as_str().starts_with(DOWNLOAD_PREFIX) {
        Ok(())
    } else {
        Err(format!(
            "the download address {address} is not under the rehearsal's {DOWNLOAD_PREFIX}"
        ))
    }
}
