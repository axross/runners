use std::fmt;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use reqwest::Url;
use serde::Deserialize;

use crate::runtime::registration::RUNNER_GROUP_ID;
use crate::runtime::secret::Secret;

use super::transport::{Error, ErrorKind, decode};

pub(crate) const ADMIN_RENEWAL_MARGIN: Duration = Duration::from_secs(60);

/// credentials for one repository, bound to the PAT that produced them; URLs and tokens stay private.
pub struct Connection {
    pub(super) owner: String,
    pub(super) repository: String,
    pub(super) pat: Secret,
    pub(super) admin: Secret,
    pub(super) service_url: Url,
    pub(super) expires_at: SystemTime,
}

impl Connection {
    /// the driver must renew before using a replaced PAT or an expiring admin credential.
    #[must_use]
    pub fn needs_renewal(&self, pat: &Secret, now: SystemTime) -> bool {
        self.pat.expose() != pat.expose()
            || now
                .checked_add(ADMIN_RENEWAL_MARGIN)
                .is_none_or(|time| time >= self.expires_at)
    }

    #[must_use]
    pub fn expires_at(&self) -> SystemTime {
        self.expires_at
    }

    #[must_use]
    pub fn owner(&self) -> &str {
        &self.owner
    }

    #[must_use]
    pub fn repository(&self) -> &str {
        &self.repository
    }

    pub(crate) fn same_service(&self, other: &Self) -> bool {
        self.owner.eq_ignore_ascii_case(&other.owner)
            && self.repository.eq_ignore_ascii_case(&other.repository)
            && self.service_url == other.service_url
    }

    /// appends the official runtime route to the tenant path and pins the preview version.
    pub(super) fn service_url(&self, route: &str) -> Result<Url, Error> {
        let mut url = self.service_url.clone();
        let path = format!("{}/_apis/runtime/{route}", url.path().trim_end_matches('/'));
        url.set_path(&path);
        let retained: Vec<_> = url
            .query_pairs()
            .filter(|(key, _)| key != "api-version")
            .map(|(key, value)| (key.into_owned(), value.into_owned()))
            .collect();
        url.set_query(None);
        url.query_pairs_mut().extend_pairs(retained);
        url.query_pairs_mut()
            .append_pair("api-version", "6.0-preview");
        Ok(url)
    }
}

impl fmt::Debug for Connection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Connection(<redacted>)")
    }
}

/// a group-1 set whose sole label has passed routing-compatibility validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScaleSet {
    id: u64,
    name: String,
}

impl ScaleSet {
    #[must_use]
    pub fn id(&self) -> u64 {
        self.id
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
}

/// only fields used to establish routing compatibility are retained.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ScaleSetWire {
    id: u64,
    name: String,
    runner_group_id: u64,
    #[serde(default)]
    labels: Vec<Label>,
}

#[derive(Deserialize)]
struct Label {
    name: String,
}

impl ScaleSetWire {
    /// refuses an incompatible set without returning its server-supplied labels in diagnostics.
    pub(super) fn validate(self, name: &str) -> Result<ScaleSet, Error> {
        if self.labels.len() != 1
            || !self
                .labels
                .first()
                .is_some_and(|label| label.name.eq_ignore_ascii_case(name))
        {
            return Err(Error::new(ErrorKind::LabelMismatch));
        }
        if self.id == 0
            || self.runner_group_id != RUNNER_GROUP_ID
            || !self.name.eq_ignore_ascii_case(name)
        {
            return Err(Error::protocol());
        }
        Ok(ScaleSet {
            id: self.id,
            name: name.to_owned(),
        })
    }
}

/// queue credentials plus initial demand; the driver owns message progress and demand reconciliation.
pub struct Session {
    pub(super) id: String,
    pub(super) scale_set_id: u64,
    pub(super) queue_url: Url,
    pub(super) queue_token: Secret,
    pub statistics: Statistics,
}

impl Session {
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    #[must_use]
    pub fn scale_set_id(&self) -> u64 {
        self.scale_set_id
    }

    pub(super) fn route(&self) -> String {
        format!("runnerscalesets/{}/sessions/{}", self.scale_set_id, self.id)
    }
}

