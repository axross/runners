# macOS Runner Host

How to run ephemeral Linux GitHub Actions runners on the maintainer's Apple
silicon Mac, the `axmc` host, with the CLI, `marshal`: each job runs in a fresh
Tart Ubuntu VM that is cloned from a golden VM, on Softnet, with no Docker
Desktop. This document is built up across the pull requests of
[#74](https://github.com/axross/runners/issues/74). Its section titles are fixed:
`marshal`'s refusal messages name them, so a title changes only with the code that
names it. The sections below say what holds today and what a later change adds.
[CLI](./cli.md) owns what each `marshal` command does, and the trust rules the host
is reviewed against are in [Security](../conventions/security.md). The Windows
equivalent is [Windows Runner Host](./windows-runner-host.md).

**Today.** `marshal start --profile axmc` runs the Linux pools of the configuration:
it checks the host prerequisites below, and then each slot runs a job VM per job,
cloned from the golden VM, with the run contract's Docker commands run inside it.
It skips each macOS pool with a warning, because macOS VM pools are not supported
yet, and exits 1 when every pool is skipped. `marshal build --profile axmc` builds
the golden VM. The procedures below that run Tart, Softnet or `marshal` on a Mac
have not been run there yet; see
[Verification Status](#verification-status).

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

`marshal` reads both versions with `--version`, each within 5 s, at `start`,
`restart` and `build`, and before each VM boot:

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

Softnet needs root privilege to filter a VM's network. Its privilege is a SUID bit
on its binary, with no `sudoers` rule, and `marshal` never sets it: it only checks
it, before every VM boot. Homebrew never sets the bit, and a `brew upgrade`
installs a new binary without it, so repeat this setup after each upgrade of Softnet
or Tart.

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

## The Signed-In Session

`marshal start` and `marshal build` on the Mac are supported from a terminal in the
signed-in graphical session of the user who owns the runners. Locking the screen
afterwards does not stop the runners. SSH and headless use are out of scope.

Since macOS 15, Apple's Virtualization framework starts a VM only while the user's
login keychain is unlocked. Without it, Tart's documentation lists errors such as
`SecKeyCreateRandomKey_ios failed`, `Failed to generate keypair` and `Interaction is
not allowed with the Security Server`. A boot failure that `marshal` logs, whether
`tart run` ended or the guest agent did not answer in 120 s, names this section,
since a locked login keychain is the documented cause. The slot then deletes the
failed VM and tries again on its backoff, so unlocking the keychain is enough. FileVault rules out
automatic login, so after a restart, sign in at the screen and then run `marshal
start` again.

## The Golden VM Contract

The golden VM is the Ubuntu VM that every job VM is cloned from. This section is the
contract that a later macOS VM runtime consumes. What the code defines today:

- **Names.** A job VM is `marshal-job-<pool>-<slot>-<yyyyMMddHHmmssfff>`. A golden VM
  is `marshal-golden-linux-g<golden VM version>-c<image contract version>-r<runner
version>-<yyyyMMddHHmmss>`. A build VM is
  `marshal-build-linux-<yyyyMMddHHmmssfff>`. Only `build` deletes golden and build
  VMs, and the cleanup at `start` matches only the job VMs of configured pools.
- **The golden VM version** is a number in the CLI (`runtime::tart::names`),
  starting at 1. It goes up whenever the base digest, the provisioning steps, the
  launcher or the Docker Engine versions change.
- **Reading it.** The golden VM's name records its versions and build time, so they
  are read from `tart list` without booting it.

- **A slot's cycle** (the [Tart Backend](./cli.md#tart-backend) in the CLI
  document has the budgets and texts). It clones the newest golden VM that runs
  here to a job VM, sizes it to the pool's `cpus`, `memoryGb` and `diskGb`, and
  boots it with `tart run` on Softnet. It waits up to 120 s for the guest agent,
  and then for the Docker Engine, and runs the CPU probe. It runs the job with
  `tart exec -i <vm> /usr/local/bin/marshal-launch <docker run arguments>`, with the
  JIT configuration on standard input only. When the job ends, it stops the VM and
  deletes it, so no job VM outlives its job.
- **The guest user.** The guest agent runs commands as `admin`, the golden VM's
  user, which the golden VM puts in the `docker` group, so `docker` needs no
  `sudo`. The source for this is the unit file `packaging/tart-guest-agent.service`
  (`User=admin`) of [cirruslabs/tart-guest-agent](https://github.com/cirruslabs/tart-guest-agent)
  at commit `6d4bd9c`.
- **Leftovers.** At `start`, each job VM of a configured pool that an earlier run
  left behind is stopped and deleted, and its diagnostic evidence, if the pool
  collects it, is not exported; the loss is reported in the log. A finished job's
  diagnostic evidence is exported from its VM, through the guest agent, into the
  pool's private `diagnosticDirectory` before the VM is deleted; a forced stop or a
  crash of `start` deletes the VM without that export and reports the loss.

- **The base image** is `ghcr.io/cirruslabs/ubuntu:24.04`, pinned by the digest of its
  manifest (`golden::pins::BASE_REFERENCE`): arm64 Ubuntu 24.04 with a 20 GB disk and
  the guest agent, and no Docker. The golden VM keeps that disk size, so a pool's
  `diskGb` is at least 20. `build` pulls it by digest, so a tag that moves changes
  nothing, and removes from Tart's cache only the digests of the retired base pins that
  the CLI lists. A change of the pin raises the golden VM version and adds the old pin
  to that list.
- **What the build installs.** The Ubuntu packages are upgraded. Docker Engine, `docker-ce`,
  `docker-ce-cli`, `containerd.io` and `docker-buildx-plugin`, comes from Docker's apt
  repository at pinned package versions, which apt holds, after the repository's signing
  key has matched a recorded fingerprint. Docker Compose is not installed. The launcher is
  `/usr/local/bin/marshal-launch`, owned by root, mode 0755. The `admin` password is
  locked and SSH is masked, so the base image's `admin`/`admin` login cannot be used; the
  guest agent does not use SSH. The runner image is built inside the VM under the
  configured `imageName`, which is `actions-runner:local` when the configuration gives
  none, and carries this CLI's image contract.
- **The launcher's input** is the JIT configuration on standard input and the arguments
  of a `docker run` as arguments; see a slot's cycle above.
- **Who deletes what.** Only `build` creates and deletes golden and build VMs. A build
  makes its VM under the build name, renames it to the golden name only when it is
  complete, and deletes every other golden VM only after that. A failed or interrupted
  build leaves the previous golden VM in place.

## Building the Golden VM

`marshal build --profile axmc --config <path>` builds the golden VM and replaces the
previous one only when the new one is complete. Run it after installing Tart and
Softnet, after any change of the CLI that raises the golden VM version or the image
contract version, when `marshal` reports a missing, mismatched or old golden VM, and
as often as the freshness warnings ask.

**Before you start**

- `marshal` must not be running, and no other `build`: the build takes the same
  instance lock as `start`, and exits 1 saying so otherwise.
- Tart and Softnet must be installed and set up as the sections
  [Installing Tart and Softnet](#installing-tart-and-softnet) and
  [Setting Up Softnet](#setting-up-softnet) say. The build checks both before it creates
  any VM, and exits 1 naming the section when one is missing or wrong.
- Sign in at the Mac's screen and run `marshal` from a terminal in that session, as
  [The Signed-In Session](#the-signed-in-session) says: the build boots a VM too.
- **Keep the Mac awake for the whole build.** `build` holds no keep-awake and ignores
  the sleep and network signals that stop a running instance. A Mac that goes to sleep
  during the build stops the VM's work, and the build then fails or has to be stopped.
  Turn off the Mac's sleep for the time being, or run `caffeinate -i` in another
  terminal until the build ends.
- The disk: the pull caches the 20 GB base image, the build VM is a clone of it that
  grows while the runner image is built, and the finished golden VM stays. This is the
  40 GB that `validate` reserves beyond the pools' own disks. The build VM keeps the
  base's 20 GB disk; the runner image and Docker's layers have to fit in it, and a build
  that runs out of disk fails at the image build with the output of Docker in its log.

**What it does**

1. Checks the prerequisites above.
2. Deletes any build VM an earlier build left behind, `marshal-build-linux-<time>`.
3. Pulls the base image by its digest and clones it to a new build VM, which keeps the
   base's CPUs and memory.
4. Boots the build VM on Softnet and waits up to 120 s for its guest agent.
5. Upgrades the Ubuntu packages, installs Docker Engine at the pinned versions after
   checking Docker's signing key against its recorded fingerprint, installs the
   launcher, and disables the SSH login. A key whose fingerprint is not the recorded one
   fails the build before anything is installed from Docker.
6. Builds the runner image inside the VM, from the definition embedded in `marshal`,
   with the arguments `build` gives it on axpc, under the configured `imageName`, and
   checks its image contract label. This needs the VM to reach the network: Docker's
   repository, the Ubuntu archive, and the registries the Dockerfile pulls from, on
   Softnet's rules, which allow globally routable addresses.
7. Shuts the VM down and renames it to the golden name,
   `marshal-golden-linux-g<golden VM version>-c<image contract version>-r<runner version>-<time>`.
   This rename is the moment the new golden VM takes over.
8. Deletes every other golden VM and the retired base images from Tart's cache. A failure
   here is only a warning; the next build tries again, and the newest golden VM is the one
   that is used.

**If it fails or is interrupted.** Ctrl+C, `marshal stop` from another terminal, and a
failure at any step before the rename delete the build VM within the 30 s stop sequence
and leave the previous golden VM as it was, so `start` is not affected. After the rename
the new golden VM stays, and a stop only skips the clean-up in step 8. If `marshal` is
killed or crashes mid-build, a helper process stops and deletes the build VM within 15 s;
if that fails too, the next `marshal build` deletes it by its name. `tart list` shows
whatever is left: `marshal-build-linux-<time>` is a build VM that is safe to delete with
`tart delete`, and `marshal-golden-linux-...` is a golden VM.

**Which command line.** `marshal build --profile axmc` is only for macOS. On Windows it
exits 1 saying that the axmc profile runs only on macOS. `--profile` defaults to axmc on
macOS. The build prints its steps to the terminal, and the image build's own output
line by line.

The procedure above has not been run on a Mac yet; see
[Verification Status](#verification-status).

## Running the Host

`marshal start --profile axmc` runs every `os: "linux"` pool, one fresh VM per
job, and skips each `os: "macos"` pool with a warning until macOS VM pools exist;
when every pool is skipped it exits 1. Before any VM boots it checks Tart and
Softnet, and a refusal makes it exit 1, naming the section of this document to
follow. A refusal after `start` holds that slot, which `marshal status` shows with
its reason, while jobs on other slots finish. This section will hold the host
configuration for the Mac, starting and stopping the pools, and `marshal status`. The example configuration is
[`hosts/macos/runner-host.example.json`](../../hosts/macos/runner-host.example.json).

## Sleep, Wake, and Network Changes

While runners run, `marshal` holds off the Mac's idle sleep. A sleep and its wake,
or a change of the primary network, stops every runner, deletes every VM and moves the
instance to stopped, with the reason in `marshal status` and in the terminal where
`marshal start` runs. The runners are never left to carry on across a sleep or onto
another network, because a VM that lived through either has a clock and a network
that no longer match its job. Only `marshal start` or `marshal restart` runs them
again. This section applies to `marshal start --profile axmc`; the Windows host is
not affected.

### Keeping the Mac Awake

- While the instance is running or resuming, and while it is stopping until the last
  VM is deleted, `marshal` runs `caffeinate -i -w <its own process ID>`. That stops
  idle sleep, and ends with `marshal` itself, even if `marshal` is killed hard.
- Stopped, the instance holds nothing: the Mac may sleep when it is idle. `marshal
build` never holds it either; keep the Mac awake for a build as
  [Building the Golden VM](#building-the-golden-vm) says.
- If `caffeinate` cannot start, or ends early, `marshal` logs
  `keep-awake is not held (<cause>), so the Mac may sleep when idle; trying again`
  once, shows it as a `warning:` line in `marshal status`, and tries again every 10
  seconds until it holds. A Mac that sleeps in the meantime stops every runner at its
  wake, as below, so a lost hold costs the running jobs and nothing else.
- The hold stops idle sleep only. Closing a laptop's lid, choosing Sleep from the
  Apple menu, or running out of battery still sleeps the Mac and stops every runner.

### What Stops Every Runner

`marshal` learns of a sleep only when the Mac wakes, so a sleep and its wake are one
stop. It compares how far the wall clock and the monotonic clock moved between two
samples one second apart, which macOS advances differently across a sleep, and
judges a gap of more than 3 seconds a sleep. A manual change of the clock by more
than 3 seconds counts as a sleep too, which only errs on the safe side. A sleep
shorter than that goes unseen, and leaves the VMs and the network as they were.

The primary network is the interface that holds the IPv4 default route, that route's
router, and the router's hardware address. It is read with `route -n get default` and
`arp -n <router>` every 5 seconds, each within 2 seconds, and only for IPv4, because
Softnet carries only IPv4. `marshal` records it when the instance starts running and
when it resumes, and it counts as changed when it differs from that record on two
checks in a row, so a stop begins within 15 seconds of a change.

| What happens                                                                   | Counts as a change |
| ------------------------------------------------------------------------------ | ------------------ |
| Joining another Wi-Fi network or a wired network                               | Yes                |
| Losing the default route for longer than one check                             | Yes                |
| A VPN that takes the default route, switched on or off                         | Yes                |
| Roaming between access points of one network, behind the same router           | No                 |
| A VPN that leaves the default route alone                                      | No                 |
| A router whose hardware address cannot be read at a check                      | No (not compared)  |
| Tart's and Softnet's own bridge interfaces, which never hold the default route | No                 |

A VPN that leaves the default route alone is not noticed, because Softnet's block on
private address ranges already covers the ranges such VPNs usually carry. A check
that cannot read the routing table counts as no default route.

When `marshal start` begins, or an instance resumes, with no IPv4 default route, as on
an IPv6-only network, `marshal` clones no VM until a primary network has held for two
checks in a row, and then records it. `marshal status` shows
`network: waiting for a network (no IPv4 default route)` meanwhile, and a stop still
ends the wait.

### The Stop Reasons

`marshal status` shows the stop as `state: stopped` and a `reason:` line, and the
terminal of `marshal start` prints it as `The instance is stopped: <reason>.`:

- `the Mac slept, so every runner was stopped`
- `the primary network changed (<old interface> to <new interface>), so every runner was stopped`,
  for example `(en0 to en0)` for another Wi-Fi network behind another router,
  `(en0 to utun4)` for a VPN that took the default route, and `(en0 to none)` when no
  interface holds the default route any more.

A reason names interfaces and never a network's name or an address. Before it says
stopped, the instance fails every running job, deletes every job VM within the same
30-second stop sequence `marshal stop` uses, and leaves the golden VM alone.

### Resuming

Nothing resumes on its own: a Mac that wakes, or a network that comes back, leaves
the instance stopped. Run `marshal start --profile axmc --config <path>` again from
any terminal of the same user, or `marshal restart`, which the stopped instance
answers by reading the configuration again, checking Tart, Softnet and the golden VM,
and starting every pool. The resume records the primary network and arms the sleep
check afresh, so the time spent stopped counts as neither a sleep nor a change. Do
this only on a network you trust, because the signals only stop the runners; they
never judge a network.

## Removing the VMs of a Pool No Longer Configured

`marshal` deletes leftover job VMs only for pools that are configured, so a VM of a
pool that was removed from the configuration stays. List the local VMs, and delete a
removed pool's job VMs by name with Tart:

```bash
tart list --source local
tart delete marshal-job-<pool>-<slot>-<time>
```

Delete only VMs whose names begin `marshal-job-` followed by the removed pool's
name, and none that is running. Never delete the golden VM by hand while `marshal`
runs; `build` replaces it.

## Verification Status

Nothing in this document has been run on the Mac yet. The host runs are performed on
the maintainer's Mac, and their evidence is posted on
[#74](https://github.com/axross/runners/issues/74). What is checked without a Mac:
the version parsing and sorting of both programs, and the judgment of Softnet's file
and the search for it on `PATH`, against fake `tart` and `softnet` programs in the
CLI's tests; and the VM runtime's slot cycle, probes, run arguments, release and VM
shim, and the golden VM build with its failure and stop cases, against fake
`tart`, `docker` and `softnet` programs; and `marshal start` on the axmc profile as
a process against the same fakes: the skipped macOS pools, each refusal before any
VM boots, a held slot, and a refused resume; the signals, driven by a scripted
source: a wake and a primary-network change that stop the instance with their
reasons, a resume that records the network afresh, no VM cloned without a default
route, and the keep-awake hold's state. No real sleep, `route`, `arp` or
`caffeinate` has been seen. The guest user comes from
the guest agent's source and has not been seen on a running VM. No real Tart VM, guest
agent or Softnet has run under `marshal`.
