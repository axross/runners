//! loads the host configuration the agent runs: the TOML file `config.toml` in the
//! configuration folder, after a migration of `config.json` when only that exists, and
//! again at each Start all, where a file with problems leaves the configuration the
//! agent already holds in use. what each load returns is plain data, for the agent to
//! show; see `docs/operations/cli.md`.
//!
//! the module reads each file only at the start, at Start all and during the
//! migration, up to 1 MiB and one byte. it writes nothing but a new `config.toml`, and
//! removes the temporary files an interrupted write left. it takes the folder from the
//! caller and the rules that check the files from a [`Checks`], so the caller decides
//! the checkout rule.
//!
//! three choices the plan leaves open:
//!
//! - an error of the system while reading `config.toml` is [`FileStatus::Unreadable`],
//!   and so is a file the validator cannot read as a TOML document, which is
//!   `Failure::Unreadable` in the validator's result; a document that breaks rules,
//!   `Failure::Invalid`, is [`FileStatus::Invalid`].
//! - a migration that failed stays [`Migration::Failed`] at a later Start all while
//!   `config.toml` is still missing; [`Migration::Pending`] is for a `config.json` that
//!   this process did not try or did not fail to migrate. once a file is there, it is
//!   read alone and a failed or pending migration no longer applies.
//! - a `config.json` that cannot be read, is over 1 MiB or breaks today's rules fails
//!   the migration as [`MigrationFailure::JsonRejected`], with nothing in use.

mod checks;
mod load;

use std::time::SystemTime;

use crate::validate::Plan;

pub use checks::{Checks, StandardChecks};
pub use load::{JSON_FILE_NAME, load_at_start, load_at_start_all};

/// what reading `config.toml` found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileStatus {
    /// it passes the rules.
    Valid,
    /// it is a TOML document that breaks rules.
    Invalid,
    /// no file of that name exists, or after a migration that failed, none was written.
    Missing,
    /// the system could not read it, or it is not a TOML document the validator reads.
    Unreadable,
}

/// where the configuration in use came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// `config.toml`, read at the latest load.
    File,
    /// a valid `config.toml` read earlier, kept because the file read at Start all has
    /// problems or is missing.
    LastValid,
    /// `config.json`, because a migration to `config.toml` failed.
    Json,
}

/// the configuration the agent runs.
#[derive(Debug, Clone, PartialEq)]
pub struct InUse {
    pub origin: Origin,
    pub plan: Plan,
    /// when the configuration was read, which a later load that keeps it does not
    /// change.
    pub read_at: SystemTime,
}

/// why a migration ended without a `config.toml` of its own writing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MigrationFailure {
    /// `config.json` could not be read or breaks today's rules; the problems are in
    /// today's JSON wording, and nothing is in use.
    JsonRejected(Vec<String>),
    /// the converted document failed its check, and nothing was written; the text says
    /// how. the configuration in use is `config.json`'s.
    CheckFailed(String),
    /// `config.toml` could not be written; the text says why. the configuration in use
    /// is `config.json`'s.
    WriteFailed(String),
}

/// the state of the migration of `config.json` to `config.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Migration {
    /// none applies: `config.toml` existed, or neither file did.
    None,
    /// this process wrote `config.toml` from `config.json` at the time.
    Migrated {
        at: SystemTime,
    },
    Failed(MigrationFailure),
    /// `config.toml` is missing and `config.json` exists, and the next start of the
    /// agent migrates it.
    Pending,
}

/// the result of a load.
#[derive(Debug, Clone, PartialEq)]
pub struct Loaded {
    pub file: FileStatus,
    /// the configuration the agent runs, if there is one.
    pub in_use: Option<InUse>,
    pub migration: Migration,
    /// the problems of `config.toml`, in the validator's order, as text; a failed
    /// migration carries its own.
    pub problems: Vec<String>,
}
