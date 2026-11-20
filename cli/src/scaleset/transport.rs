use std::fmt;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use reqwest::{Method, RequestBuilder, Url};
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::github_status::budget::RATE_WINDOW_SECONDS;
use crate::runtime::registration::{
    ACCEPT_VALUE, API_VERSION, HttpLimits, error_chain, https_client,
};
use crate::runtime::scrub::replace_secret;
use crate::runtime::secret::Secret;

/// the service may hold a poll for 50 seconds; response reading needs time beyond that hold.
pub const POLL_TIMEOUT: Duration = Duration::from_secs(70);
const SERVICE_ACCEPT: &str = "application/json; api-version=6.0-preview";
const MAX_HOST_CHARS: usize = 253;

/// failures the listener classifies without inspecting untrusted service text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorKind {
    Setup,
    InvalidInput,
    ConnectionRenewalRequired,
    Conflict,
    NotFound,
    Rejected,
    RateLimited,
    RefusedHost,
    Redirect,
    /// carries the url-less cause chain of the transport failure, which holds no credential.
    Unreachable(String),
    ResponseTooLarge,
    Protocol,
    Service,
    LabelMismatch,
    OwnershipMismatch,
}

/// only non-secret status and retry hints survive the HTTP boundary; response bodies never do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub kind: ErrorKind,
    pub status: Option<u16>,
    pub retry_after: Option<Duration>,
    pub rate_reset: Option<u64>,
    pub refused_host: Option<String>,
}

impl Error {
    pub(super) fn new(kind: ErrorKind) -> Self {
        Self {
            kind,
            status: None,
            retry_after: None,
            rate_reset: None,
            refused_host: None,
        }
    }

    pub(super) fn protocol() -> Self {
        Self::new(ErrorKind::Protocol)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let reason = match &self.kind {
            ErrorKind::Setup => "scale-set HTTPS setup failed",
            ErrorKind::InvalidInput => "invalid scale-set request",
            ErrorKind::ConnectionRenewalRequired => "scale-set connection renewal required",
            ErrorKind::Conflict => "another listener",
            ErrorKind::NotFound => "scale set, session or runner not found",
            ErrorKind::Rejected => "rejected by GitHub",
            ErrorKind::RateLimited => "rate limited",
            ErrorKind::RefusedHost => "rejected by GitHub: refused host",
            ErrorKind::Redirect => "scale-set redirect refused",
            ErrorKind::Unreachable(_) => {
                "scale-set request failed or timed out; outcome unconfirmed"
            }
            ErrorKind::ResponseTooLarge => {
                "scale-set response exceeds size limit; outcome unconfirmed"
            }
            ErrorKind::Protocol => "invalid scale-set response; outcome unconfirmed",
            ErrorKind::Service => "scale-set service failure",
            ErrorKind::LabelMismatch => "scale-set label mismatch",
            ErrorKind::OwnershipMismatch => "runner ownership mismatch",
        };
        f.write_str(reason)?;
        if let Some(status) = self.status {
            write!(f, " ({status})")?;
        }
        if let ErrorKind::Unreachable(cause) = &self.kind {
            write!(f, ": {cause}")?;
        }
        if let Some(host) = &self.refused_host {
            write!(f, ": {host}")?;
        }
        Ok(())
    }
}

impl std::error::Error for Error {}

/// the bounded HTTP boundary; neither reqwest errors nor serde errors are exposed.
pub(super) struct Transport {
    client: reqwest::Client,
    limits: HttpLimits,
}

impl Transport {
    /// reuses registration's TLS provider, proxy handling and redirect prohibition.
    pub fn new(base: &str, limits: HttpLimits) -> Result<Self, Error> {
        Ok(Self {
            client: https_client(base, limits).map_err(|_| Error::new(ErrorKind::Setup))?,
            limits,
        })
    }

