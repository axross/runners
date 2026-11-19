use std::fmt;
use std::time::Duration;

use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use reqwest::{Method, RequestBuilder, Url};
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::runtime::registration::{ACCEPT_VALUE, API_VERSION, HttpLimits, https_client};
use crate::runtime::scrub::replace_secret;
use crate::runtime::secret::Secret;

/// the service may hold a poll for 50 seconds; response reading needs time beyond that hold.
pub const POLL_TIMEOUT: Duration = Duration::from_secs(70);
const SERVICE_ACCEPT: &str = "application/json; api-version=6.0-preview";
const RATE_WINDOW_SECONDS: u64 = 3600;
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
    Unreachable,
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
        let reason = match self.kind {
            ErrorKind::Setup => "scale-set HTTPS setup failed",
            ErrorKind::InvalidInput => "invalid scale-set request",
            ErrorKind::ConnectionRenewalRequired => "scale-set connection renewal required",
            ErrorKind::Conflict => "another listener",
            ErrorKind::NotFound => "scale set, session or runner not found",
            ErrorKind::Rejected => "rejected by GitHub",
            ErrorKind::RateLimited => "rate limited",
            ErrorKind::RefusedHost => "rejected by GitHub: refused host",
            ErrorKind::Redirect => "scale-set redirect refused",
            ErrorKind::Unreachable => "scale-set request failed or timed out; outcome unconfirmed",
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

    /// marks credentials sensitive before handing a request to reqwest.
    fn request(
        &self,
        method: Method,
        url: Url,
        scheme: &str,
        token: &Secret,
        body: Option<Value>,
    ) -> Result<RequestBuilder, Error> {
        let mut auth = HeaderValue::from_str(&format!("{scheme} {}", token.expose()))
            .map_err(|_| Error::new(ErrorKind::InvalidInput))?;
        auth.set_sensitive(true);
        let mut request = self
            .client
            .request(method, url)
            .header(AUTHORIZATION, auth)
            .header(CONTENT_TYPE, "application/json")
            .header(ACCEPT, SERVICE_ACCEPT);
        if let Some(body) = body {
            request = request.body(body.to_string());
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
            .request(method, url, "Bearer", token, body)?
            .header(ACCEPT, ACCEPT_VALUE)
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
        self.send(method, url, ("Bearer", token), body, statuses, None)
            .await
    }

    /// keeps exchange and bounded-close requests behind the same safe response reader.
    pub async fn send(
        &self,
        method: Method,
        url: Url,
        authorization: (&str, &Secret),
        body: Option<Value>,
        statuses: &[u16],
        timeout: Option<Duration>,
    ) -> Result<Vec<u8>, Error> {
        let mut request = self.request(method, url, authorization.0, authorization.1, body)?;
        if let Some(timeout) = timeout {
            request = request.timeout(timeout);
        }
        self.read(request, statuses).await.map(|(_, body)| body)
    }

    /// overrides the ordinary request timeout without changing REST requests or their limits.
    pub async fn poll(
        &self,
        url: Url,
        token: &Secret,
        capacity: u32,
    ) -> Result<Option<Vec<u8>>, Error> {
        let request = self
            .request(Method::GET, url, "Bearer", token, None)?
            .timeout(POLL_TIMEOUT)
            .header("X-ScaleSetMaxCapacity", capacity);
        let (status, body) = self.read(request, &[200, 202]).await?;
        Ok((status == 200).then_some(body))
    }

    /// checks both advertised and streamed size, retaining only a typed failure on non-success.
    async fn read(
        &self,
        request: RequestBuilder,
        statuses: &[u16],
    ) -> Result<(u16, Vec<u8>), Error> {
        let mut response = request
            .send()
            .await
            .map_err(|_| Error::new(ErrorKind::Unreachable))?;
        let status = response.status().as_u16();
        let headers = response.headers().clone();
        let cap = self.limits.max_response_bytes;
        if response
            .content_length()
            .is_some_and(|length| length > cap as u64)
        {
            return Err(Error::new(ErrorKind::ResponseTooLarge));
        }
        let mut body = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| Error::new(ErrorKind::Unreachable))?
        {
            if chunk.len() > cap.saturating_sub(body.len()) {
                return Err(Error::new(ErrorKind::ResponseTooLarge));
            }
            body.extend_from_slice(&chunk);
        }
        if !statuses.contains(&status) {
            return Err(classify(status, &headers, &body));
        }
        Ok((status, body))
    }
}

fn header_number(headers: &HeaderMap, key: &str) -> Option<u64> {
    headers.get(key)?.to_str().ok()?.trim().parse().ok()
}

/// rate-limit signals take precedence over permission rejection, including Retry-After alone.
fn classify(status: u16, headers: &HeaderMap, body: &[u8]) -> Error {
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
            409 => ErrorKind::Conflict,
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
            .flatten(),
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
