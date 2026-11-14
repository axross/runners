//! the messages of protocol version 2, one JSON object per line; every field name
//! here is a promise within version 2, and a reader ignores a field it does not know;
//! `docs/operations/cli.md` describes them.

use std::fmt;

use serde::{Deserialize, Serialize};

use super::config::Problem;
use super::snapshot::Snapshot;
use crate::github_status::GithubStatus;

/// the protocol version this agent serves.
pub const VERSION: u32 = 2;

/// every version this agent serves, as the `unsupported_protocol` error lists them.
pub const SUPPORTED_VERSIONS: [u32; 1] = [VERSION];

/// the longest message a client sends, its line break included.
pub const MAX_CLIENT_MESSAGE_BYTES: usize = 16 * 1024;

/// the longest message the agent sends, its line break included.
pub const MAX_AGENT_MESSAGE_BYTES: usize = 64 * 1024;

/// the methods `welcome` lists; `renew` keeps a `github` subscription alive.
pub const METHODS: [&str; 15] = [
    "status",
    "subscribe",
    "start_all",
    "stop_all",
    "build",
    "quit",
    "token_set",
    "token_remove",
    "drain_for_update",
    "renew",
    "config_read",
    "config_edit",
    "config_reload",
    "launch",
    "lifecycle_set",
];

/// the `ending` reason of an agent that ends for an update.
pub const ENDING_UPDATE: &str = "update";

/// what `ending` adds when the agent ends for an update: the version it ends for, and
/// how the runners stood, which the app keeps across the update: `runners` is
/// [`RUNNERS_RUNNING`] or the code of the stop, such as `by_you` or `slept`, and
/// `stopped_by_you` repeats whether that code is `by_you`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct UpdateEnding {
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub stopped_by_you: bool,
    #[serde(default)]
    pub runners: String,
}

/// what `runners` says when the runners were running, or would have been without an
/// error stop, so the new app starts them.
pub const RUNNERS_RUNNING: &str = "running";

impl UpdateEnding {
    /// the ending of runners that were running, or stopped by you.
    #[must_use]
    pub fn new(version: &str, stopped_by_you: bool) -> Self {
        let runners = if stopped_by_you {
            super::state::Reason::BY_YOU
        } else {
            RUNNERS_RUNNING
        };
        Self::with_runners(version, runners)
    }

    /// the ending whose runners stood as `runners` says.
    #[must_use]
    pub fn with_runners(version: &str, runners: &str) -> Self {
        Self {
            version: version.to_string(),
            stopped_by_you: runners == super::state::Reason::BY_YOU,
            runners: runners.to_string(),
        }
    }

    /// whether the runners stay stopped after the update: anything but running, or
    /// stopped by you from an agent that does not say `runners`.
    #[must_use]
    pub fn keeps_stopped(&self) -> bool {
        if self.runners.is_empty() {
            self.stopped_by_you
        } else {
            self.runners != RUNNERS_RUNNING
        }
    }
}

/// why the agent is about to end, as every connection is told.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndingNotice {
    pub reason: String,
    pub update: Option<UpdateEnding>,
}

/// the snapshot `status` returns.
pub const TOPIC_STATE: &str = "state";

/// what GitHub says about this host's runners, collected only while a subscription
/// to it is renewed.
pub const TOPIC_GITHUB: &str = "github";

/// the topics `welcome` lists.
pub const TOPICS: [&str; 2] = [TOPIC_STATE, TOPIC_GITHUB];

/// the error codes of this slice; a later slice may add more, so a reader keeps a
/// code it does not know as text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
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
    /// a `renew` for a subscription the connection does not hold, such as one that
    /// ended for want of renewal.
    NotSubscribed,
    /// the file no longer holds the revision the request names.
    ConfigChanged,
    /// the result breaks the rules; the error lists the problems.
    ConfigInvalid,
    /// the file cannot be parsed, is missing or unreadable, or a migration failed or is
    /// pending.
    ConfigUnusable,
    /// the file could not be replaced.
    ConfigUnwritable,
    /// an edit touches `schema_version` or `lifecycle`.
    ConfigRefused,
    /// the file is written in a form only the user can change, such as an inline
    /// table over several lines.
    ConfigFileForm,
    /// an edit is being applied.
    ConfigApplying,
    /// the runtime core waits to restart after a failure.
    RuntimeRestarting,
    /// the image a change names is missing or unusable.
    ImageUnusable,
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
            Self::NotSubscribed => "not_subscribed",
            Self::ConfigChanged => "config_changed",
            Self::ConfigInvalid => "config_invalid",
            Self::ConfigUnusable => "config_unusable",
            Self::ConfigUnwritable => "config_unwritable",
            Self::ConfigRefused => "config_refused",
            Self::ConfigFileForm => "config_file_form",
            Self::ConfigApplying => "config_applying",
            Self::RuntimeRestarting => "runtime_restarting",
            Self::ImageUnusable => "image_unusable",
        }
    }
}

/// an error as it travels.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ErrorBody {
    pub code: String,
    pub message: String,
    /// the versions the agent serves, in `unsupported_protocol`.
    #[serde(default, skip_serializing_if = "<[u32]>::is_empty")]
    pub versions: Box<[u32]>,
    /// what a `store_failed` adds: the kind of the store's failure and its platform
    /// code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub store: Option<StoreFault>,
    /// what a `config_invalid` lists, each with its TOML key path and line when it has
    /// them; a problem never quotes the file.
    #[serde(default, skip_serializing_if = "<[Problem]>::is_empty")]
    pub problems: Box<[Problem]>,
}

