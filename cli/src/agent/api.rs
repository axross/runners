//! what a connection asks of the agent, parsed from the wire: the methods that reach
//! the agent's one task, and the errors they answer with.

use serde::Deserialize;
use tokio::sync::oneshot;

use crate::runtime::secret::Secret;
use crate::token_store::{RepositoryKey, StoreError, TokenRefusal, accepted_token};

use super::wire::{ErrorBody, ErrorCode, StoreFault, TOPIC_STATE};

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
    TokenSet { key: RepositoryKey, token: Secret },
    TokenRemove { key: RepositoryKey },
}

/// a method and where its result goes.
#[derive(Debug)]
pub struct Call {
    pub method: Method,
    pub reply: oneshot::Sender<ApiResult>,
}

/// a request as the connection understands it.
#[derive(Debug)]
pub enum Parsed {
    Subscribe,
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
            if topic != TOPIC_STATE {
                return Err(invalid("the agent serves the topic state only"));
            }
            return Ok(Parsed::Subscribe);
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
    fn subscribe_takes_the_state_topic_only() {
        assert!(matches!(
            parse("subscribe", json!({"topic": "state"})),
            Ok(Parsed::Subscribe)
        ));
        assert_eq!(
            code("subscribe", json!({"topic": "later"})),
            ErrorCode::InvalidRequest
        );
        assert_eq!(code("subscribe", json!(null)), ErrorCode::InvalidRequest);
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
    fn a_method_the_agent_lacks_is_unknown() {
        assert_eq!(code("diagnostics", json!(null)), ErrorCode::UnknownMethod);
    }
}
