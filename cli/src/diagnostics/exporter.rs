//! the exporter child: one export, or one count, of a pool's sink; it ends at its
//! deadline or when its standard input, the instance's liveness pipe, closes, killing
//! its `docker` clients first, which run with the JIT variable cleared; every file
//! system call runs on a blocking thread, so a hung sink cannot keep it from ending;
//! see `docs/operations/cli.md`.

use std::ffi::OsStr;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use tokio::sync::oneshot;
use tokio::time::Instant;

use crate::runtime::diagnostic::BUNDLE_QUOTA;

use super::archive::{EntrySink, MAX_METRICS_BYTES, METRICS_FILE};
use super::sink::{checked_sink, count_bundles, create_bundle, create_file};
use super::transport::{DockerCopy, EvidenceTransport};
use super::{
    ADMITTED, BUNDLES, EXIT_COMPLETE, EXIT_FAILED, EXIT_GAP, EXIT_SINK_FULL, ExportRequest,
};

/// what `complete.txt` holds, without a line break.
pub const COMPLETE_TEXT: &str = "export complete; evidence remains untrusted and private";

/// the line of `metrics.txt` the collector writes once it observed the runner's end.
const FINAL_OBSERVED: &str = "final=observed";

/// runs the exporter for `request` and returns its exit code.
#[must_use]
pub fn run(request: &ExportRequest, docker: &OsStr) -> ExitCode {
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        return ExitCode::from(EXIT_FAILED);
    };
    let code = runtime.block_on(supervise(request, &DockerCopy::new(docker)));
    // a blocking thread stuck in the sink is left behind; the process ends anyway
    runtime.shutdown_background();
    ExitCode::from(code)
}

/// waits on a thread of its own for standard input to end, the liveness pipe closing.
fn input_closed() -> oneshot::Receiver<()> {
    let (closed, receiver) = oneshot::channel();
    std::thread::spawn(move || {
        let _ = std::io::copy(&mut std::io::stdin(), &mut std::io::sink());
        let _ = closed.send(());
    });
    receiver
}

async fn supervise(request: &ExportRequest, transport: &impl EvidenceTransport) -> u8 {
    let deadline = Instant::now() + request.deadline;
    let closed = input_closed();
    // dropping the export kills its clients, which were started to die with it
    tokio::select! {
        biased;
        _ = closed => EXIT_FAILED,
        () = tokio::time::sleep_until(deadline) => EXIT_FAILED,
        code = export(request, transport) => code,
    }
}

async fn blocking<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    tokio::task::spawn_blocking(work).await.ok()
}

fn say(line: &str) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{line}");
    let _ = out.flush();
}

