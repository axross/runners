//! everything the golden VM is pinned to. A change to the base digest, the
//! provisioning steps, the launcher or the Docker Engine versions raises
//! [`GOLDEN_VM_VERSION`](crate::runtime::tart::GOLDEN_VM_VERSION), and a change to
//! the base digest also adds the old [`BASE_REFERENCE`] to [`RETIRED_BASE_PINS`].

/// the base image, by the digest of its manifest; the tag it was resolved from is
/// `24.04`, and it is arm64 Linux.
pub const BASE_REFERENCE: &str = "ghcr.io/cirruslabs/ubuntu@sha256:fb693c6ba8df94b4d89fa1d66b5eeb53a2bbcd66f8437d0b635099cd8906282d";

/// the base pins `build` removes from Tart's cache; each is a [`BASE_REFERENCE`]
/// that has been replaced, and the current one is never on it.
pub const RETIRED_BASE_PINS: &[&str] = &[];

/// the user the base image's guest agent runs commands as.
pub const GUEST_USER: &str = "admin";

/// where the launcher is installed, which `tart exec` runs with the arguments of a
/// `docker run` and the JIT configuration on standard input.
pub const LAUNCHER_PATH: &str = "/usr/local/bin/marshal-launch";

/// the id of the `docker` group, which owns the socket the runner container mounts:
/// the golden VM creates the group with it, and the runner image's `docker` group
/// has it, the one its `runner` user is in. a test holds the Dockerfile to it.
pub const DOCKER_GROUP_ID: u32 = 123;

/// where Docker publishes the key that signs its apt repository.
pub const DOCKER_KEY_URL: &str = "https://download.docker.com/linux/ubuntu/gpg";

/// where the key is kept, and where the apt source reads it from.
pub const DOCKER_KEY_PATH: &str = "/etc/apt/keyrings/docker.asc";

/// the fingerprint of the key's primary key; its signing subkey is
/// `D3306A018370199E527AE7997EA0A9C3F273FCD8`.
pub const DOCKER_KEY_FINGERPRINT: &str = "9DC858229FC7DD38854AE2D88D81803C0EBFCD88";

/// where the apt source is installed.
pub const DOCKER_SOURCES_PATH: &str = "/etc/apt/sources.list.d/docker.sources";

pub const DOCKER_REPOSITORY_URI: &str = "https://download.docker.com/linux/ubuntu";
pub const DOCKER_SUITE: &str = "noble";
pub const DOCKER_COMPONENT: &str = "stable";
pub const DOCKER_ARCHITECTURE: &str = "arm64";

/// an apt package at the one version the golden VM installs and holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PackagePin {
    pub name: &'static str,
    pub version: &'static str,
}

impl PackagePin {
    /// the `name=version` form apt installs by.
    #[must_use]
    pub fn apt_specification(&self) -> String {
        format!("{}={}", self.name, self.version)
    }
}

/// Docker Engine's packages for Ubuntu 24.04 on arm64; Compose is not among them.
pub const DOCKER_PACKAGES: [PackagePin; 4] = [
    PackagePin {
        name: "docker-ce",
        version: "5:29.9.3-1~ubuntu.24.04~noble",
    },
    PackagePin {
        name: "docker-ce-cli",
        version: "5:29.9.3-1~ubuntu.24.04~noble",
    },
    PackagePin {
        name: "containerd.io",
        version: "2.4.2-3~ubuntu.24.04~noble",
    },
    PackagePin {
        name: "docker-buildx-plugin",
        version: "0.38.1-1~ubuntu.24.04~noble",
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::tart::args::is_digest_reference;

    #[test]
    fn the_base_is_a_digest_reference_that_is_not_retired() {
        assert!(is_digest_reference(BASE_REFERENCE));
        assert!(!RETIRED_BASE_PINS.contains(&BASE_REFERENCE));
    }

    #[test]
    fn every_retired_pin_is_a_digest_reference() {
        for pin in RETIRED_BASE_PINS {
            assert!(is_digest_reference(pin), "{pin}");
        }
    }

    #[test]
    fn the_fingerprint_is_forty_uppercase_hex_digits() {
        assert_eq!(DOCKER_KEY_FINGERPRINT.len(), 40);
        assert!(
            DOCKER_KEY_FINGERPRINT
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'A'..=b'F').contains(&byte))
        );
    }

    #[test]
    fn the_packages_are_the_four_pinned_versions() {
        let specifications: Vec<String> = DOCKER_PACKAGES
            .iter()
            .map(PackagePin::apt_specification)
            .collect();
        assert_eq!(
            specifications,
            [
                "docker-ce=5:29.9.3-1~ubuntu.24.04~noble",
                "docker-ce-cli=5:29.9.3-1~ubuntu.24.04~noble",
                "containerd.io=2.4.2-3~ubuntu.24.04~noble",
                "docker-buildx-plugin=0.38.1-1~ubuntu.24.04~noble",
            ]
        );
    }
}
