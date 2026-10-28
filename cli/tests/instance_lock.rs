//! the single-instance lock, within a process and across processes; only debug
//! builds can move its root, so the file is debug-only.
#![cfg(debug_assertions)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod support;

use std::io::{BufRead, BufReader, Read};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{Mutex, MutexGuard, PoisonError, mpsc};
use std::time::Duration;

use marshal_cli::instance::lock::{InstanceLock, LockError};
use marshal_cli::instance::roots::Roots;
use support::TempDir;

const HELPER: &str = env!("CARGO_BIN_EXE_instance-helper");

const HELD: i32 = 3;

struct Holder {
    process: Child,
    stdin: Option<ChildStdin>,
    stdout: Option<BufReader<ChildStdout>>,
}

impl Holder {
    fn start(command: Command) -> Self {
        let mut command = command;
        let mut process = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let stdin = process.stdin.take();
        let mut stdout = BufReader::new(process.stdout.take().unwrap());
        let mut first = String::new();
        stdout.read_line(&mut first).unwrap();
        assert_eq!(
            first.trim_end(),
            "locked",
            "the helper did not take the lock"
        );
        Self {
            process,
            stdin,
            stdout: Some(stdout),
        }
    }

    fn hold(root: &TempDir, child: bool) -> Self {
        let mut command = Command::new(HELPER);
        command.arg("hold").arg("--root").arg(root.path());
        if child {
            command.arg("--child");
        }
        Self::start(command)
    }

    /// holds under a umask that removes every permission from group and others.
    #[cfg(unix)]
    fn hold_under_restrictive_umask(root: &TempDir) -> Self {
        let mut command = Command::new("/bin/sh");
        command
            .arg("-c")
            .arg("umask 077 && exec \"$0\" \"$@\"")
            .arg(HELPER)
            .arg("hold")
            .arg("--root")
            .arg(root.path());
        Self::start(command)
    }

    fn kill(&mut self) {
        self.process.kill().unwrap();
        self.process.wait().unwrap();
    }
}

impl Drop for Holder {
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
    }
}

/// runs the tests of this file one at a time, since a process one test spawns holds
/// a copy of every open descriptor of this process until it calls `exec`, so a lock
/// another test has just dropped would still read as held.
fn serial() -> MutexGuard<'static, ()> {
    static SERIAL: Mutex<()> = Mutex::new(());
    SERIAL.lock().unwrap_or_else(PoisonError::into_inner)
}

fn try_exit_code(root: &TempDir) -> i32 {
    Command::new(HELPER)
        .arg("try")
        .arg("--root")
        .arg(root.path())
        .stdout(Stdio::null())
        .status()
        .unwrap()
        .code()
        .unwrap()
}

#[test]
fn a_second_acquisition_is_refused_while_the_first_holder_runs() {
    let _serial = serial();
    let root = TempDir::new();
    let roots = Roots::for_tests(root.path());
    let _holder = Holder::hold(&root, false);
    assert_eq!(InstanceLock::acquire(&roots).unwrap_err(), LockError::Held);
    assert_eq!(try_exit_code(&root), HELD);
}

#[test]
fn a_second_acquisition_in_one_process_is_refused_until_the_first_is_dropped() {
    let _serial = serial();
    let root = TempDir::new();
    let roots = Roots::for_tests(root.path());
    let first = InstanceLock::acquire(&roots).unwrap();
    assert_eq!(InstanceLock::acquire(&roots).unwrap_err(), LockError::Held);
    drop(first);
    drop(InstanceLock::acquire(&roots).unwrap());
}

#[test]
fn a_lock_in_another_root_is_independent() {
    let _serial = serial();
    let (one, other) = (TempDir::new(), TempDir::new());
    let _first = InstanceLock::acquire(&Roots::for_tests(one.path())).unwrap();
    let _second = InstanceLock::acquire(&Roots::for_tests(other.path())).unwrap();
}

#[test]
fn a_hard_killed_holder_frees_the_lock_at_once_while_its_child_still_runs() {
    let _serial = serial();
    let root = TempDir::new();
    let roots = Roots::for_tests(root.path());
    let mut holder = Holder::hold(&root, true);
    assert_eq!(InstanceLock::acquire(&roots).unwrap_err(), LockError::Held);

    // the child shares the helper's standard output, so that stream stays open for
    // as long as the child lives, and only then.
    let (finished, ended) = mpsc::channel();
    let mut stdout = holder.stdout.take().unwrap();
    holder.kill();
    let lock = InstanceLock::acquire(&roots);
    std::thread::spawn(move || {
        let mut rest = String::new();
        let _ = stdout.read_to_string(&mut rest);
        let _ = finished.send(());
    });
    assert!(
        ended.recv_timeout(Duration::from_millis(500)).is_err(),
        "the child ended with the helper, so the test shows nothing"
    );
    lock.expect("the lock is free at once, though the child runs");

    drop(holder.stdin.take());
    ended
        .recv_timeout(Duration::from_secs(10))
        .expect("the child ends when its input closes");
}

#[cfg(unix)]
mod unix {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    const CANNOT_OPEN: i32 = 4;

