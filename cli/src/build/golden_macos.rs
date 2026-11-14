//! `build --profile axmc` for a configuration with a macOS pool: makes the golden
//! macOS VM from the pinned base in a macOS build VM and commits it by renaming the
//! build VM to the golden name; any failure or stop before the rename deletes the
//! build VM and leaves the previous golden macOS VM as it was; see
//! `docs/operations/cli.md`.

use std::time::{Duration, SystemTime};

use tokio::sync::watch;
use tokio::time::Instant;

use crate::golden::macos::{
    disable_remote_access_call, disable_updates_call, flush_disk_call, install_launcher_call,
    install_runner_call, remove_runner_call,
};
use crate::golden::steps::GuestCall;
use crate::image::macos_base::{BASE_REPOSITORY, MacosBase, host_refusal};
use crate::image::runner::DeclaredRunner;
use crate::macos::count::{ConfiguredPool, count_macos_vms, listed_vms};
use crate::macos::name::{GOLDEN_MACOS_VM_VERSION, GoldenMacosName, build_name, is_build_name};
use crate::macos::prerequisites::{
    Check, VERSION_BUDGET, check_softnet, check_softnet_with, check_tart,
};
use crate::macos::softnet::OwnerMode;
use crate::runtime::log::Log;
use crate::runtime::tart::list::VmSource;
use crate::runtime::tart::teardown::{Stop, tear_down};
use crate::runtime::tart::{TartClient, VmLauncher};
use crate::validate::MAX_MACOS_VMS;

use super::build_vm::{Abort, BuildVm, Session, text_of};
use super::golden::GoldenTimings;

const PROVISIONING_STEPS: usize = 5;

/// how long a macOS build VM's guest agent has to answer after `tart run`, longer
/// than a Linux VM's because macOS boots more slowly.
pub const MACOS_AGENT_DEADLINE: Duration = Duration::from_secs(180);

/// the bound on pulling the macOS base, a download of about 26 GB.
pub const MACOS_PULL_BUDGET: Duration = Duration::from_secs(2 * 60 * 60);

pub struct GoldenMacosBuild<'a> {
    pub client: TartClient,
    pub launcher: VmLauncher,
    pub base: MacosBase,
    pub runner: DeclaredRunner,
    /// the configured pools, which tell the count which job VMs are macOS ones.
    pub pools: Vec<ConfiguredPool>,
    /// this Mac's macOS version, as `sw_vers -productVersion` prints it.
    pub host_macos: String,
    pub log: &'a dyn Log,
    pub timings: GoldenTimings,
    /// the owner and mode Softnet's file is judged to have, as a test gives it.
    pub softnet_owner_mode: Option<OwnerMode>,
    pub stop: watch::Receiver<Option<Instant>>,
}

