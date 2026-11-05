# macOS Runner Host

How to prepare the maintainer's Apple silicon Mac, the `axmc` host, to run ephemeral
Linux GitHub Actions runners with the CLI, `marshal`: each job will run in a fresh Tart
Ubuntu VM that is cloned from a golden VM, on Softnet, with no Docker Desktop. It
covers installing Tart and Softnet, setting up Softnet's privilege, the signed-in
session the host needs, the golden VM and how `build` makes it, the host
configuration, and the verification status. It is the operator's side of
[#74](https://github.com/axross/runners/issues/74). Its titles for installing Tart
and Softnet, setting up Softnet and the signed-in session are fixed, because
`marshal`'s messages name them; a title changes only with the code that names it.
[CLI](./cli.md) owns what each `marshal` command does, and the trust rules the host
is reviewed against are in [Security](../conventions/security.md). The Windows
equivalent is [Windows Runner Host](./windows-runner-host.md).

`marshal build --profile axmc` makes the golden VM. `marshal start --profile axmc`
does not run the pools yet: it validates the configuration and then refuses, as
[Running the Host](#running-the-host) says. No procedure below has been run on a Mac
yet; see [Verification Status](#verification-status).

## Installing Tart and Softnet

`marshal` installs, bundles and repairs neither program. It calls `tart` as an
external program, and Tart runs the `softnet` that comes first on the `PATH` it
gets, which is the `PATH` of the terminal where `marshal` runs. Both programs need
an Apple silicon Mac, and Softnet 0.25.0 needs macOS 26.

The tested versions are recorded in the CLI (`macos::version`):

| Program | Tested version | `--version` prints              |
| ------- | -------------- | ------------------------------- |
| Tart    | 2.41.1         | `2.41.1`                        |
| Softnet | 0.25.0         | `softnet 0.25.0-<short commit>` |

Install them with Homebrew, which installs Softnet as a dependency of Tart:

```bash
brew install openai/tools/tart
tart --version
softnet --version
```

`marshal` reads both versions with `--version`, each within 5 s, at `build`, before
it creates any VM:

- The tested version runs.
- A newer patch of the tested minor version runs, with a warning that names the
  tested version.
- Anything else is refused with a message that names the tested version and this
  section: an older patch, another minor or major version, output with a fourth
  number or any suffix (Softnet's `-<short commit>` aside), and output with no
  version. A `tart` or `softnet` that is not on `PATH` is refused the same way.

Homebrew installs the current release of each program. This document does not yet
record a way to hold a version back, so when Homebrew's current version is another
minor version than the tested one, `marshal` refuses it until the tested versions
are updated in the CLI.

## Setting Up Softnet

Softnet filters a VM's network, and it needs root privilege to do so. Its privilege
is a SUID bit on its binary, with no `sudoers` rule, and `marshal` never sets it: it
only checks it, before it boots a VM. Homebrew never sets the bit, and a
`brew upgrade` installs a new binary without it, so repeat this setup after each
upgrade of Softnet or Tart.

`marshal` finds `softnet` as Tart does, as the first match on `PATH`, and follows
its links. Homebrew's file in `/opt/homebrew/bin` is a link to a versioned file, and
the setup applies to that file, not to the link. The file must be a regular file,
owned by root, with the SUID bit set and no write permission for its group or others:

```bash
softnet_file="$(realpath "$(command -v softnet)")"
sudo chown root "$softnet_file"
sudo chmod u+s,go-w "$softnet_file"
ls -l "$softnet_file"
```

The listing starts with `-rwsr-xr-x` and names `root` as the owner. `marshal`
refuses with a message that names the failing requirement when the file is a
directory or another kind of file, is owned by another user, lacks the SUID bit, or
can be written by its group or others. It also refuses a `softnet` that does not
answer `--version` within 5 s.

**What the network allows.** Every VM boots with the two rules in
[The Golden VM Contract](#the-golden-vm-contract), on top of Softnet's defaults. A
VM can then send to globally routable IPv4 addresses and to DNS on the Mac's address
on Softnet's bridge, the gateway, and to nothing else: not the local network, not
another VM, not a multicast group, not an IPv6 address, and no other port on the Mac.
Four consequences matter to an operator:

- A VM cannot reach a service on a private address, such as an internal package
  mirror, and cannot use an IPv6-only network.
- DNS goes through the Mac's resolver, which can answer the names of the Mac's
  networks and VPNs, though their addresses stay blocked.
- A globally routable address on one of the Mac's other interfaces stays reachable
  through the gateway. Run the host on networks that give the Mac private addresses.
- With these rules Softnet tracks every flow a VM starts, in a table of 32,768
  entries that refuses new connections while it is full. Idle entries lapse after 5
  minutes for TCP and 30 s for UDP. A VM that opens more connections than that
  within that time has new connections refused.

[Security](../conventions/security.md#softnet-confines-a-job-vms-network) records the
reasoning, and the accepted surface that remains.

## The Signed-In Session

`marshal build` on the Mac is supported from a terminal in the signed-in graphical
session of the user who owns the runners. SSH and headless use are out of scope.

Since macOS 15, Apple's Virtualization framework starts a VM only while the user's
login keychain is unlocked. Without it, Tart's documentation lists errors such as
`SecKeyCreateRandomKey_ios failed`, `Failed to generate keypair` and
`Interaction is not allowed with the Security Server`. A boot failure that `marshal`
logs, whether `tart run` ended or the guest agent did not answer in 120 s, names this
section, since a locked login keychain is the documented cause. The build deletes
the failed VM and exits 1, so unlocking the keychain and running `marshal build`
again is enough. FileVault rules out automatic login, so after a restart, sign in at
the screen first.

## The Golden VM Contract

The golden VM is the Ubuntu VM that every job VM will be cloned from. This section is
the contract between `build`, which makes it, and a runtime that clones it. The code
defines it, in `runtime::tart`, `golden` and `build::golden`.

- **Names.**
  - A golden VM is
    `marshal-golden-linux-g<golden VM version>-c<image contract version>-r<runner version>-<yyyyMMddHHmmss>`.
  - A build VM is `marshal-build-linux-<yyyyMMddHHmmssfff>`.
  - Only `build` creates golden and build VMs and deletes old golden VMs, and only
    `build` and a build VM's own shim delete a build VM. `build` matches a build VM
    by this exact form, so it never matches a VM `marshal` does not own.
- **The golden VM version** is a number in the CLI (`runtime::tart::names`),
  starting at 1. It goes up whenever the base digest, the provisioning steps, the
  launcher or the Docker Engine versions change.
- **Reading it.** The golden VM's name records its golden VM version, its image
  contract version, its runner version and its build time, so they are read from
  `tart list` without booting it. The newest golden VM by build time is the one a
  runtime uses. It must carry the CLI's golden VM version and image contract version;
  a runner version that differs is not a mismatch.
- **The base image** is `ghcr.io/cirruslabs/ubuntu:24.04`, pinned by the digest of its
  manifest (`golden::pins::BASE_REFERENCE`): arm64 Ubuntu 24.04 with a 20 GB disk and
  the guest agent, and no Docker. The golden VM keeps that disk size, so a pool's
  `diskGb` is at least 20. `build` pulls it by digest, so a tag that moves changes
  nothing, and removes from Tart's cache only the digests of the retired base pins
  that the CLI lists. A change of the pin raises the golden VM version and adds the
  old pin to that list.
- **What the build installs.**
  - The Ubuntu packages are upgraded.
  - Docker Engine (`docker-ce`, `docker-ce-cli`, `containerd.io` and
    `docker-buildx-plugin`) comes from Docker's apt repository at pinned package
    versions, which apt holds, after the repository's signing key has matched a
    recorded fingerprint. Docker Compose is not installed.
  - The launcher is `/usr/local/bin/marshal-launch`, owned by root, mode 0755.
  - The `admin` password is locked and SSH is masked, so the base image's
    `admin`/`admin` login cannot be used. The guest agent does not use SSH.
  - The runner image is built inside the VM under the configured `imageName`, which
    is `actions-runner:local` when the configuration gives none, and carries this
    CLI's image contract.
- **The guest user.** The guest agent runs commands as `admin`, the golden VM's
  user, which the golden VM puts in the `docker` group, so `docker` needs no `sudo`
  in a VM cloned from it. The source for this is the unit file
  `packaging/tart-guest-agent.service` (`User=admin`) of
  [cirruslabs/tart-guest-agent](https://github.com/cirruslabs/tart-guest-agent) at
  commit `6d4bd9c`.
- **The launcher's input.** `tart exec -i <vm> /usr/local/bin/marshal-launch <docker run arguments>`
  would run the launcher with the arguments of a `docker run`, and with the JIT
  configuration on standard input only. The launcher reads it, up to 64 KiB, into
  the environment of the `docker` client it then starts, so it appears in no argument
  list. It refuses an empty input or a longer one with exit code 64.
- **How a VM boots.** `tart run` is held by a shim, with these options and no other
  network option, no shared directory and no Rosetta:
  `--no-graphics --no-clipboard --no-audio --no-usb-accessories --net-softnet --net-softnet-allow "out proto udp @host port 53,out proto tcp @host port 53" --net-softnet-block "out @host,224.0.0.0/4"`.
  A rule with a port outranks one without, so DNS to the gateway passes while every
  other packet to the gateway and all multicast do not.
- **Who deletes what.** A build makes its VM under the build name, renames it to the
  golden name only when it is complete, and deletes every other golden VM only after
  that. A failed or interrupted build leaves the previous golden VM in place. A
  build VM is deleted by the build, by its shim when the CLI dies, within 15 s, and
  by the next build.

## Building the Golden VM

`marshal build --profile axmc --config <path>` builds the golden VM and replaces the
previous one only when the new one is complete. Run it after installing Tart and
Softnet, and after any change of the CLI that raises the golden VM version or the
image contract version.

**Before you start**

- No other `marshal build` or `marshal start` may be running: the build takes the
  same instance lock as `start`, and exits 1 saying so otherwise.
- Tart and Softnet must be installed and set up as
  [Installing Tart and Softnet](#installing-tart-and-softnet) and
  [Setting Up Softnet](#setting-up-softnet) say. The build checks both before it
  creates any VM, and exits 1 naming the section when one is missing or wrong.
- Sign in at the Mac's screen and run `marshal` from a terminal in that session, as
  [The Signed-In Session](#the-signed-in-session) says: the build boots a VM too.
- **Keep the Mac awake for the whole build.** `build` holds no keep-awake. A Mac that
  goes to sleep during the build stops the VM's work, and the build then fails or has
  to be stopped. Turn off the Mac's sleep for the time being, or run `caffeinate -i` in
  another terminal until the build ends.
- Leave room on the disk. The pull caches the 20 GB base image, the build VM is a
  clone of it that grows while the runner image is built, and the finished golden VM
  stays. This is the 40 GB that `validate` reserves beyond the pools' own disks. The
  build VM keeps the base's 20 GB disk, so the runner image and Docker's layers have
  to fit in it; a build that runs out of disk fails at the image build, with Docker's
  output in its log.

**What it does**

1. Checks the prerequisites above.
2. Deletes any build VM an earlier build left behind, `marshal-build-linux-<time>`.
3. Pulls the base image by its digest and clones it to a new build VM, which keeps
   the base's CPUs and memory.
4. Boots the build VM on Softnet and waits up to 120 s for its guest agent.
5. Upgrades the Ubuntu packages, installs Docker Engine at the pinned versions after
   checking Docker's signing key against its recorded fingerprint, installs the
   launcher, and disables the SSH login. A key whose fingerprint is not the recorded
   one fails the build before anything is installed from Docker.
6. Builds the runner image inside the VM, from the definition embedded in `marshal`,
   with the arguments `build` gives it on axpc, under the configured `imageName`, and
   checks its image contract label. The image build runs `sudo -n docker`, because
   the `docker` group reaches guest-agent commands only after the agent restarts.
   This needs the VM to reach the network: Docker's repository, the Ubuntu archive,
   and the registries the Dockerfile pulls from, on Softnet's rules, which allow
   globally routable addresses.
7. Shuts the VM down and renames it to the golden name,
   `marshal-golden-linux-g<golden VM version>-c<image contract version>-r<runner version>-<time>`.
   This rename is the moment the new golden VM takes over.
8. Deletes every other golden VM and the retired base images from Tart's cache. A
   failure here is only a warning; the next build tries again, and the newest golden
   VM is the one that is used.

`build` prints its steps to the terminal, and the image build's own output line by
line, masked as a job's output is.

**If it fails or is interrupted.** Ctrl+C, `marshal stop` from another terminal, and
a failure at any step before the rename delete the build VM within the 30 s stop
sequence and leave the previous golden VM as it was. After the rename the new golden
VM stays, and a stop only skips the clean-up in step 8. If `marshal` is killed or
crashes mid-build, a helper process stops and deletes the build VM within 15 s; if that fails too, the next `marshal build` deletes it by its
name. `tart list` shows whatever is left: `marshal-build-linux-<time>` is a build VM
that is safe to delete with `tart delete`, and `marshal-golden-linux-...` is a golden
VM.

**Which command line.** `marshal build --profile axmc` is only for macOS; on Windows
it exits 1 saying that the axmc profile runs only on macOS. `--profile` defaults to
`axmc` on macOS.

## Running the Host

### The host configuration

[`hosts/macos/runner-host.example.json`](../../hosts/macos/runner-host.example.json)
is the configuration to copy. It holds placeholders only: replace the owners,
repositories, names and token paths, and keep the files outside any checkout. Each
entry is a pool of `slots` runners for one repository:

- `os` is `linux` or `macos`, and `diskGb` is the per-VM disk cap. A Linux VM has
  whole vCPUs, so `cpus` is an integer from 1 to 64, and `diskGb` is at least 20, the
  golden VM's disk size.
- `labels` replaces the routing label `axmc`. A Linux runner registers with
  `self-hosted`, `linux`, `ARM64` and the routing label, so a workflow routes a job
  with `runs-on: [axmc, linux]`.
- `imageName`, when present, names the runner image inside the golden VM; without it
  the image is `actions-runner:local`.
- `tokenPath` names a file that holds one repository's token. Create the token as
  [Windows Runner Host](./windows-runner-host.md#the-fine-grained-token) describes,
  one fine-grained token per repository with the Administration permission. On macOS
  the file must be a regular file owned by you with mode `0600`: `chmod 600 <file>`.
- `budget` holds the Mac's `cpus`, `memoryGb` and `diskGb` for all pools together.
  The disk total counts every slot's `diskGb` plus a fixed 40 GB for the golden VM
  and Tart's image cache. The example's two slots of 4 vCPUs, 8 GB and 40 GB fit its
  budget of 8 CPUs, 16 GB and 120 GB exactly. [CLI](./cli.md#profiles) has every
  rule, its message, and the migration from the earlier rules.

`marshal validate --config <path> --profile axmc` checks a file on any machine.

The VMs run beside the macOS system, and Apple's licence allows two macOS guests at
once. `validate` limits macOS pools to two VMs and does not count Linux VMs. Whether
Linux VMs count against Apple's limit has not been found out; see
[Verification Status](#verification-status).

### Starting the pools

`marshal start --profile axmc` does not run the pools yet. It takes the instance
lock, validates the configuration, and exits 1 with
`error: the configuration is valid, but this version of marshal cannot run axmc's VM pools yet`.
Build the golden VM now with `marshal build`; the pools run in a later version.
`--profile axmc` is the default on macOS.

## Verification Status

**Nothing in this document has been run on a Mac.** No real Tart VM, guest agent or
Softnet has run under `marshal`, and no claim above about their behaviour rests on a
run. The host runs are performed on the maintainer's Mac, or by an agent session
there, and their evidence, with the literal output of `tart --version` and
`softnet --version`, is posted on
[#74](https://github.com/axross/runners/issues/74). This note changes only when that
evidence is there, and each item moves from the list below to a verified statement
with a link to it.

**What is checked without a Mac.** The CLI's tests run against fake `tart`, `docker`
and `softnet` programs:

- the version parsing and sorting of both programs, and the judgment of Softnet's
  file and the search for it on `PATH`;
- the `tart` client's argument lists and bounds, and the VM shim;
- the golden VM build, with a failure scripted at each step and at a stop.

[CLI](./cli.md) says what each test file covers.

**What the Mac run must confirm.** Each of these rests on a source or on the output
of an Apple tool as the source prints it, and not on a run:

- The guest agent runs commands as `admin`, and `admin` is in the `docker` group in a
  VM cloned from the golden VM without `sudo` (from the guest agent's source, not
  from a running VM).
- In the build VM, the image build works with `sudo -n docker`.
- The image build fits the golden VM's 20 GB disk, with BuildKit's cache, and the
  golden VM, its build and Tart's cache fit the 40 GB reserve; record their sizes.
- `tart delete` removes a cache entry pulled by digest, which is how `build` removes a
  retired base pin. The retired list is empty today, so no run has needed it.
- A locked login keychain makes a boot fail with the logged text that names
  [The Signed-In Session](#the-signed-in-session).
- Softnet's rules hold: from a VM, every blocked target that answers from the Mac is
  unreachable, and DNS and an HTTPS request to the internet succeed; with Softnet's
  SUID bit removed, `build` refuses and `tart run` boots no VM. A VM that reaches a
  blocked target stops this work until a plan revision decides the fallback.
- The build VM has no shared directory, Rosetta share, sound card or USB device.
- Whether Linux VMs count against Apple's limit of two VMs: three Linux VMs run at
  once, or the limit found is recorded here, `validate` gains a rule with its
  migration, and the example is reduced to fit.
