//! Just-in-time runner registration with GitHub.
//!
//! The request is the PowerShell supervisor's: the same endpoint, headers, body and
//! response field. It adds a `User-Agent`, follows no redirect, fails a connection
//! after 10 s and a whole request after 30 s, and fails a response over 1 MiB. The
//! token travels in a header marked sensitive and is replaced by `<token>` in every
//! error text.

use std::fmt;
use std::future::Future;
use std::time::Duration;

use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE, HeaderValue};

use crate::json::{self, Value};
use crate::validate::patterns::{is_owner, is_repository};

use super::scrub::scrub_error_text;
use super::secret::Secret;

/// Where registrations go, unless a debug build is told otherwise.
pub const GITHUB_API: &str = "https://api.github.com";

/// The runner group every registration joins: the default group, which the endpoint
/// requires even though a repository-level runner has no group to choose.
pub const RUNNER_GROUP_ID: u64 = 1;

const API_VERSION: &str = "2022-11-28";
const ACCEPT_VALUE: &str = "application/vnd.github+json";

/// The time and size bounds on one registration request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HttpLimits {
    pub connect_timeout: Duration,
    /// The whole request, from connecting to the last byte of the response.
    pub total_timeout: Duration,
    pub max_response_bytes: usize,
}

impl Default for HttpLimits {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(10),
            total_timeout: Duration::from_secs(30),
            max_response_bytes: 1024 * 1024,
        }
    }
}

/// What a registration asks for.
pub struct JitRequest<'a> {
    pub owner: &'a str,
    pub repository: &'a str,
    /// The runner's name, which is also its container's name.
    pub name: &'a str,
    pub labels: &'a [String],
    pub token: &'a Secret,
}

/// Why a registration failed. The text is scrubbed of the token, on one line, and
/// at most 503 characters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistrationError(String);

impl RegistrationError {
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for RegistrationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for RegistrationError {}

/// Something that can turn a request into a JIT configuration.
pub trait Registrar {
    fn request_jit(
        &self,
        request: &JitRequest<'_>,
    ) -> impl Future<Output = Result<Secret, RegistrationError>>;
}

/// Why the HTTPS client could not be set up. This is a startup failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetupError(String);

impl fmt::Display for SetupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for SetupError {}

/// Installs the `ring` crypto provider as the process default, which the HTTPS
/// client needs before it is built. Calling it again, or after another provider was
/// installed, succeeds.
///
/// # Errors
///
/// Fails when no provider is installed and none can be.
pub fn install_tls_provider() -> Result<(), SetupError> {
    if rustls::crypto::CryptoProvider::get_default().is_some() {
        return Ok(());
    }
    // a lost race with another installer leaves a provider in place, which is enough.
    let _ = rustls::crypto::ring::default_provider().install_default();
    if rustls::crypto::CryptoProvider::get_default().is_some() {
        Ok(())
    } else {
        Err(SetupError(
            "TLS setup failed: no crypto provider could be installed".to_string(),
        ))
    }
}

/// Resolves the base address of the API.
///
/// A release build always returns [`GITHUB_API`] and ignores `requested`, so a
/// release binary cannot send a token anywhere but GitHub. A debug build accepts
/// `http://127.0.0.1:<port>` for tests and refuses any other value.
///
/// # Errors
///
/// A debug build refuses a `requested` address that is not a loopback address with
/// a port.
pub fn resolve_base_url(requested: Option<&str>) -> Result<String, SetupError> {
    #[cfg(debug_assertions)]
    if let Some(requested) = requested {
        return if is_loopback_base(requested) {
            Ok(requested.to_string())
        } else {
            Err(SetupError(
                "the GitHub base address can only be http://127.0.0.1:<port>".to_string(),
            ))
        };
    }
    #[cfg(not(debug_assertions))]
    let _ = requested;
    Ok(GITHUB_API.to_string())
}

/// Whether the text is exactly `http://127.0.0.1:<port>` with a port from 1 to 65535.
#[cfg(any(debug_assertions, test))]
fn is_loopback_base(text: &str) -> bool {
    let Some(port) = text.strip_prefix("http://127.0.0.1:") else {
        return false;
    };
    let digits = !port.is_empty()
        && port.len() <= 5
        && port.bytes().all(|byte| byte.is_ascii_digit())
        && !port.starts_with('0');
    digits && port.parse::<u32>().is_ok_and(|number| number <= 65535)
}

/// The registration client over HTTPS.
pub struct GitHubRegistrar {
    client: reqwest::Client,
    base_url: String,
    limits: HttpLimits,
}

impl GitHubRegistrar {
    /// Builds the client. `base_override` is honoured only by a debug build; see
    /// [`resolve_base_url`].
    ///
    /// # Errors
    ///
    /// Fails when the TLS provider cannot be installed or the client cannot be
    /// built, for example because the platform has no trusted roots to load.
    pub fn new(base_override: Option<&str>, limits: HttpLimits) -> Result<Self, SetupError> {
        install_tls_provider()?;
        let base_url = resolve_base_url(base_override)?;
        let user_agent = format!("runners/{}", env!("CARGO_PKG_VERSION"));
        let mut builder = reqwest::Client::builder()
            .user_agent(user_agent)
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(limits.connect_timeout)
            .timeout(limits.total_timeout);
        if base_url == GITHUB_API {
            builder = builder.https_only(true);
        } else {
            // a loopback stub is never behind a proxy.
            builder = builder.no_proxy();
        }
        let client = builder
            .build()
            .map_err(|error| SetupError(format!("the HTTPS client could not be built: {error}")))?;
        Ok(Self {
            client,
            base_url,
            limits,
        })
    }