impl fmt::Debug for Session {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Session")
            .field("scale_set_id", &self.scale_set_id)
            .field("statistics", &self.statistics)
            .finish_non_exhaustive()
    }
}

/// statistics are mandatory, so a new session can reconcile before any job event arrives.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SessionWire {
    pub session_id: String,
    pub message_queue_url: String,
    pub message_queue_access_token: String,
    pub statistics: Statistics,
}

/// authoritative demand and display counts from a session or message, never an event tally.
#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Statistics {
    #[serde(default)]
    pub total_available_jobs: u64,
    #[serde(default)]
    pub total_acquired_jobs: u64,
    pub total_assigned_jobs: u64,
    pub total_running_jobs: u64,
    #[serde(default)]
    pub total_registered_runners: u64,
    #[serde(default)]
    pub total_busy_runners: u64,
    pub total_idle_runners: u64,
}

/// the lifecycle events understood by the pinned protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobEventKind {
    Available,
    Assigned,
    Started,
    Completed,
}

/// an offer identity for acquisition/replay; job text and acquire URLs are not needed or exposed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobEvent {
    pub kind: JobEventKind,
    pub request_id: u64,
}

/// a batch whose ID may advance only after its explicit acknowledgement succeeds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub id: u64,
    pub statistics: Statistics,
    pub jobs: Vec<JobEvent>,
}

/// rejects unsupported envelopes/events rather than allowing the driver to acknowledge lost work.
pub(super) fn message(body: &[u8]) -> Result<Message, Error> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Envelope {
        message_id: u64,
        message_type: String,
        body: String,
        statistics: Statistics,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Event {
        message_type: String,
        runner_request_id: u64,
    }
    let envelope: Envelope = decode(body)?;
    if envelope.message_type != "RunnerScaleSetJobMessages" || envelope.message_id == 0 {
        return Err(Error::protocol());
    }
    let events: Vec<Event> = if envelope.body.is_empty() {
        Vec::new()
    } else {
        decode(envelope.body.as_bytes())?
    };
    let mut jobs = Vec::new();
    for event in events {
        let kind = match event.message_type.as_str() {
            "JobAvailable" => JobEventKind::Available,
            "JobAssigned" => JobEventKind::Assigned,
            "JobStarted" => JobEventKind::Started,
            "JobCompleted" => JobEventKind::Completed,
            _ => return Err(Error::protocol()),
        };
        jobs.push(JobEvent {
            kind,
            request_id: event.runner_request_id,
        });
    }
    Ok(Message {
        id: envelope.message_id,
        statistics: envelope.statistics,
        jobs,
    })
}

/// registration ownership survives launch failure; only `config` is secret.
#[derive(Debug)]
pub struct JitRunner {
    pub id: u64,
    pub name: String,
    pub config: Secret,
}

/// a REST runner identity; the driver applies its exact offline crash-cleanup naming rules.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct Runner {
    pub id: u64,
    pub name: String,
    pub status: String,
    pub busy: bool,
}

/// limits a session identity to a UUID before it can enter a request path.
pub(super) fn is_session_id(id: &str) -> bool {
    id.len() == 36
        && id.bytes().enumerate().all(|(index, byte)| {
            if [8, 13, 18, 23].contains(&index) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
}

/// reads expiry without signature verification; authenticity rests on the TLS exchange, not these claims.
pub(super) fn token_expiry(token: &Secret) -> Result<SystemTime, Error> {
    let mut parts = token.expose().split('.');
    let (_, Some(payload), Some(_), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(Error::protocol());
    };
    let mut decoded = Vec::new();
    let mut buffer = 0_u32;
    let mut bits = 0;
    for byte in payload.bytes() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'-' => 62,
            b'_' => 63,
            _ => return Err(Error::protocol()),
        };
        buffer = (buffer << 6) | u32::from(value);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            decoded.push((buffer >> bits) as u8);
        }
    }
    #[derive(Deserialize)]
    struct Claims {
        exp: u64,
    }
    let claims: Claims = decode(&decoded)?;
    UNIX_EPOCH
        .checked_add(Duration::from_secs(claims.exp))
        .ok_or_else(Error::protocol)
}
