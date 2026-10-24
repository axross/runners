//! The instance's own configuration file, as a [`Source`].

use std::path::PathBuf;

use crate::checkout;
use crate::command::read_limited;
use crate::profile::Profile;
use crate::validate::{Failure, Plan, validate};

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
        let bytes = read_limited(&self.path).map_err(|error| {
            vec![format!(
                "Host configuration cannot be read: {}: {error}",
                self.path.display()
            )]
        })?;
        let checkout = checkout::find(&self.path);
        match validate(&bytes, self.profile, checkout.as_deref()).outcome {
            Ok(plan) => Ok(plan),
            Err(Failure::Unreadable(problem)) => Err(vec![format!(
                "Host configuration is not valid JSON: {problem}"
            )]),
            Err(Failure::Invalid(errors)) => Err(errors),
        }
    }
}