impl GoldenMacosBuild<'_> {
    fn vm(&self) -> BuildVm<'_> {
        BuildVm {
            client: &self.client,
            launcher: &self.launcher,
            log: self.log,
            timings: self.timings,
            stop: &self.stop,
        }
    }

    /// runs the build until the golden macOS VM is committed; the error says why not.
    pub async fn run(&self) -> Result<(), String> {
        let mut session = Session::default();
        let Err(abort) = self.make(&mut session).await else {
            return Ok(());
        };
        self.vm().remove_build_vm(&session).await;
        Err(abort.message("the golden macOS VM"))
    }

    async fn make(&self, session: &mut Session) -> Result<(), Abort> {
        self.vm().check(self.check_prerequisites()).await?;
        self.vm().guard(self.delete_leftovers()).await?;
        // counted after the clean-up, so a leftover build VM this build deleted
        // never refuses it
        self.vm().check(self.check_limit()).await?;
        let base = self.base.reference();
        self.log.info(&format!("Pulling {base}."));
        self.vm()
            .guard(async { self.client.pull(&base).await.map_err(|e| text_of(&e)) })
            .await?;
        let vm = build_name(SystemTime::now());
        session.vm = Some(vm.clone());
        self.log
            .info(&format!("Cloning the macOS base to the build VM {vm}."));
        self.vm()
            .guard(async {
                self.client
                    .clone_vm(&base, &vm)
                    .await
                    .map_err(|e| text_of(&e))
            })
            .await?;
        self.vm().boot(session, &vm).await?;
        self.provision(&vm).await?;
        let built = SystemTime::now();
        self.vm()
            .shut_down(session, &vm, &flush_disk_call())
            .await?;
        let golden = GoldenMacosName::at(GOLDEN_MACOS_VM_VERSION, &self.runner.version, built)
            .ok_or_else(|| {
                Abort::Failed(format!(
                    "the declared runner version {} is not three numbers",
                    self.runner.version
                ))
            })?
            .to_string();
        self.vm().commit(session, &vm, &golden).await?;
        self.log.info(&format!(
            "The golden macOS VM {golden} is built, with runner {} inside.",
            self.runner.version
        ));
        self.retire_old(&golden).await;
        Ok(())
    }

    /// Tart and Softnet, and the host's macOS against the base's.
    async fn check_prerequisites(&self) -> Result<(), String> {
        let tart = self.accepted(check_tart(&self.client).await)?;
        let path = self.client.config().path.as_deref();
        let softnet = self.accepted(match self.softnet_owner_mode {
            Some(owner_mode) => check_softnet_with(path, Some(owner_mode), VERSION_BUDGET).await,
            None => check_softnet(path).await,
        })?;
        if let Some(refusal) = host_refusal(&self.base, &self.host_macos) {
            return Err(refusal);
        }
        self.log
            .info(&format!("Using Tart {tart} and Softnet {softnet}."));
        Ok(())
    }

    /// refuses the build while two macOS VMs count against the limit.
    async fn check_limit(&self) -> Result<(), String> {
        let entries = self
            .client
            .list()
            .await
            .map_err(|error| format!("the VMs could not be listed ({})", text_of(&error)))?;
        let count = count_macos_vms(&listed_vms(&entries), &self.pools, &self.client).await;
        if !count.has_free_place() {
            let names: Vec<&str> = count.counted().iter().map(|vm| vm.name.as_str()).collect();
            return Err(format!(
                "{} macOS VMs already count against the limit of {MAX_MACOS_VMS} ({}), so no \
                 macOS build VM was created; stop or delete one of them and build again",
                count.total(),
                names.join(", ")
            ));
        }
        Ok(())
    }

    fn accepted(&self, check: Check) -> Result<String, String> {
        match check {
            Check::Pass { version } => Ok(version),
            Check::Warn { version, message } => {
                self.log.warn(&message);
                Ok(version)
            }
            Check::Refuse(refusal) => Err(refusal.message),
        }
    }

    /// deletes the macOS build VMs an earlier build left; one that cannot be deleted
    /// is a warning.
    async fn delete_leftovers(&self) -> Result<(), String> {
        let entries = self
            .client
            .list()
            .await
            .map_err(|error| format!("the VMs could not be listed ({})", text_of(&error)))?;
        for leftover in entries
            .iter()
            .filter(|entry| entry.source == VmSource::Local && is_build_name(&entry.name))
        {
            let name = &leftover.name;
            self.log.info(&format!(
                "Deleting the macOS build VM {name}, left by an earlier build."
            ));
            if let Err(error) =
                tear_down(&self.client, name, Stop::Forced, self.timings.leftover).await
            {
                self.log
                    .warn(&format!("{error}; the next build tries again."));
            }
        }
        Ok(())
    }

    async fn step(
        &self,
        vm: &str,
        number: usize,
        what: &str,
        call: &GuestCall,
    ) -> Result<(), Abort> {
        self.log.info(&format!(
            "Provisioning the golden macOS VM, step {number} of {PROVISIONING_STEPS}: {what}."
        ));
        self.vm()
            .guard(self.vm().guest(vm, call, what))
            .await
            .map(drop)
    }

    async fn provision(&self, vm: &str) -> Result<(), Abort> {
        self.step(vm, 1, "removing the base's runner", &remove_runner_call())
            .await?;
        self.step(
            vm,
            2,
            &format!("installing runner {}", self.runner.version),
            &install_runner_call(&self.runner),
        )
        .await?;
        self.step(vm, 3, "installing the launcher", &install_launcher_call())
            .await?;
        self.step(
            vm,
            4,
            "turning off Remote Login and Screen Sharing",
            &disable_remote_access_call(),
        )
        .await?;
        self.step(
            vm,
            5,
            "turning off the automatic download and installation of updates",
            &disable_updates_call(),
        )
        .await
    }

    /// after the commit: deletes every other golden macOS VM and the retired macOS
    /// bases from Tart's cache, best effort; a stop skips what is left.
    async fn retire_old(&self, golden: &str) {
        match self.vm().guard(self.delete_old(golden)).await {
            Ok(()) => {}
            Err(Abort::Failed(reason)) => self
                .log
                .warn(&format!("{reason}; the next build tries again.")),
            Err(_) => self.log.info(
                "The stop skipped removing the old golden macOS VMs and retired macOS bases; \
                 the next build removes them.",
            ),
        }
    }

    async fn delete_old(&self, golden: &str) -> Result<(), String> {
        let entries = self.client.list().await.map_err(|error| {
            format!(
                "the old golden macOS VMs could not be listed ({})",
                text_of(&error)
            )
        })?;
        let retired: Vec<String> = self
            .base
            .retired
            .iter()
            .map(|digest| format!("{BASE_REPOSITORY}@{digest}"))
            .collect();
        let current = self.base.reference();
        let doomed = entries.iter().filter_map(|entry| match entry.source {
            VmSource::Local
                if entry.name != golden && GoldenMacosName::parse(&entry.name).is_some() =>
            {
                Some((entry.name.as_str(), "the old golden macOS VM"))
            }
            VmSource::Oci if entry.name != current && retired.contains(&entry.name) => {
                Some((entry.name.as_str(), "the retired macOS base"))
            }
            _ => None,
        });
        for (name, what) in doomed {
            self.log.info(&format!("Deleting {what} {name}."));
            if let Err(error) = self.client.delete(name, self.timings.leftover).await {
                self.log.warn(&format!(
                    "{what} {name} could not be deleted ({}); the next build tries again.",
                    text_of(&error)
                ));
            }
        }
        Ok(())
    }
}
