//! building the runner image for the agent: the seam the machine builds through, the
//! Docker implementation that builds the definition embedded in this program as
//! `build` does on axpc, and the golden VM builds that `build` runs on axmc.

use std::ffi::OsString;
use std::future::Future;
use std::path::PathBuf;

use tokio::sync::watch;
use tokio::time::Instant;

use crate::build::golden_vms;
use crate::build::steps::{BuildLimits, BuildOutput, Steps};
use crate::image::archive::context_archive;
use crate::image::definition::resolve_definition;
use crate::macos::count::ConfiguredPool;
use crate::runtime::log::Log;
use crate::runtime::tart::{TartClient, TartConfig, VmLauncher};

/// builds an image until its name holds the new one; a stop asked for through `stop`
/// ends the build, and the error says why it ended and what the name holds. `pools`
/// are the configuration's pools, which tell an axmc build which golden VMs to make.
pub trait ImageBuilder {
    fn build(
        &self,
        image: &str,
        pools: &[ConfiguredPool],
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
        _pools: &[ConfiguredPool],
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

/// builds the golden VMs with Tart, as `build` does for axmc: M1's golden Linux VM for
/// a Linux pool and M3's golden macOS VM for a macOS pool. The build VMs run under
/// `launcher`, which is the VM shim when the agent has one.
pub struct GoldenImageBuilder {
    pub tart: TartConfig,
    pub launcher: VmLauncher,
    /// names a test definition to build instead of the embedded one; a debug build
    /// only.
    pub definition: Option<PathBuf>,
}

impl ImageBuilder for GoldenImageBuilder {
    async fn build(
        &self,
        image: &str,
        pools: &[ConfiguredPool],
        log: &dyn Log,
        stop: watch::Receiver<Option<Instant>>,
    ) -> Result<(), String> {
        let context = resolve_definition(self.definition.as_deref())
            .and_then(|definition| context_archive(&definition.files))?;
        // boxed: the whole golden build is a large future
        Box::pin(golden_vms(
            TartClient::new(self.tart.clone()),
            self.launcher.clone(),
            image,
            context,
            pools.to_vec(),
            log,
            stop,
        ))
        .await
    }
}
