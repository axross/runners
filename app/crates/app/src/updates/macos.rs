//! the Mac's part of an update: the updater exchanges `Marshal.app` in place, so the
//! app first checks that its bundle can be exchanged without an administrator's
//! password, and afterwards starts a shell that opens the new bundle once this process
//! has exited. The agent's process is watched with `/bin/ps`, matched by its start
//! time so that a reused process ID is not taken for the agent.

use std::fs;
use std::os::unix::fs::MetadataExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use marshal_session::{Later, Process, Processes};

use super::EXIT_POLL;

/// the bundle this executable runs from, `<bundle>/Contents/MacOS/<executable>`.
fn bundle() -> Result<PathBuf, String> {
    let executable = std::env::current_exe()
        .map_err(|error| format!("the app's own file cannot be found: {error}"))?;
    executable
        .ancestors()
        .nth(3)
        .filter(|path| path.extension().is_some_and(|extension| extension == "app"))
        .map(Path::to_path_buf)
        .ok_or_else(|| format!("{} is not inside an app bundle", executable.display()))
}

/// `Ok` when this user can exchange the bundle: an entry can be made in its folder, and
/// the bundle belongs to the user who made it. Otherwise the updater would ask for an
/// administrator's password.
pub(super) fn replaceable() -> Result<(), String> {
    let bundle = bundle()?;
    let folder = bundle
        .parent()
        .ok_or_else(|| format!("{} has no folder", bundle.display()))?;
    let probe = folder.join(format!(".marshal-update-{}", std::process::id()));
    let made = fs::File::create_new(&probe).and_then(|file| file.metadata());
    let _ = fs::remove_file(&probe);
    let mine = made
        .map_err(|error| format!("{} cannot be changed: {error}", folder.display()))?
        .uid();
    let owner = fs::metadata(&bundle)
        .map_err(|error| format!("{} cannot be read: {error}", bundle.display()))?
        .uid();
    if owner != mine {
        return Err(format!(
            "{} belongs to another user, so it cannot be replaced without a password",
            bundle.display()
        ));
    }
    Ok(())
}

/// waits in `/bin/sh` for the process `$1` to exit, then opens the bundle `$2`.
const RELAUNCH: &str =
    "while /bin/kill -0 \"$1\" 2>/dev/null; do /bin/sleep 0.2; done; exec /usr/bin/open \"$2\"";

/// starts the shell that opens the new bundle, at the same path, once this process has
/// exited; the shell is in its own process group, so the app's end does not end it.
pub(super) fn relaunch() -> Result<(), String> {
    let bundle = bundle()?;
    Command::new("/bin/sh")
        .args(["-c", RELAUNCH, "sh"])
        .arg(std::process::id().to_string())
        .arg(&bundle)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .map(drop)
        .map_err(|error| format!("the new app could not be started: {error}"))
}

/// when the process `pid` started, as `/bin/ps` gives it, or `None` when no such
/// process runs.
fn started(pid: u32) -> Result<Option<String>, String> {
    let output = Command::new("/bin/ps")
        .args(["-o", "lstart=", "-p"])
        .arg(pid.to_string())
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|error| format!("/bin/ps could not be run: {error}"))?;
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    // `ps` exits with 1 and prints nothing when no process has the ID
    Ok((output.status.success() && !text.is_empty()).then_some(text))
}

/// watches processes by ID and start time.
pub struct Watched;

impl Processes for Watched {
    fn open(&self, pid: u32) -> Result<Option<Arc<dyn Process>>, String> {
        Ok(started(pid)?.map(|start| Arc::new(Watch { pid, start }) as Arc<dyn Process>))
    }
}

struct Watch {
    pid: u32,
    start: String,
}

impl Process for Watch {
    /// a process that no longer runs, or a new one under the same ID, has exited; one
    /// that `ps` cannot be asked about is taken as still running.
    fn has_exited(&self) -> bool {
        started(self.pid).is_ok_and(|start| start.as_deref() != Some(self.start.as_str()))
    }

    fn wait(self: Arc<Self>, limit: Duration) -> Later<bool> {
        Box::pin(async move {
            let until = tokio::time::Instant::now() + limit;
            loop {
                if self.has_exited() {
                    return true;
                }
                if tokio::time::Instant::now() >= until {
                    return false;
                }
                tokio::time::sleep(EXIT_POLL).await;
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use std::process::Command;
    use std::sync::Arc;
    use std::time::Duration;

    use marshal_session::Processes;

    use super::Watched;

    #[tokio::test]
    async fn a_watched_process_is_seen_to_exit_and_a_missing_one_is_none() {
        let mut child = Command::new("/bin/sleep").arg("30").spawn().unwrap();
        let process = Watched.open(child.id()).unwrap().expect("the child runs");
        assert!(!process.has_exited());
        assert!(!Arc::clone(&process).wait(Duration::from_millis(300)).await);
        child.kill().unwrap();
        child.wait().unwrap();
        assert!(process.wait(Duration::from_secs(5)).await);
        assert!(Watched.open(child.id()).unwrap().is_none());
    }
}
