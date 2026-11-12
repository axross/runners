//! the exporter child, which is this executable run again, against the fake docker.

use std::ffi::OsString;
use std::fs;
use std::path::PathBuf;
use std::rc::Rc;

use marshal_cli::diagnostics::exporter::COMPLETE_TEXT;
use marshal_cli::runtime::backend::{
    Backend, Deadlines, DiagnosticRecord, ExportOutcome, Finalized,
};
use marshal_cli::runtime::diagnostic;
use marshal_cli::runtime::docker::{DockerBackend, DockerConfig, DockerLimits};
use marshal_cli::runtime::fake::{FakeClock, MemoryLog};
use tokio::time::Instant;

use crate::support::{APP, FAKE_DOCKER, Scratch};

const NAME: &str = "d-1-20240305060708009";
const LABEL: &str = "runners.diagnostic-lifecycle=1";

#[cfg(unix)]
fn private(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}

#[cfg(not(unix))]
fn private(_path: &std::path::Path) {}

#[tokio::test(flavor = "current_thread")]
async fn the_exporter_child_runs_from_the_apps_executable_and_writes_a_complete_bundle() {
    let scratch = Scratch::new();
    let engine = scratch.docker();
    fs::write(
        engine.join("containers"),
        format!("{NAME}\t{LABEL}\texited"),
    )
    .unwrap();
    let sink = scratch.path().join("sink");
    fs::create_dir(&sink).unwrap();
    private(&sink);
    let sink = fs::canonicalize(sink).unwrap();

    let clock = Rc::new(FakeClock::at(1_709_618_828_009));
    let log = Rc::new(MemoryLog::default());
    let config = DockerConfig {
        program: OsString::from(FAKE_DOCKER),
        environment: vec![(OsString::from("FAKE_DOCKER_DIR"), engine.into_os_string())],
        limits: DockerLimits::default(),
        // the exporter is the app's own executable, which dispatches `export` to the library
        exporter: Some(PathBuf::from(APP)),
        ..DockerConfig::default()
    };
    let backend = DockerBackend::new(config, clock, log.clone());
    let record = DiagnosticRecord {
        pool: "d".to_string(),
        directory: sink.to_str().unwrap().to_string(),
        raw_records: false,
    };
    backend.retained().adopt(NAME, &record);
    let claim = backend.retained().claim(NAME).unwrap();

    let finalized = diagnostic::finalize(&backend, &claim, Deadlines::from(Instant::now())).await;

    assert_eq!(
        finalized,
        Finalized {
            export: ExportOutcome::Complete,
            removed: true,
            kept: false,
            bundles: Some(1),
        },
        "{:?}",
        log.lines()
    );
    let bundles: Vec<PathBuf> = fs::read_dir(&sink)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(bundles.len(), 1);
    assert_eq!(
        fs::read_to_string(bundles[0].join("complete.txt")).unwrap(),
        COMPLETE_TEXT
    );
}
