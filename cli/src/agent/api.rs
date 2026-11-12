//! what a connection asks of the agent, parsed from the wire: the methods that reach
//! the agent's one task, and the errors they answer with.

use serde::Deserialize;
use tokio::sync::oneshot;

use crate::runtime::secret::Secret;
use crate::token_store::{RepositoryKey, StoreError, TokenRefusal, accepted_token};

use super::wire::{ErrorBody, ErrorCode, StoreFault, TOPIC_GITHUB, TOPIC_STATE};

/// an error with its stable code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiError {
    pub code: ErrorCode,
    pub message: String,
    pub store: Option<StoreFault>,
}

impl ApiError {
    #[must_use]
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            store: None,
        }
    }

    /// a `store_failed` error, which says what kind of failure the store reported.
    #[must_use]
    pub fn store_failed(action: &str, error: &StoreError) -> Self {
        Self {
            store: Some(StoreFault {
                kind: error.kind().word().to_string(),
                code: error.code(),
            }),
            ..Self::new(
                ErrorCode::StoreFailed,
                format!("the token was not {action}: {}", error.describe()),
            )
        }
    }

    #[must_use]
    pub fn body(&self) -> ErrorBody {
        ErrorBody {
            store: self.store.clone(),
            ..ErrorBody::new(self.code, &self.message)
        }
    }
}

pub type ApiResult = Result<serde_json::Value, ApiError>;

/// a method for the agent; a token travels only in `TokenSet`, as a [`Secret`].
#[derive(Debug)]
pub enum Method {
    Status,
    StartAll,
    StopAll,
    Build,
    Quit,
    TokenSet {
        key: RepositoryKey,
        token: Secret,
    },
    TokenRemove {
        key: RepositoryKey,
    },
    /// drain for an update to `version`; `requester` reports, by closing, that the
    /// connection that asked has closed, and `None` never does.
    DrainForUpdate {
        version: String,
        requester: Option<oneshot::Receiver<()>>,
    },
}

/// a method and where its result goes.
#[derive(Debug)]
pub struct Call {
    pub method: Method,
    pub reply: oneshot::Sender<ApiResult>,
}

/// a topic a connection subscribes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Topic {
    State,
    /// collected only while a subscription to it is renewed.
    Github,
}

impl Topic {
    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Self::State => TOPIC_STATE,
            Self::Github => TOPIC_GITHUB,
        }
    }

    fn of(word: &str) -> Result<Self, ApiError> {
        match word {
            TOPIC_STATE => Ok(Self::State),
            TOPIC_GITHUB => Ok(Self::Github),
            _ => Err(invalid("the agent serves the topics state and github")),
        }
    }
}

/// a request as the connection understands it.
#[derive(Debug)]
pub enum Parsed {
    Subscribe(Topic),
    /// keeps the connection's `github` subscription alive.
    Renew(Topic),
    Call(Method),
}

#[derive(Deserialize)]
struct SubscribeParams {
    topic: String,
}

#[derive(Deserialize)]
struct TokenSetParams {
    repository: String,
    token: String,
}

#[derive(Deserialize)]
struct TokenRemoveParams {
    repository: String,
}

#[derive(Deserialize)]
struct DrainParams {
    version: String,
}

/// the longest version a drain takes.
const MAX_VERSION_CHARS: usize = 64;

/// whether `text` is a version number: three dot-separated numbers, then optionally
/// `-` or `+` and ASCII letters, digits, dots, hyphens and pluses.
fn is_version(text: &str) -> bool {
    if text.is_empty() || text.len() > MAX_VERSION_CHARS {
        return false;
    }
    let (core, rest) = match text.find(['-', '+']) {
        Some(at) => text.split_at(at),
        None => (text, ""),
    };
    let numbers: Vec<&str> = core.split('.').collect();
    numbers.len() == 3
        && numbers
            .iter()
            .all(|number| !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit()))
        && rest
            .bytes()
            .skip(1)
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'+'))
        && rest.len() != 1
}

fn invalid(message: &str) -> ApiError {
    ApiError::new(ErrorCode::InvalidRequest, message)
}

/// reads the params with the type `T`; the message never repeats the params, because
/// they may hold a token.
fn params<T: for<'de> Deserialize<'de>>(value: serde_json::Value) -> Result<T, ApiError> {
    serde_json::from_value(value).map_err(|_| invalid("the params are not what the method takes"))
}

fn key(text: &str) -> Result<RepositoryKey, ApiError> {
    RepositoryKey::parse(text).map_err(|error| invalid(&error.to_string()))
}

