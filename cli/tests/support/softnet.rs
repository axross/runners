//! a scratch directory of `PATH` entries and the fake `softnet`, which a test links
//! or copies in as a file called `softnet`.
#![allow(
    dead_code,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::ffi::OsString;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use super::TempDir;
use super::tart::{Invocation, ended_within, invocations};

pub const FAKE_SOFTNET: &str = env!("CARGO_BIN_EXE_fake-softnet");

pub struct Host {
    pub root: TempDir,
}

impl Host {
    pub fn new() -> Self {
        Self {
            root: TempDir::new(),
        }
    }

    /// creates the directory `name` and returns it.
    pub fn directory(&self, name: &str) -> PathBuf {
        let path = self.root.path().join(name);
        fs::create_dir_all(&path).unwrap();
        path
    }

    /// a `PATH` of these directories, in order.
    pub fn path(&self, names: &[&str]) -> OsString {
        std::env::join_paths(names.iter().map(|name| self.root.path().join(name))).unwrap()
    }

    /// [`Host::path`] followed by this process's own `PATH`, so the fake can run `ps`
    /// to record its process group. A test that expects Softnet to be missing uses
    /// [`Host::path`], so a `softnet` installed on the machine is never found.
    pub fn path_with_system(&self, names: &[&str]) -> OsString {
        let mut directories: Vec<PathBuf> = names
            .iter()
            .map(|name| self.root.path().join(name))
            .collect();
        directories.extend(std::env::split_paths(
            &std::env::var_os("PATH").unwrap_or_default(),
        ));
        std::env::join_paths(directories).unwrap()
    }

    /// links the fake `softnet` as `<directory>/softnet` and returns that file.
    #[cfg(unix)]
    pub fn link(&self, directory: &str) -> PathBuf {
        let file = self.directory(directory).join("softnet");
        std::os::unix::fs::symlink(FAKE_SOFTNET, &file).unwrap();
        file
    }

    /// copies the fake `softnet` to `<directory>/softnet` and returns that file.
    pub fn copy(&self, directory: &str) -> PathBuf {
        let file = self.directory(directory).join("softnet");
        fs::copy(FAKE_SOFTNET, &file).unwrap();
        file
    }

    /// sets a script file the fake reads from `directory`.
    pub fn script(&self, directory: &str, file: &str, content: &str) {
        fs::write(self.directory(directory).join(file), content).unwrap();
    }

    /// every call the fake in `directory` received, in order.
    pub fn invocations(&self, directory: &str) -> Vec<Invocation> {
        invocations(&self.directory(directory))
    }

    pub fn ended_within(&self, directory: &str, number: usize, within: Duration) -> bool {
        ended_within(&self.directory(directory), number, within)
    }
}
