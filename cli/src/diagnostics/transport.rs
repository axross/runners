//! how the exporter reaches a diagnostic container's evidence; `docker cp` is the
//! first transport, and `docs/operations/cli.md` describes the seam.

use std::ffi::OsStr;
use std::process::Stdio;

use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Child;

use crate::runtime::docker::client_command;
use crate::runtime::docker_args::{copy_arguments, identity_arguments};

use super::archive::{ArchiveError, EntrySink, read_archive};
use super::identity::{MAX_IDENTITY_BYTES, identity_text};

/// the most bytes a copy client may print after the end of its archive.
const MAX_TRAILER_BYTES: u64 = 1024 * 1024;

/// reads a retained diagnostic source's evidence for the exporter; the exporter
/// bounds the time a call takes by ending the transport's work when it is dropped.
pub trait EvidenceTransport {
    /// the allowlisted identity text of `source`, or `None` when it cannot be read
    /// within its cap.
    fn read_identity(&self, source: &str) -> impl Future<Output = Option<String>>;

    /// streams `source`'s evidence archive into `sink` through the strict archive
    /// reader, and returns `None` when the transfer or the archive failed; the sink
    /// is dropped, and its file closed, before the call returns.
    fn copy_archive<S: EntrySink>(
        &self,
        source: &str,
        raw_records: bool,
        sink: S,
    ) -> impl Future<Output = Option<()>>;
}

/// the transport that runs `docker inspect` and `docker cp` against a container.
pub struct DockerCopy<'a> {
    docker: &'a OsStr,
}

impl<'a> DockerCopy<'a> {
    #[must_use]
    pub fn new(docker: &'a OsStr) -> Self {
        Self { docker }
    }

    fn client(&self, arguments: &[String]) -> Option<Child> {
        let mut command = client_command(self.docker, &[], arguments);
        command.stdout(Stdio::piped()).stderr(Stdio::null());
        command.spawn().ok()
    }
}

impl EvidenceTransport for DockerCopy<'_> {
    async fn read_identity(&self, source: &str) -> Option<String> {
        let mut inspect = self.client(&identity_arguments(source))?;
        let stdout = inspect.stdout.take()?;
        let output = read_capped(stdout, MAX_IDENTITY_BYTES).await?;
        let text = identity_text(&output).ok()?;
        inspect.wait().await.ok()?.success().then_some(text)
    }

    async fn copy_archive<S: EntrySink>(
        &self,
        source: &str,
        raw_records: bool,
        mut sink: S,
    ) -> Option<()> {
        let mut copy = self.client(&copy_arguments(source))?;
        let mut stdout = copy.stdout.take()?;
        read_archive(&mut stdout, raw_records, &mut sink)
            .await
            .map_err(|_: ArchiveError| ())
            .ok()?;
        drop(sink);
        let mut trailer = Vec::new();
        (&mut stdout)
            .take(MAX_TRAILER_BYTES)
            .read_to_end(&mut trailer)
            .await
            .ok()?;
        copy.wait().await.ok()?.success().then_some(())
    }
}

/// reads to the end or to `cap` bytes, whichever comes first.
async fn read_capped<R: AsyncRead + Unpin>(stream: R, cap: usize) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    stream
        .take(u64::try_from(cap).ok()?)
        .read_to_end(&mut bytes)
        .await
        .ok()?;
    Some(bytes)
}
