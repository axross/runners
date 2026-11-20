//! which runtime runs a pool: the pool's operating system, on the host that runs it,
//! decides; `docs/operations/cli.md` has the table.

use crate::profile::{Os, Profile};

/// where a pool's jobs run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Runtime {
    /// a container in Docker Desktop, run by [`super::docker::DockerBackend`].
    DockerDesktop,
    /// a virtual machine per job, cloned from the golden VM, run by
    /// [`super::tart::TartBackend`].
    TartVm,
}

impl Runtime {
    /// the runtime of a pool of `os` on the `profile` host, or `None` when the host
    /// has none for it. An axpc pool's `os` does not choose its runtime.
    #[must_use]
    pub fn of_pool(profile: Profile, os: Option<Os>) -> Option<Self> {
        match (profile, os) {
            (Profile::Axpc, _) => Some(Self::DockerDesktop),
            (Profile::Axmc, Some(_)) => Some(Self::TartVm),
            (Profile::Axmc, None) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_axpc_pool_selects_docker_and_every_axmc_pool_a_vm() {
        assert_eq!(
            Runtime::of_pool(Profile::Axpc, None),
            Some(Runtime::DockerDesktop)
        );
        assert_eq!(
            Runtime::of_pool(Profile::Axpc, Some(Os::Linux)),
            Some(Runtime::DockerDesktop)
        );
        assert_eq!(
            Runtime::of_pool(Profile::Axmc, Some(Os::Linux)),
            Some(Runtime::TartVm)
        );
        assert_eq!(
            Runtime::of_pool(Profile::Axmc, Some(Os::Macos)),
            Some(Runtime::TartVm)
        );
    }
}
