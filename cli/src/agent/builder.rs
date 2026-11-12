//! building the runner image for the agent: the seam the machine builds through, and
//! the Docker implementation that builds the definition embedded in this program as
//! `build` does.

use std::ffi::OsString;
use std::future::Future;
use std::path::PathBuf;

use tokio::sync::watch;
use tokio::time::Instant;

use crate::build::steps::{BuildLimits, BuildOutput, Steps};
use crate::image::archive::context_archive;
use crate::image::definition::resolve_definition;
use crate::runtime::log::Log;

/// builds an image until its name holds the new one; a stop asked for through `stop`
/// ends the build, and the error says why it ended and what the name holds.
pub trait ImageBuilder {
    fn build(
        &self,
        image: &str,
        log: &dyn Log,
        stop: watch::Receiver<Option<Instant>>,
    ) -> impl Future<Output = Result<(), String>>;
}

/// builds with the `docker` client.
pub struct DockerImageBuilder {
    pub program: OsString,
    pub environment: Vec<(OsString, OsString)>,
    /// names a test definition to build instead of the embedded one; a debug build
    /// only.
    pub definition: Option<PathBuf>,
}

impl ImageBuilder for DockerImageBuilder {
    async fn build(
        &self,
        image: &str,
        log: &dyn Log,
        stop: watch::Receiver<Option<Instant>>,
    ) -> Result<(), String> {
        let context = resolve_definition(self.definition.as_deref())
            .and_then(|definition| context_archive(&definition.files))?;
        Steps {
            program: self.program.clone(),
            environment: self.environment.clone(),
            image,
            context,
            log,
            limits: BuildLimits::default(),
            output: BuildOutput::Log,
            stop,
        }
        .run()
        .await
    }
}
