//! the display requests over HTTPS: conditional `GET`s with the registration
//! client's limits, API version and secret handling; an answer keeps only its
//! status, the headers the agent reads, GitHub's cleaned message, and a body only
//! when it is a fresh page.

use std::future::Future;
use std::time::Duration;

use reqwest::header::{ACCEPT, AUTHORIZATION, HeaderMap, HeaderValue, IF_NONE_MATCH};

use crate::json::{self, Value};
use crate::runtime::registration::{
    ACCEPT_VALUE, API_VERSION, HttpLimits, SetupError, https_client, resolve_base_url,
};
use crate::runtime::scrub::replace_secret;
use crate::runtime::secret::Secret;

use super::answer::{Answer, Cause, Next};
use super::budget::RateHeaders;
use super::clean;

/// one display request.
pub struct Get<'a> {
    /// the path and query, starting with `/repos/`.
    pub path: &'a str,
    pub token: &'a Secret,
    /// the page's last `ETag`, sent as `If-None-Match`.
    pub etag: Option<&'a str>,
}

/// something that sends a display request; an `Err` is a request that got no
/// answer, `Unreachable`, or one whose answer was not usable, `Error`.
pub trait Fetch {
    fn get(&self, request: &Get<'_>) -> impl Future<Output = Result<Answer, Cause>>;

    /// the origin requests go to, which a `link` header's next page must keep.
    fn base_url(&self) -> &str;
}

/// the display client over HTTPS.
pub struct GitHubReader {
    client: reqwest::Client,
    base_url: String,
    limits: HttpLimits,
}

impl GitHubReader {
    /// builds the client; `base_override` is honoured only by a debug build, as
    /// registration's is.
    pub fn new(base_override: Option<&str>, limits: HttpLimits) -> Result<Self, SetupError> {
        let base_url = resolve_base_url(base_override)?;
        let client = https_client(&base_url, limits)?;
        Ok(Self {
            client,
            base_url,
            limits,
        })
    }
}

impl Fetch for GitHubReader {
    fn base_url(&self) -> &str {
        &self.base_url
    }

    async fn get(&self, request: &Get<'_>) -> Result<Answer, Cause> {
        if !is_request_path(request.path) {
            return Err(Cause::Error);
        }
        let mut authorization =
            HeaderValue::from_str(&format!("Bearer {}", request.token.expose()))
                .map_err(|_| Cause::TokenUnreadable)?;
        authorization.set_sensitive(true);
        let mut builder = self
            .client
            .get(format!("{}{}", self.base_url, request.path))
            .header(AUTHORIZATION, authorization)
            .header(ACCEPT, ACCEPT_VALUE)
            .header("X-GitHub-Api-Version", API_VERSION);
        if let Some(etag) = request.etag {
            let value = HeaderValue::from_str(etag).map_err(|_| Cause::Error)?;
            builder = builder.header(IF_NONE_MATCH, value);
        }
        let mut response = builder.send().await.map_err(|_| Cause::Unreachable)?;
        let status = response.status().as_u16();
        let headers = response.headers().clone();
        let cap = self.limits.max_response_bytes;
        if response
            .content_length()
            .is_some_and(|length| length > cap as u64)
        {
            return Err(Cause::Error);
        }
        let mut body: Vec<u8> = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| Cause::Unreachable)? {
            if body.len() + chunk.len() > cap {
                return Err(Cause::Error);
            }
            body.extend_from_slice(&chunk);
        }
        Ok(answer(status, &headers, body, request.token))
    }
}

fn header<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name).and_then(|value| value.to_str().ok())
}

fn number(headers: &HeaderMap, name: &str) -> Option<u64> {
    header(headers, name).and_then(|text| text.trim().parse().ok())
}

/// GitHub's `message`, cleaned and cut for the log. The token is replaced before the
/// cut, so a cut never leaves part of it where the whole-token mask cannot find it.
fn message(body: &[u8], token: &Secret) -> Option<String> {
    let Value::Object(object) = json::parse(body).ok()? else {
        return None;
    };
    object.iter().find_map(|(key, value)| match value {
        Value::String(text) if key == "message" => Some(clean::cut(
            &replace_secret(text, token.expose()),
            clean::MESSAGE_CHARS,
        )),
        _ => None,
    })
}

/// a wait GitHub asks for, at most an hour, the rate limit's own window, so no
/// header can stop collection for longer or overflow a deadline.
fn bounded_wait(seconds: u64) -> Duration {
    Duration::from_secs(seconds.min(MAX_WAIT_SECONDS))
}

/// the longest wait a `retry-after` or `x-poll-interval` header sets.
pub const MAX_WAIT_SECONDS: u64 = 3600;