    /// marks credentials sensitive before handing a request to reqwest; `accept` is the one
    /// Accept value of the endpoint family, and a request without a body sends no content type.
    fn request(
        &self,
        method: Method,
        url: Url,
        authorization: (&str, &Secret),
        accept: Option<&str>,
        body: Option<Value>,
    ) -> Result<RequestBuilder, Error> {
        let (scheme, token) = authorization;
        let mut auth = HeaderValue::from_str(&format!("{scheme} {}", token.expose()))
            .map_err(|_| Error::new(ErrorKind::InvalidInput))?;
        auth.set_sensitive(true);
        let mut request = self.client.request(method, url).header(AUTHORIZATION, auth);
        if let Some(accept) = accept {
            request = request.header(ACCEPT, accept);
        }
        if let Some(body) = body {
            request = request
                .header(CONTENT_TYPE, "application/json")
                .body(body.to_string());
        }
        Ok(request)
    }

    /// REST lifecycle calls stay separate from the display collector's rate accounting.
    pub async fn rest(
        &self,
        method: Method,
        url: Url,
        token: &Secret,
        body: Option<Value>,
        statuses: &[u16],
    ) -> Result<Vec<u8>, Error> {
        let request = self
            .request(method, url, ("Bearer", token), Some(ACCEPT_VALUE), body)?
            .header("X-GitHub-Api-Version", API_VERSION);
        self.read(request, statuses).await.map(|(_, body)| body)
    }

    pub async fn service(
        &self,
        method: Method,
        url: Url,
        token: &Secret,
        body: Option<Value>,
        statuses: &[u16],
    ) -> Result<Vec<u8>, Error> {
        let request = self.request(method, url, ("Bearer", token), Some(SERVICE_ACCEPT), body)?;
        self.read(request, statuses).await.map(|(_, body)| body)
    }

    /// a bodiless service request with its own timeout, for the bounded session close.
    pub async fn service_within(
        &self,
        method: Method,
        url: Url,
        token: &Secret,
        statuses: &[u16],
        timeout: Duration,
    ) -> Result<Vec<u8>, Error> {
        let request = self
            .request(method, url, ("Bearer", token), Some(SERVICE_ACCEPT), None)?
            .timeout(timeout);
        self.read(request, statuses).await.map(|(_, body)| body)
    }

    /// the registration exchange sends no service Accept, as the upstream client does not.
    pub async fn exchange(
        &self,
        url: Url,
        registration: &Secret,
        body: Value,
    ) -> Result<Vec<u8>, Error> {
        let request = self.request(
            Method::POST,
            url,
            ("RemoteAuth", registration),
            None,
            Some(body),
        )?;
        self.read(request, &[200, 201]).await.map(|(_, body)| body)
    }

    /// overrides the ordinary request timeout without changing REST requests or their limits.
    pub async fn poll(
        &self,
        url: Url,
        token: &Secret,
        capacity: u32,
    ) -> Result<Option<Vec<u8>>, Error> {
        let request = self
            .request(
                Method::GET,
                url,
                ("Bearer", token),
                Some(SERVICE_ACCEPT),
                None,
            )?
            .timeout(POLL_TIMEOUT)
            .header("X-ScaleSetMaxCapacity", capacity);
        let (status, body) = self.read(request, &[200, 202]).await?;
        Ok((status == 200).then_some(body))
    }

