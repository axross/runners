//! how the exporter reaches a diagnostic container's evidence: `docker cp` on the
//! host's engine, or the same two commands in a job VM through the guest agent;
//! `docs/operations/cli.md` describes the seam.

use std::ffi::OsStr;
use std::process::Stdio;

use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Child;

use crate::runtime::docker::client_command;
use crate::runtime::docker_args::{copy_arguments, identity_arguments};
use crate::runtime::tart::{TartClient, TartError};

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
        identity_of(self.client(&identity_arguments(source))).await
    }

    async fn copy_archive<S: EntrySink>(
        &self,
        source: &str,
        raw_records: bool,
        sink: S,
    ) -> Option<()> {
        archive_of(self.client(&copy_arguments(source)), raw_records, sink).await
    }
}

/// the transport that runs the same `docker inspect` and `docker cp` inside a job
/// VM, as `tart exec <vm> docker ...`. Both are streamed, so the bound on a captured
/// guest call does not apply to the archive, which the strict reader and the bundle
/// limits bound instead.
pub struct VmCopy<'a> {
    client: &'a TartClient,
    vm: &'a str,
}

impl<'a> VmCopy<'a> {
    #[must_use]
    pub fn new(client: &'a TartClient, vm: &'a str) -> Self {
        Self { client, vm }
    }

    fn guest(&self, arguments: &[String]) -> Result<Child, TartError> {
        let mut command = vec!["docker".to_string()];
        command.extend(arguments.iter().cloned());
        self.client
            .exec_command(self.vm, &command)?
            .spawn()
            .map_err(TartError::Unavailable)
    }
}

impl EvidenceTransport for VmCopy<'_> {
    async fn read_identity(&self, source: &str) -> Option<String> {
        identity_of(self.guest(&identity_arguments(source)).ok()).await
    }

    async fn copy_archive<S: EntrySink>(
        &self,
        source: &str,
        raw_records: bool,
        sink: S,
    ) -> Option<()> {
        archive_of(self.guest(&copy_arguments(source)).ok(), raw_records, sink).await
    }
}

/// reads the identity text a started `docker inspect` prints, within its cap, and
/// succeeds when the client does.
async fn identity_of(client: Option<Child>) -> Option<String> {
    let mut inspect = client?;
    let stdout = inspect.stdout.take()?;
    let output = read_capped(stdout, MAX_IDENTITY_BYTES).await?;
    let text = identity_text(&output).ok()?;
    inspect.wait().await.ok()?.success().then_some(text)
}

/// streams the archive a started `docker cp` prints into `sink` through the strict
/// reader, then reads the client's trailer within its cap, and succeeds when the
/// client does.
async fn archive_of<S: EntrySink>(
    client: Option<Child>,
    raw_records: bool,
    mut sink: S,
) -> Option<()> {
    let mut copy = client?;
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
