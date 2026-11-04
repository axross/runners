//! where the agent is and what it knows about its configuration, for the snapshot.

use std::collections::BTreeMap;
use std::time::SystemTime;

use crate::image::check::ImageCheck;
use crate::validate::Plan;

use super::snapshot::{BuildInfo, Reason as WireReason};

/// why the agent is stopped; the codes are an open set on the wire.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reason {
    pub code: &'static str,
    pub message: String,
}

impl Reason {
    pub const BY_YOU: &'static str = "by_you";
    pub const NO_CONFIGURATION: &'static str = "no_configuration";
    pub const INVALID_CONFIGURATION: &'static str = "invalid_configuration";
    pub const IMAGE_UNUSABLE: &'static str = "image_unusable";
    pub const NO_POOL_STARTED: &'static str = "no_pool_started";
    pub const RESTART_LIMIT: &'static str = "restart_limit";
    pub const IMAGE_BUILD_FAILED: &'static str = "image_build_failed";
    pub const IMAGE_READY: &'static str = "image_ready";

    #[must_use]
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    #[must_use]
    pub fn by_you() -> Self {
        Self::new(Self::BY_YOU, "stopped by you")
    }

    pub(crate) fn wire(&self) -> WireReason {
        WireReason {
            code: self.code.to_string(),
            message: self.message.clone(),
        }
    }
}

/// where the agent is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Phase {
    Running,
    Stopping,
    Resuming,
    Building,
    Quitting,
    Stopped(Reason),
}

impl Phase {
    /// the word the snapshot and a refusal use.
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Stopping => "stopping",
            Self::Resuming => "resuming",
            Self::Building => "building",
            Self::Quitting => "quitting",
            Self::Stopped(_) => "stopped",
        }
    }
}

/// what the agent keeps between its transitions.
#[derive(Debug)]
pub struct Inner {
    pub phase: Phase,
    pub since: SystemTime,
    /// the configuration last read, or `None` when the latest read failed.
    pub plan: Option<Plan>,
    pub problems: Vec<String>,
    /// why each pool was skipped when the runners last stopped.
    pub skips: BTreeMap<String, String>,
    pub build: Option<BuildInfo>,
    pub restart_due: Option<SystemTime>,
    /// the image the latest check named, and what the check found.
    pub image: Option<(String, Option<ImageCheck>)>,
}

impl Inner {
    #[must_use]
    pub fn new(phase: Phase) -> Self {
        Self {
            phase,
            since: SystemTime::now(),
            plan: None,
            problems: Vec::new(),
            skips: BTreeMap::new(),
            build: None,
            restart_due: None,
            image: None,
        }
    }
}