    /// The address registrations go to, for tests.
    #[must_use]
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    async fn send(&self, request: &JitRequest<'_>) -> Result<Secret, String> {
        // the configuration's own patterns, which also refuse the `.` and `..`
        // components that would climb out of `/repos/`.
        if !is_owner(request.owner) || !is_repository(request.repository) {
            return Err("the repository owner or name is not usable in a URL".to_string());
        }
        let mut authorization =
            HeaderValue::from_str(&format!("Bearer {}", request.token.expose()))
                .map_err(|_| "the token is not valid in an HTTP header".to_string())?;
        authorization.set_sensitive(true);
        let url = format!(
            "{}/repos/{}/{}/actions/runners/generate-jitconfig",
            self.base_url, request.owner, request.repository
        );
        let mut response = self
            .client
            .post(url)
            .header(AUTHORIZATION, authorization)
            .header(ACCEPT, ACCEPT_VALUE)
            .header("X-GitHub-Api-Version", API_VERSION)
            .header(CONTENT_TYPE, "application/json")
            .body(jit_request_body(request.name, request.labels))
            .send()
            .await
            .map_err(|error| error_chain(&error.without_url()))?;
        let status = response.status();
        let cap = self.limits.max_response_bytes;
        if response
            .content_length()
            .is_some_and(|length| length > cap as u64)
        {
            return Err(too_large(cap));
        }
        let mut body: Vec<u8> = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|error| error_chain(&error.without_url()))?
        {
            if body.len() + chunk.len() > cap {
                return Err(too_large(cap));
            }
            body.extend_from_slice(&chunk);
        }
        if !status.is_success() {
            return Err(format!(
                "Response status code does not indicate success: {} ({}). {}",
                status.as_u16(),
                status.canonical_reason().unwrap_or("unknown"),
                String::from_utf8_lossy(&body)
            ));
        }
        encoded_jit_config(&body)
            .map(Secret::new)
            .ok_or_else(|| "GitHub returned no JIT configuration".to_string())
    }
}

impl Registrar for GitHubRegistrar {
    async fn request_jit(&self, request: &JitRequest<'_>) -> Result<Secret, RegistrationError> {
        self.send(request)
            .await
            .map_err(|text| RegistrationError(scrub_error_text(&text, request.token.expose())))
    }
}

fn too_large(cap: usize) -> String {
    format!("the response is larger than {cap} bytes")
}

/// The error's own text and every cause's, in order.
fn error_chain(error: &dyn std::error::Error) -> String {
    let mut text = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        text.push_str(": ");
        text.push_str(&cause.to_string());
        source = cause.source();
    }
    text
}

/// The `encoded_jit_config` of a response, when it is a non-blank string.
fn encoded_jit_config(body: &[u8]) -> Option<String> {
    let Value::Object(object) = json::parse(body).ok()? else {
        return None;
    };
    object.iter().find_map(|(key, value)| match value {
        Value::String(text) if key == "encoded_jit_config" && !text.trim().is_empty() => {
            Some(text.clone())
        }
        _ => None,
    })
}

