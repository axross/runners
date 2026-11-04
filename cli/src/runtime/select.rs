//! which runtime runs a pool: the pool's operating system, on the host that runs it,
//! decides; `docs/operations/cli.md` has the table.

use crate::profile::{Os, Profile};

/// where a pool's jobs run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Runtime {
    /// a container in Docker Desktop, run by [`super::docker::DockerBackend`].
    DockerDesktop,
    /// a virtual machine per job, cloned from the golden VM; no code runs it yet.
    TartVm,
}

impl Runtime {
    /// the runtime of a pool of `os` on the `profile` host, or `None` when the host
    /// has none for it. An axpc pool has no `os`.
    #[must_use]
    pub fn of_pool(profile: Profile, os: Option<Os>) -> Option<Self> {
        match (profile, os) {
            (Profile::Axpc, _) => Some(Self::DockerDesktop),
            (Profile::Axmc, Some(Os::Linux)) => Some(Self::TartVm),
            (Profile::Axmc, Some(Os::Macos) | None) => None,
        }
    }

    /// whether this version of the CLI can run pools with this runtime.
    #[must_use]
    pub fn is_implemented(self) -> bool {
        matches!(self, Self::DockerDesktop)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_axpc_pool_selects_docker_an_axmc_linux_pool_a_vm_and_a_macos_pool_none() {
        assert_eq!(
            Runtime::of_pool(Profile::Axpc, None),
            Some(Runtime::DockerDesktop)
        );
        assert_eq!(
            Runtime::of_pool(Profile::Axmc, Some(Os::Linux)),
            Some(Runtime::TartVm)
        );
        assert_eq!(Runtime::of_pool(Profile::Axmc, Some(Os::Macos)), None);
        assert_eq!(Runtime::of_pool(Profile::Axmc, None), None);
    }

    #[test]
    fn only_the_docker_runtime_is_implemented() {
        assert!(Runtime::DockerDesktop.is_implemented());
        assert!(!Runtime::TartVm.is_implemented());
    }
}
