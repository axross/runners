//! the launcher run as a real shell script, with a stand-in `docker` first on
//! `PATH`.
#![cfg(unix)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod support;

use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

use marshal_cli::golden::steps::LAUNCHER;
use marshal_cli::runtime::docker_args::JIT_VARIABLE;
use support::{TempDir, settle_executable};

const SECRET: &str = "eyJmYWtlIjoianVzdC1hLXRlc3QifQ==";

struct Launcher {
    dir: TempDir,
    script: PathBuf,
}

impl Launcher {
    /// a launcher whose `docker` records the variable and its arguments, and exits
    /// with `docker_exit`.
    fn new(docker_exit: i32) -> Self {
        let dir = TempDir::new();
        let script = dir.write("marshal-launch", LAUNCHER.as_bytes());
        let docker = dir.write(
            "bin/docker",
            format!(
                "#!/bin/sh\nprintf '%s' \"${JIT_VARIABLE}\" > \"$RECORD/variable\"\n\
                 for argument in \"$@\"; do printf '%s\\n' \"$argument\"; done > \"$RECORD/arguments\"\n\
                 exit {docker_exit}\n"
            )
            .as_bytes(),
        );
        fs::set_permissions(&docker, fs::Permissions::from_mode(0o755)).unwrap();
        let missing = dir.path().join("no-record");
        settle_executable(&docker, |command| {
            command.env("RECORD", &missing);
        });
        Self { dir, script }
    }

    fn run(&self, input: &[u8], arguments: &[&str]) -> Output {
        let path = format!(
            "{}:{}",
            self.dir.path().join("bin").display(),
            std::env::var("PATH").unwrap()
        );
        let mut child = Command::new("sh")
            .arg(&self.script)
            .args(arguments)
            .env("PATH", path)
            .env("RECORD", self.dir.path())
            .env_remove(JIT_VARIABLE)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut stdin = child.stdin.take().unwrap();
        let input = input.to_vec();
        let writer = std::thread::spawn(move || {
            let _ = stdin.write_all(&input);
        });
        let output = child.wait_with_output().unwrap();
        writer.join().unwrap();
        output
    }

    fn recorded(&self, file: &str) -> Option<String> {
        fs::read_to_string(self.dir.path().join(file)).ok()
    }
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn the_value_reaches_the_docker_client_environment_and_never_its_arguments() {
    let launcher = Launcher::new(0);

    let output = launcher.run(
        format!("{SECRET}\n").as_bytes(),
        &["run", "--rm", "-e", JIT_VARIABLE, "image"],
    );

    assert!(output.status.success());
    assert_eq!(launcher.recorded("variable").as_deref(), Some(SECRET));
    assert_eq!(
        launcher.recorded("arguments").as_deref(),
        Some(format!("run\n--rm\n-e\n{JIT_VARIABLE}\nimage\n").as_str())
    );
    assert!(!launcher.recorded("arguments").unwrap().contains(SECRET));
    assert!(!text(&output.stdout).contains(SECRET));
    assert!(!text(&output.stderr).contains(SECRET));
}

#[test]
fn the_value_is_written_to_no_file() {
    let launcher = Launcher::new(0);

    launcher.run(SECRET.as_bytes(), &["run"]);

    for entry in fs::read_dir(launcher.dir.path()).unwrap() {
        let path = entry.unwrap().path();
        if path.is_file() && path.file_name().unwrap() != "variable" {
            assert!(
                !fs::read_to_string(&path).unwrap().contains(SECRET),
                "{}",
                path.display()
            );
        }
    }
}

#[test]
fn the_exit_code_of_docker_is_the_launchers() {
    let launcher = Launcher::new(7);

    let output = launcher.run(SECRET.as_bytes(), &["run"]);

    assert_eq!(output.status.code(), Some(7));
}

#[test]
fn an_empty_input_is_refused_without_starting_docker() {
    for input in [&b""[..], b"\n", b"\n\n"] {
        let launcher = Launcher::new(0);

        let output = launcher.run(input, &["run"]);

        assert_eq!(output.status.code(), Some(64), "{input:?}");
        assert!(text(&output.stderr).contains("no jit configuration"));
        assert_eq!(launcher.recorded("variable"), None);
    }
}

#[test]
fn an_input_over_the_bound_is_refused_without_echoing_it() {
    let launcher = Launcher::new(0);
    let oversized = "x".repeat(65_537);

    let output = launcher.run(oversized.as_bytes(), &["run"]);

    assert_eq!(output.status.code(), Some(64));
    assert!(text(&output.stderr).contains("longer than 65536 bytes"));
    assert!(!text(&output.stderr).contains("xxxx"));
    assert_eq!(launcher.recorded("variable"), None);
}

#[test]
fn an_input_at_the_bound_is_accepted() {
    let launcher = Launcher::new(0);
    let largest = "x".repeat(65_536);

    let output = launcher.run(largest.as_bytes(), &["run"]);

    assert!(output.status.success());
    assert_eq!(launcher.recorded("variable"), Some(largest));
}
