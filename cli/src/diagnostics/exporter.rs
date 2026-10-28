//! the exporter child: one export, or one count, of a pool's sink; it ends at its
//! deadline or when its standard input, the instance's liveness pipe, closes, killing
//! its `docker` clients first, which run in a process group of their own with the JIT
//! variable cleared, and every file system call runs on a blocking thread so a hung
//! sink cannot keep it from ending.

use std::ffi::OsStr;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{ExitCode, Stdio};

use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Child;
use tokio::sync::oneshot;
use tokio::time::Instant;

use crate::runtime::diagnostic::BUNDLE_QUOTA;
use crate::runtime::docker::client_command;
use crate::runtime::docker_args::{copy_arguments, identity_arguments};

use super::archive::{ArchiveError, EntrySink, MAX_METRICS_BYTES, METRICS_FILE, read_archive};
use super::identity::{MAX_IDENTITY_BYTES, identity_text};
use super::sink::{checked_sink, count_bundles, create_bundle, create_file};
use super::{
    ADMITTED, BUNDLES, EXIT_COMPLETE, EXIT_FAILED, EXIT_GAP, EXIT_SINK_FULL, ExportRequest,
};

/// what `complete.txt` holds, without a line break.
pub const COMPLETE_TEXT: &str = "export complete; evidence remains untrusted and private";

/// the line of `metrics.txt` the collector writes once it observed the runner's end.
const FINAL_OBSERVED: &str = "final=observed";

/// the most bytes a copy client may print after the end of its archive.
const MAX_TRAILER_BYTES: u64 = 1024 * 1024;

/// runs the exporter for `request` with `docker` as the client, and returns its
/// exit code.
#[must_use]
pub fn run(request: &ExportRequest, docker: &OsStr) -> ExitCode {
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        return ExitCode::from(EXIT_FAILED);
    };
    let code = runtime.block_on(supervise(request, docker));
    // a blocking thread stuck in the sink is left behind; the process ends anyway
    runtime.shutdown_background();
    ExitCode::from(code)
}

/// waits on a thread of its own for standard input to end, which is when the
/// instance's end of the liveness pipe closes.
fn input_closed() -> oneshot::Receiver<()> {
    let (closed, receiver) = oneshot::channel();
    std::thread::spawn(move || {
        let _ = std::io::copy(&mut std::io::stdin(), &mut std::io::sink());
        let _ = closed.send(());
    });
    receiver
}

async fn supervise(request: &ExportRequest, docker: &OsStr) -> u8 {
    let deadline = Instant::now() + request.deadline;
    let closed = input_closed();
    // dropping the export kills its clients, which were started to die with it
    tokio::select! {
        biased;
        _ = closed => EXIT_FAILED,
        () = tokio::time::sleep_until(deadline) => EXIT_FAILED,
        code = export(request, docker) => code,
    }
}

async fn blocking<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    tokio::task::spawn_blocking(work).await.ok()
}

/// prints one line of the protocol the instance reads.
fn say(line: &str) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{line}");
    let _ = out.flush();
}

async fn export(request: &ExportRequest, docker: &OsStr) -> u8 {
    let (directory, checkout) = (request.directory.clone(), request.checkout.clone());
    let Some(Ok(sink)) = blocking(move || checked_sink(&directory, checkout.as_deref())).await
    else {
        return EXIT_FAILED;
    };
    let (counted, pool) = (sink.clone(), request.pool.clone());
    let Some(Ok(count)) = blocking(move || count_bundles(&counted, &pool)).await else {
        return EXIT_FAILED;
    };
    let Some(container) = &request.container else {
        say(&format!("{BUNDLES}{count}"));
        return EXIT_COMPLETE;
    };
    if count >= BUNDLE_QUOTA {
        say(&format!("{BUNDLES}{count}"));
        return EXIT_SINK_FULL;
    }
    let pool = request.pool.clone();
    let Some(Ok(bundle)) = blocking(move || create_bundle(&sink, &pool)).await else {
        return EXIT_FAILED;
    };
    say(ADMITTED);
    say(&format!("{BUNDLES}{}", count + 1));
    match fill(&bundle, container, request.raw_records, docker).await {
        Some(false) => EXIT_COMPLETE,
        Some(true) => EXIT_GAP,
        None => EXIT_FAILED,
    }
}

