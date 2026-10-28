//! the instance's own configuration file, as a [`Source`].

use std::path::PathBuf;

use crate::command::{LoadError, checkout_rule_note, load_plan};
use crate::profile::Profile;
use crate::runtime::log::Log;
use crate::validate::Plan;

use super::lifecycle::Source;

/// reads and validates one file, fixed when it is built from the path the instance
/// started with, so a request can never make it read another.
pub struct FileSource<'a> {
    path: PathBuf,
    profile: Profile,
    log: &'a dyn Log,
}

impl<'a> FileSource<'a> {
    /// a source for `path`; the note that the checkout rule was skipped goes to
    /// `log` at each read.
    #[must_use]
    pub fn new(path: PathBuf, profile: Profile, log: &'a dyn Log) -> Self {
        Self { path, profile, log }
    }
}

impl Source for FileSource<'_> {
    async fn load(&self) -> Result<Plan, Vec<String>> {
        let loaded = load_plan(&self.path, self.profile);
        if loaded.checkout_rule_skipped {
            self.log.info(&checkout_rule_note(&self.path));
        }
        loaded.outcome.map_err(LoadError::into_problems)
    }
}