/// The request body: the runner's name, the group, and the labels, as compact JSON.
#[must_use]
pub fn jit_request_body(name: &str, labels: &[String]) -> String {
    let mut body = String::from("{\"name\":");
    write_json_string(&mut body, name);
    body.push_str(&format!(
        ",\"runner_group_id\":{RUNNER_GROUP_ID},\"labels\":["
    ));
    for (index, label) in labels.iter().enumerate() {
        if index > 0 {
            body.push(',');
        }
        write_json_string(&mut body, label);
    }
    body.push_str("]}");
    body
}

/// Writes a JSON string literal with the escapes RFC 8259 requires.
fn write_json_string(out: &mut String, text: &str) {
    out.push('"');
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            control if control < ' ' => out.push_str(&format!("\\u{:04x}", u32::from(control))),
            other => out.push(other),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_body_is_compact_json_with_the_three_fields() {
        let labels = vec!["self-hosted".to_string(), "axpc".to_string()];
        assert_eq!(
            jit_request_body("example-1-20240305060708009", &labels),
            r#"{"name":"example-1-20240305060708009","runner_group_id":1,"labels":["self-hosted","axpc"]}"#
        );
        assert_eq!(
            jit_request_body("n", &[]),
            r#"{"name":"n","runner_group_id":1,"labels":[]}"#
        );
    }

    #[test]
    fn the_body_escapes_what_json_requires_and_reads_back() {
        let tricky = "a\"b\\c\nd\te\u{1}f\u{8}g\u{c}h\u{7f}é😀";
        let body = jit_request_body(tricky, &[tricky.to_string()]);
        assert!(!body.contains('\n') && !body.contains('\u{1}'));
        let Value::Object(object) = json::parse(body.as_bytes()).unwrap() else {
            panic!("not an object");
        };
        let members: Vec<_> = object.iter().collect();
        assert_eq!(members[0], ("name", &Value::String(tricky.to_string())));
        assert_eq!(
            members[2],
            (
                "labels",
                &Value::Array(vec![Value::String(tricky.to_string())])
            )
        );
    }

    #[test]
    fn only_a_loopback_address_with_a_port_is_a_base_address() {
        for good in [
            "http://127.0.0.1:1",
            "http://127.0.0.1:8080",
            "http://127.0.0.1:65535",
        ] {
            assert!(is_loopback_base(good), "{good}");
        }
        for bad in [
            "http://127.0.0.1",
            "http://127.0.0.1:",
            "http://127.0.0.1:0",
            "http://127.0.0.1:08",
            "http://127.0.0.1:65536",
            "http://127.0.0.1:80/",
            "http://127.0.0.1:80/x",
            "http://127.0.0.1:80@evil.example",
            "http://127.0.0.1:80.evil.example",
            "https://127.0.0.1:80",
            "http://localhost:80",
            "http://127.0.0.2:80",
            "http://[::1]:80",
            "http://api.github.com:80",
            "https://api.github.com",
            "",
        ] {
            assert!(!is_loopback_base(bad), "{bad}");
        }
    }

    #[test]
    fn the_response_field_is_read_only_when_it_is_a_non_blank_string() {
        assert_eq!(
            encoded_jit_config(br#"{"runner":{},"encoded_jit_config":"abc="}"#).as_deref(),
            Some("abc=")
        );
        for body in [
            &br#"{"encoded_jit_config":""}"#[..],
            br#"{"encoded_jit_config":"  "}"#,
            br#"{"encoded_jit_config":1}"#,
            br#"{}"#,
            br#"[]"#,
            b"not json",
            b"",
        ] {
            assert_eq!(encoded_jit_config(body), None);
        }
    }

    #[test]
    fn the_limits_default_to_ten_seconds_thirty_seconds_and_one_mebibyte() {
        let limits = HttpLimits::default();
        assert_eq!(limits.connect_timeout, Duration::from_secs(10));
        assert_eq!(limits.total_timeout, Duration::from_secs(30));
        assert_eq!(limits.max_response_bytes, 1_048_576);
    }

    #[test]
    fn the_client_builds_with_the_ring_provider_and_installing_twice_is_fine() {
        install_tls_provider().unwrap();
        install_tls_provider().unwrap();
        let registrar = GitHubRegistrar::new(None, HttpLimits::default()).unwrap();
        assert_eq!(registrar.base_url(), GITHUB_API);
    }
}