async fn export(request: &ExportRequest, transport: &impl EvidenceTransport) -> u8 {
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
    match fill(&bundle, container, request.raw_records, transport).await {
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
async fn fill(
    bundle: &Path,
    container: &str,
    raw_records: bool,
    transport: &impl EvidenceTransport,
) -> Option<bool> {
    let identity = transport.read_identity(container).await?;
    write_file(bundle, "identity.txt", identity.into_bytes()).await?;
    let sink = BundleSink {
        bundle: bundle.to_path_buf(),
        file: None,
    };
    transport.copy_archive(container, raw_records, sink).await?;
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
        // ignoring case, as the smoke export's -notmatch does
        line.strip_suffix(b"\r")
            .unwrap_or(line)
            .eq_ignore_ascii_case(FINAL_OBSERVED.as_bytes())
    }))
}

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

    /// a transport that reads no container: `identity` and `metrics` are what it
    /// yields, and `None` is a failed read or a failed archive.
    struct Scripted {
        identity: Option<&'static str>,
        metrics: Option<&'static [u8]>,
    }

    impl EvidenceTransport for Scripted {
        async fn read_identity(&self, source: &str) -> Option<String> {
            self.identity.map(|text| format!("{text} of {source}"))
        }

        async fn copy_archive<S: EntrySink>(
            &self,
            _source: &str,
            _raw_records: bool,
            mut sink: S,
        ) -> Option<()> {
            let metrics = self.metrics?;
            sink.open(METRICS_FILE).await.ok()?;
            sink.write(metrics.to_vec()).await.ok()?;
            Some(())
        }
    }

    /// a scratch directory that is removed when dropped, whether or not the test
    /// passed.
    struct Scratch(PathBuf);

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// what one export through a scripted transport did.
    struct Exported {
        code: u8,
        /// the sorted names of the files in the one bundle the export admitted.
        files: Vec<String>,
        bundle: PathBuf,
        _scratch: Scratch,
    }

    /// exports through `transport` into a sink of its own named `name`: private to
    /// the user and named without links, as the sink checks require.
    async fn export_with(name: &str, transport: &Scripted) -> Exported {
        let root = std::env::temp_dir().join(format!("exporter-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let scratch = Scratch(root);
        let sink = scratch.0.join("sink");
        std::fs::create_dir_all(&sink).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&sink, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let sink = std::fs::canonicalize(sink).unwrap();
        let request = ExportRequest {
            pool: "p".to_string(),
            container: Some("p-1-x".to_string()),
            directory: sink.to_string_lossy().into_owned(),
            raw_records: false,
            checkout: None,
            deadline: std::time::Duration::from_secs(10),
        };
        let code = export(&request, transport).await;
        let mut bundles: Vec<_> = std::fs::read_dir(&sink)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        assert_eq!(bundles.len(), 1, "an export admits exactly one bundle");
        let bundle = bundles.remove(0);
        let mut files: Vec<_> = std::fs::read_dir(&bundle)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        files.sort();
        Exported {
            code,
            files,
            bundle,
            _scratch: scratch,
        }
    }

    #[tokio::test]
    async fn an_export_writes_its_bundle_from_whatever_transport_it_is_given() {
        let transport = Scripted {
            identity: Some("identity"),
            metrics: Some(b"a=1\nfinal=observed\n"),
        };
        let exported = export_with("complete", &transport).await;
        assert_eq!(exported.code, EXIT_COMPLETE);
        assert_eq!(
            exported.files,
            ["complete.txt", "identity.txt", "metrics.txt"]
        );
        assert_eq!(
            std::fs::read_to_string(exported.bundle.join("identity.txt")).unwrap(),
            "identity of p-1-x"
        );
    }

    #[tokio::test]
    async fn metrics_without_the_final_observation_are_a_gap_with_a_complete_bundle() {
        let transport = Scripted {
            identity: Some("identity"),
            metrics: Some(b"a=1\n"),
        };
        let exported = export_with("gap", &transport).await;
        assert_eq!(exported.code, EXIT_GAP);
        assert_eq!(
            exported.files,
            ["complete.txt", "identity.txt", "metrics.txt"]
        );
    }

    #[tokio::test]
    async fn an_identity_that_cannot_be_read_fails_the_export_without_a_completion_marker() {
        let transport = Scripted {
            identity: None,
            metrics: Some(b"final=observed\n"),
        };
        let exported = export_with("no-identity", &transport).await;
        assert_eq!(exported.code, EXIT_FAILED);
        assert!(exported.files.is_empty(), "{:?}", exported.files);
    }

    #[tokio::test]
    async fn an_archive_that_fails_fails_the_export_without_a_completion_marker() {
        let transport = Scripted {
            identity: Some("identity"),
            metrics: None,
        };
        let exported = export_with("no-archive", &transport).await;
        assert_eq!(exported.code, EXIT_FAILED);
        assert_eq!(exported.files, ["identity.txt"]);
    }

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
    fn the_completion_marker_is_the_smoke_export_marker() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../images/actions-runner/tests/smoke-diagnostic-export.ps1");
        let source = std::fs::read_to_string(path).unwrap();
        assert!(source.contains(&format!("'complete.txt'), '{COMPLETE_TEXT}'")));
        assert!(source.contains("'(?m)^final=observed\\r?$'"));
    }
}