    fn mode(path: &std::path::Path) -> u32 {
        fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn a_lock_file_created_under_a_restrictive_umask_is_still_0644() {
        let _serial = serial();
        let root = TempDir::new();
        let _holder = Holder::hold_under_restrictive_umask(&root);
        let file = Roots::for_tests(root.path()).lock_file().to_path_buf();
        assert_eq!(mode(&file), 0o644);
    }

    #[test]
    fn the_lock_file_is_0644_when_this_process_creates_it() {
        let _serial = serial();
        let root = TempDir::new();
        let roots = Roots::for_tests(root.path());
        let _lock = InstanceLock::acquire(&roots).unwrap();
        assert_eq!(mode(roots.lock_file()), 0o644);
    }

    #[test]
    fn a_lock_file_that_cannot_be_written_is_opened_read_only_and_still_locks() {
        let _serial = serial();
        let root = TempDir::new();
        let roots = Roots::for_tests(root.path());
        fs::write(roots.lock_file(), b"").unwrap();
        fs::set_permissions(roots.lock_file(), fs::Permissions::from_mode(0o444)).unwrap();
        // root ignores the mode, so there the open for writing succeeds and the
        // read-only open is left to the CI step that runs the helper as nobody
        if fs::OpenOptions::new()
            .write(true)
            .open(roots.lock_file())
            .is_ok()
        {
            eprintln!("note: this user can write a 0444 file, so the read-only open is not used");
        }
        let first = InstanceLock::acquire(&roots).expect("a read-only open takes the lock");
        assert_eq!(InstanceLock::acquire(&roots).unwrap_err(), LockError::Held);
        assert_eq!(try_exit_code(&root), HELD);
        drop(first);
        drop(InstanceLock::acquire(&roots).unwrap());
    }

    #[test]
    fn an_unusable_lock_location_is_reported_as_not_openable_and_not_as_held() {
        let _serial = serial();
        let root = TempDir::new();
        let roots = Roots::for_tests(&root.path().join("absent"));
        let error = InstanceLock::acquire(&roots).unwrap_err();
        assert!(matches!(error, LockError::CannotOpen(_)), "{error}");
        assert!(error.to_string().contains("cannot be opened"), "{error}");
        assert!(!error.to_string().contains("held"), "{error}");
        let held = LockError::Held.to_string();
        assert!(held.contains("held") && !held.contains("cannot be opened"));
        assert_eq!(
            Command::new(HELPER)
                .arg("try")
                .arg("--root")
                .arg(root.path().join("absent"))
                .stdout(Stdio::null())
                .status()
                .unwrap()
                .code(),
            Some(CANNOT_OPEN)
        );
    }

    /// acquires on a thread of its own, so an open that waits fails the test
    /// instead of hanging it.
    fn acquire_promptly(roots: &Roots) -> Result<InstanceLock, LockError> {
        let (sender, receiver) = mpsc::channel();
        let roots = roots.clone();
        std::thread::spawn(move || {
            let _ = sender.send(InstanceLock::acquire(&roots));
        });
        receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("the acquisition waited instead of returning")
    }

    fn make_fifo(path: &std::path::Path) {
        let status = Command::new("mkfifo").arg(path).status().unwrap();
        assert!(status.success(), "mkfifo failed");
    }

    #[test]
    fn a_fifo_at_the_lock_path_cannot_be_opened_and_does_not_hang() {
        let _serial = serial();
        let root = TempDir::new();
        let roots = Roots::for_tests(root.path());
        make_fifo(roots.lock_file());

        let error = acquire_promptly(&roots).unwrap_err();

        assert!(matches!(error, LockError::CannotOpen(_)), "{error}");
    }

    #[test]
    fn a_fifo_with_a_reader_is_not_a_regular_file_and_is_not_locked() {
        let _serial = serial();
        let root = TempDir::new();
        let roots = Roots::for_tests(root.path());
        make_fifo(roots.lock_file());
        // a reader on the FIFO lets an open for writing succeed at once
        let (sender, receiver) = mpsc::channel();
        let fifo = roots.lock_file().to_path_buf();
        std::thread::spawn(move || {
            let _ = sender.send(
                fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(fifo)
                    .unwrap(),
            );
        });
        let _party = receiver.recv_timeout(Duration::from_secs(5)).unwrap();

        let error = acquire_promptly(&roots).unwrap_err();

        assert!(
            matches!(&error, LockError::CannotOpen(detail) if detail.contains("not a regular file")),
            "{error}"
        );
    }

    #[test]
    fn a_link_at_the_lock_path_is_refused_without_opening_its_target() {
        let _serial = serial();
        let root = TempDir::new();
        let roots = Roots::for_tests(root.path());
        let target = root.path().join("target");
        fs::write(&target, b"precious").unwrap();
        let modified = std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000_000);
        fs::File::options()
            .write(true)
            .open(&target)
            .unwrap()
            .set_modified(modified)
            .unwrap();
        std::os::unix::fs::symlink(&target, roots.lock_file()).unwrap();

        let error = acquire_promptly(&roots).unwrap_err();

        // following the link would lock the target and then find the path is a
        // link, which reads as lost
        assert!(matches!(error, LockError::CannotOpen(_)), "{error}");
        assert_eq!(fs::read(&target).unwrap(), b"precious");
        assert_eq!(fs::metadata(&target).unwrap().modified().unwrap(), modified);
        assert!(
            fs::symlink_metadata(roots.lock_file())
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }

    #[test]
    fn a_lock_that_is_a_directory_cannot_be_opened() {
        let _serial = serial();
        let root = TempDir::new();
        let roots = Roots::for_tests(root.path());
        fs::create_dir(roots.lock_file()).unwrap();
        assert!(matches!(
            InstanceLock::acquire(&roots),
            Err(LockError::CannotOpen(_))
        ));
    }
}