/// a token store failure as it travels; the kind is an open set (`needs_user`,
/// `access_refused`, `other`), and neither field ever holds a token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
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
            versions: Box::default(),
            store: None,
            problems: Box::default(),
        }
    }

    #[must_use]
    pub fn unsupported_protocol() -> Self {
        Self {
            versions: SUPPORTED_VERSIONS.into(),
            ..Self::new(
                ErrorCode::UnsupportedProtocol,
                "this agent speaks protocol version 2; send a hello message first",
            )
        }
    }
}

/// who the client is: its name and version, which the agent reads and ignores today.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ClientInfo {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub version: String,
}

/// what a client sends. `Debug` leaves out a request's params, since those of a
/// `token_set` hold a token.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
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

impl fmt::Debug for ClientMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Hello { versions, client } => f
                .debug_struct("Hello")
                .field("versions", versions)
                .field("client", client)
                .finish(),
            Self::Request { id, method, .. } => f
                .debug_struct("Request")
                .field("id", id)
                .field("method", method)
                .field("params", &"<redacted>")
                .finish(),
        }
    }
}

/// who the agent is.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct AgentVersion {
    #[serde(default)]
    pub version: String,
}

/// what the agent sends, as a reader reads it.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
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
    /// a topic's value; the reader reads it by its topic, as [`Self::snapshot`] and
    /// [`Self::github`] do.
    Event {
        topic: String,
        seq: u64,
        value: serde_json::Value,
    },
    Heartbeat {
        seq: u64,
    },
    /// the agent is about to end; `reason` is an open set, and `update` is there when
    /// it ends for an update.
    Ending {
        reason: String,
        #[serde(default)]
        update: Option<UpdateEnding>,
    },
}

impl AgentMessage {
    /// the snapshot an event of the `state` topic carries.
    #[must_use]
    pub fn snapshot(topic: &str, value: &serde_json::Value) -> Option<Box<Snapshot>> {
        (topic == TOPIC_STATE)
            .then(|| Snapshot::deserialize(value).ok().map(Box::new))
            .flatten()
    }

    /// the value an event of the `github` topic carries.
    #[must_use]
    pub fn github(topic: &str, value: &serde_json::Value) -> Option<Box<GithubStatus>> {
        (topic == TOPIC_GITHUB)
            .then(|| GithubStatus::deserialize(value).ok().map(Box::new))
            .flatten()
    }
}

/// a topic's value as the agent writes it.
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum EventValue<'a> {
    State(&'a Snapshot),
    Github(&'a GithubStatus),
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
        value: EventValue<'a>,
    },
    Heartbeat {
        seq: u64,
    },
    Ending {
        reason: &'a str,
        #[serde(skip_serializing_if = "Option::is_none")]
        update: Option<&'a UpdateEnding>,
    },
}

impl ServerMessage<'_> {
    /// the message as the line the agent writes, with its line break; `None` when it
    /// cannot be serialised, in which case nothing is written.
    #[must_use]
    pub fn line(&self) -> Option<Vec<u8>> {
        let mut line = serde_json::to_vec(self).ok()?;
        line.push(b'\n');
        Some(line)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_debug_form_of_a_request_leaves_out_its_params() {
        let line = br#"{"type":"request","id":3,"method":"token_set","params":{"repository":"o/r","token":"fake-token-for-the-test"}}"#;
        let message: ClientMessage = serde_json::from_slice(line).unwrap();
        for form in [format!("{message:?}"), format!("{message:#?}")] {
            assert!(!form.contains("fake-token-for-the-test"), "{form}");
            assert!(form.contains("token_set"), "{form}");
            assert!(form.contains("<redacted>"), "{form}");
        }
    }

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
            String::from_utf8(line.unwrap()).unwrap(),
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
            String::from_utf8(line.unwrap()).unwrap(),
            "{\"type\":\"response\",\"id\":4,\"error\":{\"code\":\"refused\",\"message\":\"no\"}}\n"
        );
    }

    #[test]
    fn an_ending_for_an_update_carries_its_field_and_another_does_not() {
        let update = UpdateEnding::new("1.4.0", true);
        let line = ServerMessage::Ending {
            reason: ENDING_UPDATE,
            update: Some(&update),
        }
        .line()
        .unwrap();
        assert_eq!(
            String::from_utf8(line.clone()).unwrap(),
            "{\"type\":\"ending\",\"reason\":\"update\",\"update\":{\"version\":\"1.4.0\",\"stopped_by_you\":true,\"runners\":\"by_you\"}}\n"
        );
        let read: AgentMessage = serde_json::from_slice(&line).unwrap();
        assert_eq!(
            read,
            AgentMessage::Ending {
                reason: "update".to_string(),
                update: Some(update)
            }
        );
        let quit = ServerMessage::Ending {
            reason: "quit",
            update: None,
        }
        .line()
        .unwrap();
        assert_eq!(
            String::from_utf8(quit).unwrap(),
            "{\"type\":\"ending\",\"reason\":\"quit\"}\n"
        );
    }

    #[test]
    fn the_unsupported_protocol_error_lists_the_versions() {
        let error = ErrorBody::unsupported_protocol();
        assert_eq!(error.code, "unsupported_protocol");
        assert_eq!(*error.versions, [2]);
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
            ErrorCode::NotSubscribed,
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
                "refused",
                "not_subscribed"
            ]
        );
    }
}