async fn write_file(bundle: &Path, name: &'static str, bytes: Vec<u8>) -> Option<()> {
    let bundle = bundle.to_path_buf();
    blocking(move || create_file(&bundle, name)?.write_all(&bytes))
        .await?
        .ok()
}

/// writes the bundle's files and returns whether its final observation is missing.
async fn fill(bundle: &Path, container: &str, raw_records: bool, docker: &OsStr) -> Option<bool> {
    let identity = read_identity(docker, container).await?;
    write_file(bundle, "identity.txt", identity.into_bytes()).await?;
    let mut copy = client(docker, &copy_arguments(container))?;
    let mut stdout = copy.stdout.take()?;
    let mut sink = BundleSink {
        bundle: bundle.to_path_buf(),
        file: None,
    };
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
    if !copy.wait().await.ok()?.success() {
        return None;
    }
    let metrics = bundle.join(METRICS_FILE);
    let observed = blocking(move || final_observed(&metrics)).await??;
    write_file(bundle, "complete.txt", COMPLETE_TEXT.as_bytes().to_vec()).await?;
    Some(!observed)
}

/// whether `metrics.txt` holds the line `final=observed`.
fn final_observed(metrics: &Path) -> Option<bool> {
    let mut bytes = Vec::new();
    std::fs::File::open(metrics)
        .ok()?
        .take(MAX_METRICS_BYTES)
        .read_to_end(&mut bytes)
        .ok()?;
    Some(bytes.split(|byte| *byte == b'\n').any(|line| {
        // ignoring case, as PowerShell's -notmatch does
        line.strip_suffix(b"\r")
            .unwrap_or(line)
            .eq_ignore_ascii_case(FINAL_OBSERVED.as_bytes())
    }))
}

/// starts a client whose standard output is read and whose standard error is
/// discarded.
fn client(docker: &OsStr, arguments: &[String]) -> Option<Child> {
    let mut command = client_command(docker, &[], arguments);
    command.stdout(Stdio::piped()).stderr(Stdio::null());
    command.spawn().ok()
}

/// reads the allowlisted identity within its cap.
async fn read_identity(docker: &OsStr, container: &str) -> Option<String> {
    let mut inspect = client(docker, &identity_arguments(container))?;
    let stdout = inspect.stdout.take()?;
    let output = read_capped(stdout, MAX_IDENTITY_BYTES).await?;
    let text = identity_text(&output).ok()?;
    inspect.wait().await.ok()?.success().then_some(text)
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

/// writes the archive's accepted files into the bundle, each through a blocking
/// thread.
struct BundleSink {
    bundle: PathBuf,
    file: Option<std::fs::File>,
}

fn unavailable() -> std::io::Error {
    std::io::Error::other("the blocking thread ended")
}

impl EntrySink for BundleSink {
    async fn open(&mut self, name: &str) -> std::io::Result<()> {
        let (bundle, name) = (self.bundle.clone(), name.to_string());
        self.file = Some(
            blocking(move || create_file(&bundle, &name))
                .await
                .ok_or_else(unavailable)??,
        );
        Ok(())
    }

    async fn write(&mut self, bytes: Vec<u8>) -> std::io::Result<()> {
        let mut file = self
            .file
            .take()
            .ok_or_else(|| std::io::Error::other("no file is open"))?;
        let file = blocking(move || file.write_all(&bytes).map(|()| file))
            .await
            .ok_or_else(unavailable)??;
        self.file = Some(file);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_final_observation_is_a_whole_line_with_an_optional_carriage_return() {
        let dir = std::env::temp_dir().join(format!("exporter-final-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let metrics = dir.join("metrics.txt");
        for (text, observed) in [
            ("a=1\nfinal=observed\n", true),
            ("a=1\r\nfinal=observed\r\n", true),
            ("final=observed", true),
            ("a=1\nFINAL=Observed\n", true),
            ("a=1\nfinal=observed later\n", false),
            ("xfinal=observed\n", false),
            ("", false),
        ] {
            std::fs::write(&metrics, text).unwrap();
            assert_eq!(final_observed(&metrics), Some(observed), "{text:?}");
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_completion_marker_is_the_powershell_text() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../hosts/windows-docker-desktop/diagnostic-export.ps1");
        let source = std::fs::read_to_string(path).unwrap();
        assert!(source.contains(&format!("'complete.txt'), '{COMPLETE_TEXT}'")));
        assert!(source.contains("'(?m)^final=observed\\r?$'"));
    }
}
