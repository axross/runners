//! the golden VM's install-docker step run as a real shell script, with stand-ins
//! for the system tools it calls, to hold what it does with the `docker` group.
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
use std::process::{Command, Output, Stdio};

use marshal_cli::golden::key::check_key_listing;
use marshal_cli::golden::pins::DOCKER_GROUP_ID;
use marshal_cli::golden::steps::{GuestCall, docker_sources, install_docker_call};
use support::{TempDir, settle_executable, stderr};

const KEY_LISTING: &str = include_str!("../src/golden/fixtures/docker-key-listing.txt");

/// writes each stand-in tool in `bin/`; the ones that act on the guest record their
/// call in `$STATE/calls`, and `getent` and `groupadd` share the group in
/// `$STATE/group`.
const TOOLS: [(&str, &str); 6] = [
    (
        "getent",
        "[ \"$1 $2\" = 'group docker' ] && [ -s \"$STATE/group\" ] || exit 2\n\
         cat \"$STATE/group\"\n",
    ),
    (
        "groupadd",
        "echo \"groupadd $*\" >> \"$STATE/calls\"\n\
         while [ \"$#\" -gt 0 ]; do\n\
         \x20 [ \"$1\" = --gid ] && gid=$2\n\
         \x20 shift\n\
         done\n\
         printf 'docker:x:%s:\\n' \"$gid\" > \"$STATE/group\"\n",
    ),
    ("apt-get", "echo \"apt-get $*\" >> \"$STATE/calls\"\n"),
    ("apt-mark", "echo \"apt-mark $*\" >> \"$STATE/calls\"\n"),
    ("usermod", "echo \"usermod $*\" >> \"$STATE/calls\"\n"),
    (
        "dpkg-query",
        "for argument in \"$@\"; do package=$argument; done\n\
         for spec in $PINS; do\n\
         \x20 [ \"${spec%%=*}\" = \"$package\" ] && printf '%s' \"${spec#*=}\"\n\
         done\n\
         exit 0\n",
    ),
];

struct Guest {
    dir: TempDir,
    call: GuestCall,
}

impl Guest {
    /// a guest whose `getent` reports `existing_group`, if any.
    fn new(existing_group: Option<&str>) -> Self {
        let dir = TempDir::new();
        let missing = dir.path().join("no-state");
        for (name, body) in TOOLS {
            let tool = dir.write(
                &format!("bin/{name}"),
                format!("#!/bin/sh\n{body}").as_bytes(),
            );
            fs::set_permissions(&tool, fs::Permissions::from_mode(0o755)).unwrap();
            settle_executable(&tool, |command| {
                command.env("STATE", &missing).env("PINS", "");
            });
        }
        fs::create_dir_all(dir.path().join("state")).unwrap();
        if let Some(entry) = existing_group {
            dir.write("state/group", format!("{entry}\n").as_bytes());
        }
        let call = install_docker_call(&check_key_listing(KEY_LISTING).unwrap());
        Self { dir, call }
    }

    /// runs the step's own script and arguments, with the apt source written under
    /// the temporary directory.
    fn run(&self) -> Output {
        let call = &self.call;
        assert_eq!(&call.command[..4], ["sudo", "-n", "bash", "-c"]);
        let sources = self.dir.path().join("docker.sources");
        let path = format!(
            "{}:{}",
            self.dir.path().join("bin").display(),
            std::env::var("PATH").unwrap()
        );
        let mut child = Command::new("bash")
            .arg("-c")
            .arg(&call.command[4])
            .arg(&call.command[5])
            .arg(&sources)
            .args(&call.command[7..])
            .env("PATH", path)
            .env("STATE", self.dir.path().join("state"))
            .env("PINS", call.command[9..].join("\n"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(docker_sources().as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    }

    fn calls(&self) -> Vec<String> {
        fs::read_to_string(self.dir.path().join("state/calls"))
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }
}

fn ran(calls: &[String], program: &str) -> bool {
    calls.iter().any(|call| call.starts_with(program))
}

#[test]
fn a_missing_group_is_created_with_the_id_and_the_install_goes_on() {
    let guest = Guest::new(None);

    let output = guest.run();

    assert!(output.status.success(), "{}", stderr(&output));
    let calls = guest.calls();
    assert_eq!(
        calls[0],
        format!("groupadd --system --gid {DOCKER_GROUP_ID} docker")
    );
    assert!(ran(&calls, "apt-get"));
    assert!(ran(&calls, "usermod --append --groups docker admin"));
}

#[test]
fn a_group_with_the_id_is_accepted_with_members_and_without() {
    for entry in [
        format!("docker:x:{DOCKER_GROUP_ID}:admin,runner"),
        format!("docker:x:{DOCKER_GROUP_ID}:"),
    ] {
        let guest = Guest::new(Some(&entry));

        let output = guest.run();

        assert!(output.status.success(), "{entry}: {}", stderr(&output));
        let calls = guest.calls();
        assert!(!ran(&calls, "groupadd"), "{entry}");
        assert!(ran(&calls, "apt-get"), "{entry}");
        assert!(ran(&calls, "apt-mark hold docker-ce"), "{entry}");
    }
}

#[test]
fn a_group_with_another_id_is_refused_before_apt_runs() {
    for id in ["1234", "12", "1230", "0"] {
        let entry = format!("docker:x:{id}:admin");
        let guest = Guest::new(Some(&entry));

        let output = guest.run();

        assert_eq!(output.status.code(), Some(1), "{entry}");
        assert!(
            stderr(&output).contains(&format!(
                "the docker group is not {DOCKER_GROUP_ID}: {entry}"
            )),
            "{}",
            stderr(&output)
        );
        assert!(guest.calls().is_empty(), "{entry}: {:?}", guest.calls());
    }
}
