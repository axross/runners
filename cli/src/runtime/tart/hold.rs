//! the process that holds a VM's `tart run` for as long as the CLI needs the VM, a job
//! VM now and a build VM in the build; on Unix it is the VM shim, which also deletes
//! the VM when the CLI is gone, however it ended.

use std::sync::Mutex;

use tokio::process::{Child, ChildStdin};
use tokio::sync::watch;
use tokio::task::AbortHandle;

use super::client::{Captured, READ_TAIL, Sink, TartClient, TartError, captured, drain};

/// how a `tart run` is started.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum VmLauncher {
    /// as a child of this process, killed when it is dropped; the VM then outlives a
    /// CLI that is killed hard, and the next `start` deletes it.
    #[default]
    Direct,
    /// by the VM shim, which, when its liveness pipe closes however this process
    /// ended, stops and deletes the VM.
    #[cfg(unix)]
    Shim(std::path::PathBuf),
}

/// a started `tart run` and the pipe that keeps its VM alive.
pub struct HeldVm {
    liveness: Mutex<Option<ChildStdin>>,
    ended: watch::Receiver<Option<Captured>>,
    tasks: Vec<AbortHandle>,
}

impl HeldVm {
    /// starts `tart run` for the local VM `vm` the way `launcher` says.
    pub fn start(client: &TartClient, launcher: &VmLauncher, vm: &str) -> Result<Self, TartError> {
        Self::start_for(client, launcher, vm, None)
    }

    /// starts `tart run` for the job VM `vm` of the diagnostics-enabled pool `pool`: a
    /// shim that deletes the VM because the CLI is gone reports the evidence that
    /// loses.
    pub fn start_diagnostic(
        client: &TartClient,
        launcher: &VmLauncher,
        vm: &str,
        pool: &str,
    ) -> Result<Self, TartError> {
        Self::start_for(client, launcher, vm, Some(pool))
    }

    fn start_for(
        client: &TartClient,
        launcher: &VmLauncher,
        vm: &str,
        diagnostic_pool: Option<&str>,
    ) -> Result<Self, TartError> {
        #[cfg(not(unix))]
        let _ = diagnostic_pool;
        let mut command = match launcher {
            VmLauncher::Direct => client.run_command(vm)?,
            #[cfg(unix)]
            VmLauncher::Shim(program) => shim_command(client, program, vm, diagnostic_pool),
        };
        let child = command.spawn().map_err(TartError::Unavailable)?;
        Ok(Self::hold(child))
    }

    fn hold(mut child: Child) -> Self {
        let liveness = child.stdin.take();
        let (stdout, stderr) = (Sink::default(), Sink::default());
        let mut tasks = Vec::new();
        let mut readers = Vec::new();
        if let Some(stream) = child.stdout.take() {
            readers.push(tokio::spawn(drain(stream, stdout.clone())));
        }
        if let Some(stream) = child.stderr.take() {
            readers.push(tokio::spawn(drain(stream, stderr.clone())));
        }
        tasks.extend(readers.iter().map(tokio::task::JoinHandle::abort_handle));
        let (ended_sender, ended) = watch::channel(None);
        let waiter = tokio::spawn(async move {
            let status = child.wait().await;
            for reader in readers {
                let _ = tokio::time::timeout(READ_TAIL, reader).await;
            }
            let code = status.ok().and_then(|status| status.code());
            ended_sender.send_replace(Some(captured(code, &stdout, &stderr)));
        });
        tasks.push(waiter.abort_handle());
        Self {
            liveness: Mutex::new(liveness),
            ended,
            tasks,
        }
    }

    /// waits until the process has ended and returns its exit code and what it
    /// printed, each stream cut at the client's bound. For the VM shim that is Tart's
    /// own exit code and output, which the shim relays.
    pub async fn ended(&self) -> Captured {
        let mut ended = self.ended.clone();
        loop {
            if let Some(end) = ended.borrow_and_update().clone() {
                return end;
            }
            if ended.changed().await.is_err() {
                return Captured {
                    code: None,
                    stdout: String::new(),
                    stderr: String::new(),
                    stdout_cut: false,
                    stderr_cut: false,
                };
            }
        }
    }

    /// whether the process has ended.
    #[must_use]
    pub fn has_ended(&self) -> bool {
        self.ended.borrow().is_some()
    }

    /// closes the liveness pipe, which makes the shim stop and delete the VM if it is
    /// still there; the VM then has no holder, so the caller does this once it has
    /// deleted the VM itself, or when it can do no more.
    pub fn close_liveness(&self) {
        let pipe = self
            .liveness
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        drop(pipe);
    }
}

impl Drop for HeldVm {
    fn drop(&mut self) {
        self.close_liveness();
        for task in &self.tasks {
            task.abort();
        }
    }
}

/// the command that runs the VM shim for `vm`: this program again, as the hidden
/// `marshal vm-shim --name <vm> -- <tart>`, in a process group of its own, with the
/// liveness pipe as its standard input, Tart's output relayed on its standard
/// output, and its own warnings on this process's standard error. For a VM of a
/// diagnostics-enabled pool it also gets that pool's name.
#[cfg(unix)]
fn shim_command(
    client: &TartClient,
    shim: &std::path::Path,
    vm: &str,
    diagnostic_pool: Option<&str>,
) -> tokio::process::Command {
    use std::process::Stdio;

    use tokio::process::Command;

    use crate::runtime::docker_args::JIT_VARIABLE;

    let config = client.config();
    let mut command = Command::new(shim);
    command
        .arg(crate::vm_shim::VM_SHIM_COMMAND)
        .arg("--name")
        .arg(vm);
    if let Some(pool) = diagnostic_pool {
        command
            .arg(crate::vm_shim::DIAGNOSTIC_POOL_OPTION)
            .arg(pool);
    }
    command
        .arg("--")
        .arg(&config.program)
        .env_remove(JIT_VARIABLE)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(false)
        .process_group(0);
    for (name, value) in &config.environment {
        command.env(name, value);
    }
    match &config.path {
        Some(path) => command.env("PATH", path),
        None => command.env_remove("PATH"),
    };
    command
}
