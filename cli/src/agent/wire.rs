//! the messages of protocol version 2, one JSON object per line; every field name
//! here is a promise within version 2, and a reader ignores a field it does not know;
//! `docs/operations/cli.md` describes them.

use serde::{Deserialize, Serialize};

use super::snapshot::Snapshot;

/// the protocol version this agent serves.
pub const VERSION: u32 = 2;

/// every version this agent serves, as the `unsupported_protocol` error lists them.
pub const SUPPORTED_VERSIONS: [u32; 1] = [VERSION];

/// the longest message a client sends, its line break included.
pub const MAX_CLIENT_MESSAGE_BYTES: usize = 16 * 1024;

/// the longest message the agent sends, its line break included.
pub const MAX_AGENT_MESSAGE_BYTES: usize = 64 * 1024;

/// the methods `welcome` lists.
pub const METHODS: [&str; 8] = [
    "status",
    "subscribe",
    "start_all",
    "stop_all",
    "build",
    "quit",
    "token_set",
    "token_remove",
];

/// the only topic of version 2's first slice: the snapshot `status` returns.
pub const TOPIC_STATE: &str = "state";

/// the topics `welcome` lists.
pub const TOPICS: [&str; 1] = [TOPIC_STATE];

/// the error codes of this slice; a later slice may add more, so a reader keeps a
/// code it does not know as text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    Busy,
    UnsupportedProtocol,
    UnknownMethod,
    InvalidRequest,
    SubscriptionLimit,
    TokenInvalid,
    StoreFailed,
    /// the state does not allow the method.
    Refused,
}

impl ErrorCode {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Busy => "busy",
            Self::UnsupportedProtocol => "unsupported_protocol",
            Self::UnknownMethod => "unknown_method",
            Self::InvalidRequest => "invalid_request",
            Self::SubscriptionLimit => "subscription_limit",
            Self::TokenInvalid => "token_invalid",
            Self::StoreFailed => "store_failed",
            Self::Refused => "refused",
        }
    }
}

/// an error as it travels.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorBody {
    pub code: String,
    pub message: String,
    /// the versions the agent serves, in `unsupported_protocol`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub versions: Vec<u32>,
    /// what a `store_failed` adds: the kind of the store's failure and its platform
    /// code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub store: Option<StoreFault>,
}

/// a token store failure as it travels; the kind is an open set (`needs_user`,
/// `access_refused`, `other`), and neither field ever holds a token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoreFault {
    pub kind: String,
    #[serde(default)]
    pub code: Option<i64>,
}

impl ErrorBody {
    #[must_use]
    pub fn new(code: ErrorCode, message: &str) -> Self {
        Self {
            code: code.as_str().to_string(),
            message: message.to_string(),
            versions: Vec::new(),
            store: None,
        }
    }

    #[must_use]
    pub fn unsupported_protocol() -> Self {
        Self {
            versions: SUPPORTED_VERSIONS.to_vec(),
            ..Self::new(
                ErrorCode::UnsupportedProtocol,
                "this agent speaks protocol version 2; send a hello message first",
            )
        }
    }
}

/// who the client is, for the log.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientInfo {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub version: String,
}

/// what a client sends.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    /// the first line: the versions the client speaks, and who it is.
    Hello {
        versions: Vec<u32>,
        #[serde(default)]
        client: ClientInfo,
    },
    /// a request; `id` is the client's own and comes back in the response.
    Request {
        id: u64,
        method: String,
        #[serde(default)]
        params: serde_json::Value,
    },
}

/// who the agent is.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentVersion {
    #[serde(default)]
    pub version: String,
}

/// what the agent sends, as a reader reads it.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentMessage {
    Welcome {
        version: u32,
        #[serde(default)]
        agent: AgentVersion,
        #[serde(default)]
        methods: Vec<String>,
        #[serde(default)]
        topics: Vec<String>,
    },
    Response {
        id: u64,
        #[serde(default)]
        result: Option<serde_json::Value>,
        #[serde(default)]
        error: Option<ErrorBody>,
    },
    /// an error that answers no request: a handshake that failed, or a connection
    /// beyond the limit.
    Error {
        error: ErrorBody,
    },
    Event {
        topic: String,
        seq: u64,
        value: Box<Snapshot>,
    },
    Heartbeat {
        seq: u64,
    },
    /// the agent is about to end; `reason` is an open set.
    Ending {
        reason: String,
    },
}