fn answer(status: u16, headers: &HeaderMap, body: Vec<u8>, token: &Secret) -> Answer {
    let fresh = status == 200;
    Answer {
        status,
        rate: RateHeaders {
            limit: number(headers, "x-ratelimit-limit"),
            remaining: number(headers, "x-ratelimit-remaining"),
            reset: number(headers, "x-ratelimit-reset"),
            used: number(headers, "x-ratelimit-used"),
        },
        etag: header(headers, "etag").map(str::to_string),
        retry_after: number(headers, "retry-after").map(bounded_wait),
        poll_interval: number(headers, "x-poll-interval").map(bounded_wait),
        accepted_permissions: header(headers, "x-accepted-github-permissions")
            .map(|text| clean::cut(text, clean::MESSAGE_CHARS)),
        message: (!fresh).then(|| message(&body, token)).flatten(),
        link: header(headers, "link").map(str::to_string),
        body: if fresh { body } else { Vec::new() },
    }
}

/// whether every character of a path or query is one the agent's own requests use.
fn plain(text: &str) -> bool {
    !text.is_empty()
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"/_.-=&,".contains(&byte))
}

/// the endpoint of a `/repos/<owner>/<repo>/…` path: what follows the repository.
fn repos_endpoint(route: &str) -> Option<&str> {
    let mut parts = route.strip_prefix("/repos/")?.splitn(3, '/');
    let (owner, repo, rest) = (parts.next()?, parts.next()?, parts.next()?);
    (!owner.is_empty() && !repo.is_empty() && !rest.is_empty()).then_some(rest)
}

/// the endpoint of GitHub's `/repositories/<digits>/…` form of a path.
fn id_endpoint(route: &str) -> Option<&str> {
    let (id, rest) = route.strip_prefix("/repositories/")?.split_once('/')?;
    (!id.is_empty() && id.bytes().all(|byte| byte.is_ascii_digit()) && !rest.is_empty())
        .then_some(rest)
}

/// whether a path and query is one the agent requests: one it builds under
/// `/repos/`, or a next page in GitHub's `/repositories/<id>/` form, without `..`.
fn is_request_path(path: &str) -> bool {
    let (route, query) = path.split_once('?').unwrap_or((path, ""));
    (repos_endpoint(route).is_some() || id_endpoint(route).is_some())
        && plain(route)
        && (query.is_empty() || plain(query))
        && !route.split('/').any(|part| part == "." || part == "..")
}

/// the id of a path in GitHub's `/repositories/<id>/…` form.
#[must_use]
pub fn repository_id(path: &str) -> Option<&str> {
    let route = path.split_once('?').map_or(path, |(route, _)| route);
    route
        .strip_prefix("/repositories/")
        .and_then(|rest| rest.split_once('/'))
        .map(|(id, _)| id)
}

