//! the answers of the configuration methods as a client reads them. `status` and
//! `change` are open sets, kept as the agent's words, so a reader treats a word it
//! does not know as unknown rather than failing on it.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::Problem;

/// a `config_read` answer.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[non_exhaustive]
pub struct ConfigRead {
    pub revision: String,
    /// `valid`, `invalid`, `missing` or `unreadable`.
    pub status: String,
    #[serde(default)]
    pub settings: Map<String, Value>,
    #[serde(default)]
    pub pools: Vec<Value>,
    #[serde(default)]
    pub problems: Vec<Problem>,
    /// the values would not fit one message, so none were sent.
    #[serde(default)]
    pub too_large: bool,
}

/// a `config_edit` or `config_reload` answer.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[non_exhaustive]
pub struct ConfigChange {
    pub revision: String,
    #[serde(default)]
    pub effects: Vec<PoolEffect>,
    /// the repositories no pool names after the change.
    #[serde(default)]
    pub repositories_released: Vec<String>,
    /// whether the change applies now, which only a running agent's apply does.
    #[serde(default)]
    pub applies: bool,
}

/// what a change does to one pool.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[non_exhaustive]
pub struct PoolEffect {
    pub pool: String,
    /// `unchanged`, `added`, `removed`, `runners_added`, `runners_removed` or
    /// `restarted`.
    pub change: String,
    #[serde(default)]
    pub runners_before: u32,
    #[serde(default)]
    pub runners_after: u32,
    /// the change stops runners the pool has, which fails the jobs on them.
    #[serde(default)]
    pub stops_runners: bool,
}