/// what the agent sends, as it writes it; borrowed, so a snapshot is not copied.
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage<'a> {
    Welcome {
        version: u32,
        agent: AgentVersion,
        methods: &'static [&'static str],
        topics: &'static [&'static str],
    },
    Response {
        id: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        result: Option<&'a serde_json::Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<&'a ErrorBody>,
    },
    Error {
        error: &'a ErrorBody,
    },
    Event {
        topic: &'static str,
        seq: u64,
        value: &'a Snapshot,
    },
    Heartbeat {
        seq: u64,
    },
    Ending {
        reason: &'a str,
    },
}

impl ServerMessage<'_> {
    /// the message as the line the agent writes, with its line break.
    #[must_use]
    pub fn line(&self) -> Vec<u8> {
        let mut line = serde_json::to_vec(self).unwrap_or_default();
        line.push(b'\n');
        line
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hello_is_read_and_a_field_it_does_not_know_is_ignored() {
        let line = br#"{"type":"hello","versions":[2,3],"client":{"name":"app","version":"1","extra":1},"later":true}"#;
        let message: ClientMessage = serde_json::from_slice(line).unwrap();
        assert_eq!(
            message,
            ClientMessage::Hello {
                versions: vec![2, 3],
                client: ClientInfo {
                    name: "app".to_string(),
                    version: "1".to_string()
                }
            }
        );
    }

    #[test]
    fn a_request_without_params_is_read_with_null_params() {
        let message: ClientMessage =
            serde_json::from_slice(br#"{"type":"request","id":7,"method":"status"}"#).unwrap();
        assert_eq!(
            message,
            ClientMessage::Request {
                id: 7,
                method: "status".to_string(),
                params: serde_json::Value::Null
            }
        );
    }

    #[test]
    fn a_line_that_is_not_a_message_is_not_read() {
        for line in ["status", "{}", r#"{"type":"nonsense"}"#, "[1]", ""] {
            assert!(
                serde_json::from_slice::<ClientMessage>(line.as_bytes()).is_err(),
                "{line:?}"
            );
        }
    }

    #[test]
    fn a_response_carries_a_result_or_an_error_and_never_both() {
        let result = serde_json::json!({"state": "running"});
        let line = ServerMessage::Response {
            id: 3,
            result: Some(&result),
            error: None,
        }
        .line();
        assert_eq!(
            String::from_utf8(line).unwrap(),
            "{\"type\":\"response\",\"id\":3,\"result\":{\"state\":\"running\"}}\n"
        );
        let error = ErrorBody::new(ErrorCode::Refused, "no");
        let line = ServerMessage::Response {
            id: 4,
            result: None,
            error: Some(&error),
        }
        .line();
        assert_eq!(
            String::from_utf8(line).unwrap(),
            "{\"type\":\"response\",\"id\":4,\"error\":{\"code\":\"refused\",\"message\":\"no\"}}\n"
        );
    }

    #[test]
    fn the_unsupported_protocol_error_lists_the_versions() {
        let error = ErrorBody::unsupported_protocol();
        assert_eq!(error.code, "unsupported_protocol");
        assert_eq!(error.versions, [2]);
    }

    #[test]
    fn every_error_code_has_its_documented_word() {
        let words: Vec<&str> = [
            ErrorCode::Busy,
            ErrorCode::UnsupportedProtocol,
            ErrorCode::UnknownMethod,
            ErrorCode::InvalidRequest,
            ErrorCode::SubscriptionLimit,
            ErrorCode::TokenInvalid,
            ErrorCode::StoreFailed,
            ErrorCode::Refused,
        ]
        .into_iter()
        .map(ErrorCode::as_str)
        .collect();
        assert_eq!(
            words,
            [
                "busy",
                "unsupported_protocol",
                "unknown_method",
                "invalid_request",
                "subscription_limit",
                "token_invalid",
                "store_failed",
                "refused"
            ]
        );
    }
}