    /// checks both advertised and streamed size of a success; an error status is classified
    /// from its status and headers first, and an oversized or unreadable body is discarded.
    async fn read(
        &self,
        request: RequestBuilder,
        statuses: &[u16],
    ) -> Result<(u16, Vec<u8>), Error> {
        let mut response = request.send().await.map_err(unreachable_error)?;
        let status = response.status().as_u16();
        let cap = self.limits.max_response_bytes;
        let accepted = statuses.contains(&status);
        let mut body = Vec::new();
        let oversized = response
            .content_length()
            .is_some_and(|length| length > cap as u64);
        if oversized && accepted {
            return Err(Error::new(ErrorKind::ResponseTooLarge));
        }
        if !oversized {
            loop {
                match response.chunk().await {
                    Ok(Some(chunk)) if chunk.len() <= cap.saturating_sub(body.len()) => {
                        body.extend_from_slice(&chunk);
                    }
                    Ok(Some(_)) if accepted => {
                        return Err(Error::new(ErrorKind::ResponseTooLarge));
                    }
                    Err(error) if accepted => return Err(unreachable_error(error)),
                    Ok(None) => break,
                    Ok(Some(_)) | Err(_) => {
                        body.clear();
                        break;
                    }
                }
            }
        }
        if accepted {
            Ok((status, body))
        } else {
            Err(classify(status, response.headers(), &body, now_seconds()))
        }
    }
}

fn unreachable_error(error: reqwest::Error) -> Error {
    Error::new(ErrorKind::Unreachable(error_chain(&error.without_url())))
}

fn now_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

fn header_number(headers: &HeaderMap, key: &str) -> Option<u64> {
    headers.get(key)?.to_str().ok()?.trim().parse().ok()
}

/// rate-limit signals take precedence over permission rejection, including Retry-After alone;
/// a reset further out than GitHub's window is held to one window from `now`.
fn classify(status: u16, headers: &HeaderMap, body: &[u8], now: u64) -> Error {
    let secondary = serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|value| {
            value.get("message").and_then(Value::as_str).map(|message| {
                message
                    .to_ascii_lowercase()
                    .contains("secondary rate limit")
            })
        })
        .unwrap_or(false);
    let limited = status == 429
        || (status == 403
            && (headers.contains_key("retry-after")
                || header_number(headers, "x-ratelimit-remaining") == Some(0)
                || secondary));
    let kind = if limited {
        ErrorKind::RateLimited
    } else {
        match status {
            401 | 403 => ErrorKind::Rejected,
            404 => ErrorKind::NotFound,
            300..=399 => ErrorKind::Redirect,
            _ => ErrorKind::Service,
        }
    };
    Error {
        kind,
        status: Some(status),
        retry_after: header_number(headers, "retry-after")
            .map(|seconds| Duration::from_secs(seconds.min(RATE_WINDOW_SECONDS))),
        rate_reset: (header_number(headers, "x-ratelimit-remaining") == Some(0))
            .then(|| header_number(headers, "x-ratelimit-reset"))
            .flatten()
            .map(|reset| reset.min(now.saturating_add(RATE_WINDOW_SECONDS))),
        refused_host: None,
    }
}

pub(super) fn decode<T: DeserializeOwned>(body: &[u8]) -> Result<T, Error> {
    serde_json::from_slice(body.strip_prefix(b"\xef\xbb\xbf").unwrap_or(body))
        .map_err(|_| Error::protocol())
}

/// authenticates destinations by parsed HTTPS host, or this client's own debug loopback origin.
pub(super) fn actions_url(text: &str, base: &Url, secrets: &[&Secret]) -> Result<Url, Error> {
    let url = Url::parse(text).map_err(|_| Error::protocol())?;
    let host = url.host_str().unwrap_or("");
    let actions = url.scheme() == "https"
        && (host == "actions.githubusercontent.com"
            || host.ends_with(".actions.githubusercontent.com"));
    let loopback =
        cfg!(debug_assertions) && base.scheme() == "http" && url.origin() == base.origin();
    if (actions || loopback)
        && url.username().is_empty()
        && url.password().is_none()
        && url.fragment().is_none()
    {
        return Ok(url);
    }
    let mut host = host.to_owned();
    for secret in secrets {
        host = replace_secret(&host, secret.expose());
        host = replace_secret(&host, &secret.expose().to_ascii_lowercase());
    }
    let mut error = Error::new(ErrorKind::RefusedHost);
    error.refused_host = Some(host.chars().take(MAX_HOST_CHARS).collect());
    Err(error)
}