/// reads the method a request names and its params; a method without params ignores
/// whatever it is given.
pub fn parse(method: &str, value: serde_json::Value) -> Result<Parsed, ApiError> {
    let method = match method {
        "status" => Method::Status,
        "start_all" => Method::StartAll,
        "stop_all" => Method::StopAll,
        "build" => Method::Build,
        "quit" => Method::Quit,
        "subscribe" => {
            let SubscribeParams { topic } = params(value)?;
            return Ok(Parsed::Subscribe(Topic::of(&topic)?));
        }
        "renew" => {
            let SubscribeParams { topic } = params(value)?;
            if topic != TOPIC_GITHUB {
                return Err(invalid("only a github subscription is renewed"));
            }
            return Ok(Parsed::Renew(Topic::Github));
        }
        "token_set" => {
            let TokenSetParams { repository, token } = params(value)?;
            let key = key(&repository)?;
            let token = accepted_token(&token).map_err(refused_token)?;
            Method::TokenSet { key, token }
        }
        "token_remove" => {
            let TokenRemoveParams { repository } = params(value)?;
            Method::TokenRemove {
                key: key(&repository)?,
            }
        }
        "drain_for_update" => {
            let DrainParams { version } = params(value)?;
            if !is_version(&version) {
                return Err(invalid("the version is not a version number"));
            }
            Method::DrainForUpdate {
                version,
                requester: None,
            }
        }
        _ => {
            return Err(ApiError::new(
                ErrorCode::UnknownMethod,
                "the agent has no such method",
            ));
        }
    };
    Ok(Parsed::Call(method))
}

fn refused_token(refusal: TokenRefusal) -> ApiError {
    ApiError::new(ErrorCode::TokenInvalid, refusal.to_string())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn code(method: &str, value: serde_json::Value) -> ErrorCode {
        parse(method, value).unwrap_err().code
    }

    #[test]
    fn the_methods_without_params_are_read_with_or_without_them() {
        for method in ["status", "start_all", "stop_all", "build", "quit"] {
            assert!(matches!(parse(method, json!(null)), Ok(Parsed::Call(_))));
            assert!(matches!(
                parse(method, json!({"later": 1})),
                Ok(Parsed::Call(_))
            ));
        }
    }

    #[test]
    fn subscribe_takes_the_state_and_github_topics() {
        assert!(matches!(
            parse("subscribe", json!({"topic": "state"})),
            Ok(Parsed::Subscribe(Topic::State))
        ));
        assert!(matches!(
            parse("subscribe", json!({"topic": "github"})),
            Ok(Parsed::Subscribe(Topic::Github))
        ));
        assert_eq!(
            code("subscribe", json!({"topic": "later"})),
            ErrorCode::InvalidRequest
        );
        assert_eq!(code("subscribe", json!(null)), ErrorCode::InvalidRequest);
    }

    #[test]
    fn renew_takes_the_github_topic_only() {
        assert!(matches!(
            parse("renew", json!({"topic": "github"})),
            Ok(Parsed::Renew(Topic::Github))
        ));
        assert_eq!(
            code("renew", json!({"topic": "state"})),
            ErrorCode::InvalidRequest
        );
        assert_eq!(code("renew", json!(null)), ErrorCode::InvalidRequest);
    }

    #[test]
    fn token_set_reads_the_key_in_lower_case_and_trims_the_token() {
        let parsed = parse(
            "token_set",
            json!({"repository": "Owner/Repo", "token": "  ghp_fake  "}),
        )
        .unwrap();
        let Parsed::Call(Method::TokenSet { key, token }) = parsed else {
            panic!("not a token_set");
        };
        assert_eq!(key.as_str(), "owner/repo");
        assert_eq!(token.expose(), "ghp_fake");
    }

    #[test]
    fn token_set_refuses_an_empty_or_oversize_token_and_a_bad_repository() {
        assert_eq!(
            code("token_set", json!({"repository": "o/r", "token": "  "})),
            ErrorCode::TokenInvalid
        );
        assert_eq!(
            code(
                "token_set",
                json!({"repository": "o/r", "token": "a".repeat(1281)})
            ),
            ErrorCode::TokenInvalid
        );
        assert_eq!(
            code("token_set", json!({"repository": "owner", "token": "x"})),
            ErrorCode::InvalidRequest
        );
        assert_eq!(
            code("token_set", json!({"repository": "o/r"})),
            ErrorCode::InvalidRequest
        );
    }

    #[test]
    fn an_error_for_a_malformed_token_request_repeats_no_param() {
        let error = parse(
            "token_set",
            json!({"repository": "o/r", "token": 123_456_789}),
        )
        .unwrap_err();
        assert!(!error.message.contains("123456789"), "{}", error.message);
    }

    #[test]
    fn drain_for_update_takes_a_version_number_only() {
        for version in ["1.4.0", "10.0.12", "1.4.0-rc.1", "1.4.0+build.7"] {
            let parsed = parse("drain_for_update", json!({ "version": version })).unwrap();
            let Parsed::Call(Method::DrainForUpdate { version: read, .. }) = parsed else {
                panic!("not a drain");
            };
            assert_eq!(read, version);
        }
        for version in [
            "",
            "1.4",
            "1.4.0.1",
            "v1.4.0",
            "1..0",
            "1.4.0-",
            "1.4.0 ",
            "1.4.0-a b",
        ] {
            assert_eq!(
                code("drain_for_update", json!({ "version": version })),
                ErrorCode::InvalidRequest,
                "{version:?}"
            );
        }
        assert_eq!(
            code("drain_for_update", json!({ "version": "9".repeat(65) })),
            ErrorCode::InvalidRequest
        );
        assert_eq!(
            code("drain_for_update", json!(null)),
            ErrorCode::InvalidRequest
        );
    }

    #[test]
    fn a_method_the_agent_lacks_is_unknown() {
        assert_eq!(code("diagnostics", json!(null)), ErrorCode::UnknownMethod);
    }
}