/// the next page of a paged read that a `link` header names: compared with the
/// read's first request, followed only on its origin, on its path or on GitHub's
/// `/repositories/<id>` form of it, with only the query changed. `held_id` is the id
/// the read has already seen on one of its pages, and a next page with another id
/// is refused, so one read names one id.
#[must_use]
pub fn next_page(link: &str, base_url: &str, first: &str, held_id: Option<&str>) -> Next {
    let Some(target) = link.split(',').find_map(|part| {
        let (address, params) = part.trim().split_once(';')?;
        let is_next = params
            .split(';')
            .any(|param| param.trim().eq_ignore_ascii_case("rel=\"next\""));
        is_next.then(|| address.trim().strip_prefix('<')?.strip_suffix('>'))?
    }) else {
        return Next::None;
    };
    let Some(local) = target
        .strip_prefix(base_url)
        .filter(|rest| rest.starts_with('/'))
    else {
        return Next::Refused;
    };
    let route = local.split_once('?').map_or(local, |(route, _)| route);
    let route_of = |path: &'_ str| {
        path.split_once('?')
            .map_or(path, |(route, _)| route)
            .to_string()
    };
    let first_route = route_of(first);
    let held = held_id.or_else(|| repository_id(first));
    let same_id = match (repository_id(route), held) {
        (Some(next), Some(held)) => next == held,
        _ => true,
    };
    let same_endpoint = route == first_route
        || (same_id
            && matches!((id_endpoint(route), repos_endpoint(&first_route)), (Some(next), Some(asked)) if next == asked));
    let usable = is_request_path(local);
    if same_endpoint && usable {
        Next::Page(local.to_string())
    } else {
        Next::Refused
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = "https://api.github.com";
    const REQUEST: &str = "/repos/owner/app/actions/runs?status=in_progress&per_page=10";

    fn link(next: &str) -> String {
        format!("<{next}>; rel=\"next\", <{BASE}/repositories/1/actions/runs?page=9>; rel=\"last\"")
    }

    #[test]
    fn a_next_page_on_the_same_path_is_followed() {
        let next =
            format!("{BASE}/repos/owner/app/actions/runs?status=in_progress&per_page=10&page=2");
        assert_eq!(
            next_page(&link(&next), BASE, REQUEST, None),
            Next::Page(
                "/repos/owner/app/actions/runs?status=in_progress&per_page=10&page=2".to_string()
            )
        );
    }

    #[test]
    fn a_next_page_in_githubs_repository_id_form_is_followed() {
        let next = format!(
            "{BASE}/repositories/724712/actions/runs?status=in_progress&per_page=10&page=2"
        );
        assert_eq!(
            next_page(&link(&next), BASE, REQUEST, None),
            Next::Page(
                "/repositories/724712/actions/runs?status=in_progress&per_page=10&page=2"
                    .to_string()
            )
        );
        let jobs = "/repos/owner/app/actions/runs/123/jobs?filter=latest&per_page=30";
        let next =
            format!("{BASE}/repositories/7/actions/runs/123/jobs?filter=latest&per_page=30&page=2");
        assert!(matches!(
            next_page(&link(&next), BASE, jobs, None),
            Next::Page(_)
        ));
    }

    #[test]
    fn a_next_page_elsewhere_is_refused() {
        for next in [
            "https://evil.example/repos/owner/app/actions/runs?page=2".to_string(),
            "http://api.github.com/repos/owner/app/actions/runs?page=2".to_string(),
            format!("{BASE}.evil.example/repos/owner/app/actions/runs?page=2"),
            format!("{BASE}@evil.example/repos/owner/app/actions/runs?page=2"),
            format!("{BASE}/repos/owner/other/actions/runs?page=2"),
            format!("{BASE}/repos/owner/app/actions/runners?page=2"),
            format!("{BASE}/repositories/7/actions/runners?page=2"),
            format!("{BASE}/repositories/x7/actions/runs?page=2"),
            format!("{BASE}/repositories/7/actions/runs/../../secrets?page=2"),
            format!("{BASE}/repositories/7/actions/runs?page=2#frag"),
            format!("{BASE}/repositories/7/actions/runs?page=%32"),
        ] {
            assert_eq!(
                next_page(&link(&next), BASE, REQUEST, None),
                Next::Refused,
                "{next}"
            );
        }
    }

    #[test]
    fn later_pages_keep_the_first_requests_path_and_one_id() {
        let first = "/repos/owner/app/actions/runs?status=in_progress";
        let second = "/repositories/7/actions/runs?status=in_progress&page=2";
        let third = format!("{BASE}/repositories/7/actions/runs?status=in_progress&page=3");
        assert!(matches!(
            next_page(&link(&third), BASE, first, repository_id(second)),
            Next::Page(_)
        ));
        let back = format!("{BASE}/repos/owner/app/actions/runs?status=in_progress&page=3");
        assert!(matches!(
            next_page(&link(&back), BASE, first, repository_id(second)),
            Next::Page(_)
        ));
        let other = format!("{BASE}/repositories/8/actions/runs?status=in_progress&page=3");
        assert_eq!(
            next_page(&link(&other), BASE, first, repository_id(second)),
            Next::Refused
        );
        let other_repo = format!("{BASE}/repos/owner/other/actions/runs?page=3");
        assert_eq!(
            next_page(&link(&other_repo), BASE, first, repository_id(second)),
            Next::Refused
        );
    }

    #[test]
    fn a_link_without_a_next_page_has_none() {
        assert_eq!(
            next_page(
                &format!("<{BASE}/repositories/1/actions/runs?page=1>; rel=\"prev\""),
                BASE,
                REQUEST,
                None
            ),
            Next::None
        );
    }

    #[test]
    fn a_token_in_githubs_message_is_replaced_before_the_message_is_cut() {
        let token = "github_pat_0123456789abcdefghijklmnopqrstuvwxyz";
        // the token starts at the 181st character, so a cut at 200 would split it
        let text = format!("{}{token} more", "m".repeat(180));
        let body = serde_json::json!({ "message": text }).to_string();
        let message = message(body.as_bytes(), &Secret::new(token.to_string())).unwrap();
        assert!(!message.contains("github_pat_"), "{message}");
        assert!(message.ends_with("<token> more"), "{message}");
    }

    #[test]
    fn a_wait_github_asks_for_counts_for_at_most_an_hour() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "retry-after",
            HeaderValue::from_static("18446744073709551615"),
        );
        headers.insert("x-poll-interval", HeaderValue::from_static("7200"));
        let bounded = answer(429, &headers, Vec::new(), &Secret::new(String::new()));
        assert_eq!(bounded.retry_after, Some(Duration::from_secs(3600)));
        assert_eq!(bounded.poll_interval, Some(Duration::from_secs(3600)));
        headers.insert("retry-after", HeaderValue::from_static("120"));
        let kept = answer(429, &headers, Vec::new(), &Secret::new(String::new()));
        assert_eq!(kept.retry_after, Some(Duration::from_secs(120)));
    }

    #[test]
    fn only_paths_the_agent_builds_are_requested() {
        assert!(is_request_path(REQUEST));
        assert!(is_request_path("/repos/o/r/actions/runners?per_page=100"));
        assert!(is_request_path("/repositories/7/actions/runs?page=2"));
        assert!(!is_request_path("/user"));
        assert!(!is_request_path("/repositories/x/actions/runs"));
        assert!(!is_request_path("/repos/o/../x/actions/runners"));
        assert!(!is_request_path("/repos/o/r/actions/runners?x=%0a"));
    }
}
