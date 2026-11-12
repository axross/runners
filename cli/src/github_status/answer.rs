//! what an answer from GitHub means for a display request, as the table in
//! `docs/operations/cli.md` lists it: a 403 or 429 is a rate limit when its headers
//! or message say so, and otherwise a rejected token for the runner list or hidden
//! job names for a lookup.

use std::time::Duration;

use super::budget::RateHeaders;

/// which list a display request reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Endpoint {
    /// the repository's self-hosted runners.
    Runners,
    /// the repository's in-progress workflow runs.
    Runs,
    /// one run's jobs.
    Jobs,
}

impl Endpoint {
    fn is_lookup(self) -> bool {
        !matches!(self, Self::Runners)
    }
}

/// an answer as the HTTP reader gives it; it holds no token.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Answer {
    pub status: u16,
    pub rate: RateHeaders,
    pub etag: Option<String>,
    pub retry_after: Option<Duration>,
    pub poll_interval: Option<Duration>,
    pub accepted_permissions: Option<String>,
    /// GitHub's `message`, cleaned and cut for the log.
    pub message: Option<String>,
    pub body: Vec<u8>,
    /// the `link` header, which [`super::http::next_page`] reads.
    pub link: Option<String>,
}

/// the next page a `link` header names.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Next {
    #[default]
    None,
    /// a path and query on the request's own origin and path.
    Page(String),
    /// an address with another origin or path, which is never requested.
    Refused,
}

/// why a repository, or its job lookup, has no fresh answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cause {
    TokenRejected,
    NotFound,
    Moved,
    Unreachable,
    Error,
    TokenUnreadable,
}

impl Cause {
    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Self::TokenRejected => "token_rejected",
            Self::NotFound => "not_found",
            Self::Moved => "moved",
            Self::Unreachable => "unreachable",
            Self::Error => "error",
            Self::TokenUnreadable => "token_unreadable",
        }
    }
}

/// how long a rate-limit answer makes every display request wait.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wait {
    /// no requests left: until the budget's reset.
    UntilReset,
    /// a secondary limit, with GitHub's `retry-after` when it sent one.
    Secondary(Option<Duration>),
    /// GitHub's `retry-after`.
    For(Duration),
}

/// what one answer means.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// a page to read.
    Fresh,
    /// `304`: the page has not changed.
    Unchanged,
    RateLimited(Wait),
    /// a lookup answered a 403 that is not a rate limit: no Actions: read.
    Hidden,
    Failed(Cause),
}

/// whether GitHub's message says a secondary limit was hit.
fn says_secondary(message: Option<&str>) -> bool {
    message.is_some_and(|message| message.to_lowercase().contains("secondary rate limit"))
}

/// reads an answer to a request for `endpoint`.
#[must_use]
pub fn classify(endpoint: Endpoint, answer: &Answer) -> Outcome {
    match answer.status {
        200 => Outcome::Fresh,
        304 => Outcome::Unchanged,
        403 | 429 if answer.rate.remaining == Some(0) => Outcome::RateLimited(Wait::UntilReset),
        403 | 429 if says_secondary(answer.message.as_deref()) => {
            Outcome::RateLimited(Wait::Secondary(answer.retry_after))
        }
        403 | 429 if answer.retry_after.is_some() => {
            Outcome::RateLimited(Wait::For(answer.retry_after.unwrap_or_default()))
        }
        401 => Outcome::Failed(Cause::TokenRejected),
        403 if endpoint.is_lookup() => Outcome::Hidden,
        403 => Outcome::Failed(Cause::TokenRejected),
        404 => Outcome::Failed(Cause::NotFound),
        300..=399 => Outcome::Failed(Cause::Moved),
        _ => Outcome::Failed(Cause::Error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answer(status: u16) -> Answer {
        Answer {
            status,
            ..Answer::default()
        }
    }

    #[test]
    fn each_status_has_its_meaning_for_the_runner_list_and_a_lookup() {
        let rows: [(u16, Outcome, Outcome); 9] = [
            (200, Outcome::Fresh, Outcome::Fresh),
            (304, Outcome::Unchanged, Outcome::Unchanged),
            (
                401,
                Outcome::Failed(Cause::TokenRejected),
                Outcome::Failed(Cause::TokenRejected),
            ),
            (403, Outcome::Failed(Cause::TokenRejected), Outcome::Hidden),
            (
                404,
                Outcome::Failed(Cause::NotFound),
                Outcome::Failed(Cause::NotFound),
            ),
            (
                301,
                Outcome::Failed(Cause::Moved),
                Outcome::Failed(Cause::Moved),
            ),
            (
                307,
                Outcome::Failed(Cause::Moved),
                Outcome::Failed(Cause::Moved),
            ),
            (
                502,
                Outcome::Failed(Cause::Error),
                Outcome::Failed(Cause::Error),
            ),
            (
                429,
                Outcome::Failed(Cause::Error),
                Outcome::Failed(Cause::Error),
            ),
        ];
        for (status, runners, lookup) in rows {
            assert_eq!(
                classify(Endpoint::Runners, &answer(status)),
                runners,
                "{status}"
            );
            assert_eq!(
                classify(Endpoint::Runs, &answer(status)),
                lookup,
                "{status}"
            );
            assert_eq!(
                classify(Endpoint::Jobs, &answer(status)),
                lookup,
                "{status}"
            );
        }
    }

    #[test]
    fn a_403_with_nothing_left_is_a_rate_limit_and_not_hidden_names() {
        let mut limited = answer(403);
        limited.rate.remaining = Some(0);
        limited.message = Some("API rate limit exceeded".to_string());
        assert_eq!(
            classify(Endpoint::Runs, &limited),
            Outcome::RateLimited(Wait::UntilReset)
        );
        limited.status = 429;
        assert_eq!(
            classify(Endpoint::Runners, &limited),
            Outcome::RateLimited(Wait::UntilReset)
        );
    }

    #[test]
    fn a_secondary_limit_is_told_by_its_message_or_its_retry_after() {
        let mut secondary = answer(403);
        secondary.rate.remaining = Some(4000);
        secondary.message =
            Some("You have exceeded a Secondary Rate Limit. Please wait.".to_string());
        assert_eq!(
            classify(Endpoint::Jobs, &secondary),
            Outcome::RateLimited(Wait::Secondary(None))
        );
        let mut wait = answer(429);
        wait.retry_after = Some(Duration::from_secs(30));
        assert_eq!(
            classify(Endpoint::Runs, &wait),
            Outcome::RateLimited(Wait::For(Duration::from_secs(30)))
        );
    }

    #[test]
    fn a_missing_permission_is_hidden_names_for_a_lookup() {
        let mut missing = answer(403);
        missing.rate.remaining = Some(4496);
        missing.message = Some("Resource not accessible by personal access token".to_string());
        missing.accepted_permissions = Some("actions=read".to_string());
        assert_eq!(classify(Endpoint::Runs, &missing), Outcome::Hidden);
        assert_eq!(
            classify(Endpoint::Runners, &missing),
            Outcome::Failed(Cause::TokenRejected)
        );
    }
}
