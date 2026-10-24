//! The instance's own configuration file, as a [`Source`].

use std::path::PathBuf;

use crate::command::{LoadError, load_plan};
use crate::profile::Profile;
use crate::validate::Plan;

use super::lifecycle::Source;

/// Reads and validates one file, fixed when it is built. The instance builds it from
/// the path it started with, so a request can never make it read another.
#[derive(Debug, Clone)]
pub struct FileSource {
    path: PathBuf,
    profile: Profile,
}

impl FileSource {
    #[must_use]
    pub fn new(path: PathBuf, profile: Profile) -> Self {
        Self { path, profile }
    }
}

impl Source for FileSource {
    async fn load(&self) -> Result<Plan, Vec<String>> {
        load_plan(&self.path, self.profile)
            .outcome
            .map_err(LoadError::into_problems)
    }
}
