# macOS Runner Host

How ephemeral Linux GitHub Actions runners run on the maintainer's Apple silicon Mac,
the `axmc` host, with the `marshal-cli` library's VM runtime: each job runs in a fresh
Tart Ubuntu VM that is cloned from a golden VM, on Softnet, with no Docker Desktop. It
covers installing Tart and Softnet, setting up Softnet's privilege, the signed-in
session the host needs, the golden VM and how `build` makes it, running and stopping
the pools, what a sleep, a wake or a network change does, and the verification
status. It is the operator's side of [#74](https://github.com/axross/runners/issues/74).
Its titles for installing Tart and Softnet, setting up Softnet and the signed-in
session are fixed, because the library's messages name them; a title changes only
with the code that names it. [CLI](./cli.md) owns how the runtime works, and the trust
rules the host is reviewed against are in [Security](../conventions/security.md). The
Windows equivalent is [Windows Runner Host](./windows-runner-host.md).

**No supported way to run axmc yet.** Since W1's installer pull request no `marshal`
command line is built and no release carries a macOS file, so axmc has no supported
way to run its runners until M2's Mac app; the maintainer accepted that gap. A Mac
that runs the [last command-line release](./cli.md#the-last-command-line-release)
keeps it. That release is cut from `main` after M1, so its `start` and `build` run
axmc's pools in Tart VMs; `v0.1.0` predates M1 and refuses `--profile axmc`. That
release's tag holds this document's procedures with their
commands: installing `marshal`, `build`, `start` and its clients `status`, `restart`,
`stop` and `diagnostics`. Below, `start` and `build` name the library's entry points
of those names, which that release ran as commands and which M2's Mac app is to run.

`start` on the axmc profile runs the `os: "linux"` pools of the configuration. It
skips each `os: "macos"` pool with a warning, because macOS VM pools are not supported
yet, and refuses when every pool is skipped. No procedure below has been run on a Mac
yet; see [Verification Status](#verification-status).

## Installing Tart and Softnet

The runtime installs, bundles and repairs neither program. It calls `tart` as an
external program, and Tart runs the `softnet` that comes first on the `PATH` it
gets, which is the `PATH` of the process that runs the runtime. Both programs need
an Apple silicon Mac, and Softnet 0.25.0 needs macOS 26.

The tested versions are recorded in the CLI (`macos::version`):

| Program | Tested version | `--version` prints              |
| ------- | -------------- | ------------------------------- |
| Tart    | 2.41.1         | `2.41.1`                        |
| Softnet | 0.25.0         | `softnet 0.25.0-<short commit>` |

Install them with Homebrew, which installs Softnet as a dependency of Tart. The
`openai/tools` tap is not one Homebrew trusts by default, and on the first host run
Homebrew refused the tap's `softnet` formula until `brew trust` named it:

```bash
brew tap openai/tools
brew trust --formula openai/tools/softnet
brew install openai/tools/tart
tart --version
softnet --version
```

The runtime reads both versions with `--version`, each within 5 s, at `start` and
`build` and before each VM boot, and each within 2 s, together, at `restart` and when
`start` resumes a stopped instance, where a client waits for the answer:

- The tested version runs.
- A newer patch of the tested minor version runs, with a warning that names the
  tested version.
- Anything else is refused with a message that names the tested version and this
  section: an older patch, another minor or major version, output with a fourth
  number or any suffix (Softnet's `-<short commit>` aside), and output with no
  version. A `tart` or `softnet` that is not on `PATH` is refused the same way.

Homebrew installs the current release of each program. This document does not yet
record a way to hold a version back, so when Homebrew's current version is another
minor version than the tested one, the runtime refuses it until the tested versions
are updated in the CLI.

## Setting Up Softnet

Softnet filters a VM's network, and it needs root privilege to do so. Its privilege
is a SUID bit on its binary, with no `sudoers` rule, and the runtime never sets it: it
only checks it, before every VM boot. Homebrew never sets the bit, and a
`brew upgrade` installs a new binary without it, so repeat this setup after each
upgrade of Softnet or Tart.

The runtime finds `softnet` as Tart does, as the first match on `PATH`, and follows
its links. Homebrew's file in `/opt/homebrew/bin` is a link to a versioned file, and
the setup applies to that file, not to the link. The file must be a regular file,
owned by root, with the SUID bit set and no write permission for its group or others:

```bash
softnet_file="$(realpath "$(command -v softnet)")"
sudo chown root "$softnet_file"
sudo chmod u+s,go-w "$softnet_file"
ls -l "$softnet_file"
```

The listing starts with `-rwsr-xr-x` and names `root` as the owner. The runtime
refuses with a message that names the failing requirement when the file is a
directory or another kind of file, is owned by another user, lacks the SUID bit, or
can be written by its group or others. It also refuses a `softnet` that does not
answer `--version` within its bound (5 s, or 2 s at a restart or a resume).

**What the network allows.** Every VM boots with the two rules in
[The Golden VM Contract](#the-golden-vm-contract), on top of Softnet's defaults. A
VM can then send to globally routable IPv4 addresses and to DNS on the Mac's address
on Softnet's bridge, the gateway, and to nothing else: not the local network, not
another VM, not a multicast group, not an IPv6 address, and no other port on the Mac.
Four consequences matter to an operator:

- A job cannot reach a service on a private address, such as an internal package
  mirror, and cannot use an IPv6-only network.
- DNS goes through the Mac's resolver, which can answer the names of the Mac's
  networks and VPNs, though their addresses stay blocked.
- A globally routable address on one of the Mac's other interfaces stays reachable
  through the gateway. Run the host on networks that give the Mac private addresses.
- With these rules Softnet tracks every flow a VM starts, in a table of 32,768
  entries that refuses new connections while it is full. Idle entries lapse after 5
  minutes for TCP and 30 s for UDP. A job that opens more connections than that
  within that time has new connections refused, and the failure stays inside that
  job.

[Security](../conventions/security.md#softnet-confines-a-job-vms-network) records the
reasoning, and the accepted surface that remains.

## The Signed-In Session

`start` and `build` on the Mac need the signed-in graphical session of the user who
owns the runners; the last command-line release ran them from a terminal in it. Locking the screen
afterwards does not stop the runners. SSH and headless use are out of scope.

Since macOS 15, Apple's Virtualization framework starts a VM only while the user's
login keychain is unlocked. Without it, Tart's documentation lists errors such as
`SecKeyCreateRandomKey_ios failed`, `Failed to generate keypair` and
`Interaction is not allowed with the Security Server`. A boot failure that the runtime
logs, whether `tart run` ended or the guest agent did not answer in 120 s, names this
section, since a locked login keychain is the documented cause. The slot then
deletes the failed VM and tries again on its backoff, so unlocking the keychain is
enough. FileVault rules out automatic login, so after a restart, sign in at the
screen and then start the runners again.

## The Golden VM Contract

The golden VM is the Ubuntu VM that every job VM is cloned from. This section is the
contract between `build`, which makes it, and a runtime that clones it. The code
defines it, in `runtime::tart`, `golden` and `build::golden`.

- **Names.**
  - A job VM is `marshal-job-<pool>-<slot>-<yyyyMMddHHmmssfff>`, in UTC, with the
    slot counted from 1.
  - A golden VM is
    `marshal-golden-linux-g<golden VM version>-c<image contract version>-r<runner version>-<yyyyMMddHHmmss>`.
  - A build VM is `marshal-build-linux-<yyyyMMddHHmmssfff>`.
  - Only `build` creates golden and build VMs and deletes golden VMs, and only `build`
    and a build VM's own shim delete a build VM. Cleanup at `start` matches only the
    job VMs of configured pools by this exact form, so it never matches a golden VM,
    a build VM, a VM of a pool that is no longer configured, or a VM the runtime does
    not own.
- **The golden VM version** is a number in the CLI (`runtime::tart::names`),
  starting at 1. It goes up whenever the base digest, the provisioning steps, the
  launcher or the Docker Engine versions change. It is 2 with the `docker` group's
  fixed id, and the golden VM's image contract version is 2 with the runner image's
  Docker client, so a golden VM built before is refused as mismatched until
  the golden VM is built again.
- **Reading it.** The golden VM's name records its golden VM version, its image
  contract version, its runner version and its build time, so `start`, `restart`,
  each job launch and `status` read them from `tart list` without booting it. The
  newest golden VM by build time is the one used. It must carry the CLI's golden VM
  version and image contract version, or the runtime refuses it as a mismatched image
  is refused, naming the golden VM; a runner version that differs is not a mismatch.
  A missing golden VM is refused the same way, and the build time drives the
  freshness warnings: a golden VM 7 to 20 days old, or 21 days or more, asks for a
  rebuild.
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
    recorded fingerprint. Docker Compose is not installed. The `docker` group is
    created after the key check and before any Docker package is installed, with the id
    in `golden::pins::DOCKER_GROUP_ID`, and the build fails if the group exists with
    another id.
  - The launcher is `/usr/local/bin/marshal-launch`, owned by root, mode 0755.
  - The `admin` password is locked and SSH is masked, so the base image's
    `admin`/`admin` login cannot be used. The guest agent does not use SSH.
  - The runner image is built inside the VM under the configured `imageName`, which
    is `actions-runner:local` when the configuration gives none, and carries this
    CLI's image contract.
- **The Docker socket.** The Docker Engine in a job VM listens on
  `/var/run/docker.sock`, owned by the `docker` group. The job's runner container
  mounts that socket, so a job step can run `docker` against the VM's own engine. The
  socket belongs to the job's own VM, which is deleted when the job ends;
  [Security](../conventions/security.md#the-vm-local-docker-socket) records why that is
  permitted, what it does not extend to, and why the runner needs no `--group-add`.
  The image's Docker client is installed from the same repository, at the same
  version.
- **The guest user.** The guest agent runs commands as `admin`, the golden VM's
  user, which the golden VM puts in the `docker` group, so `docker` needs no `sudo`
  in a job VM. The source for this is the unit file
  `packaging/tart-guest-agent.service` (`User=admin`) of
  [cirruslabs/tart-guest-agent](https://github.com/cirruslabs/tart-guest-agent) at
  commit `6d4bd9c`.
- **The launcher's input.** `tart exec -i <vm> /usr/local/bin/marshal-launch <docker run arguments>`
  runs the launcher with the arguments of a `docker run`, and with the JIT
  configuration on standard input only. The launcher reads it, up to 64 KiB, into
  the environment of the `docker` client it then starts, so it appears in no argument
  list. It refuses an empty input or a longer one with exit code 64.
- **A slot's cycle.** The [Tart Backend](./cli.md#tart-backend) in the CLI document
  has the budgets and texts. For each job, a slot:
  1. checks the golden VM and the host prerequisites, and waits until the Mac has a
     primary network;
  2. clones the newest golden VM that runs here to a job VM, with `tart clone`, and
     sizes it with `tart set --cpu <cpus> --memory <MB> --disk-size <GB>` from the
     pool's `cpus`, `memoryGb` and `diskGb`;
  3. boots it with `tart run`, held by a shim, with these options and no other
     network option, no shared directory and no Rosetta:
     `--no-graphics --no-clipboard --no-audio --no-usb-accessories --net-softnet --net-softnet-allow "out proto udp @host port 53,out proto tcp @host port 53" --net-softnet-block "out @host,224.0.0.0/4"`.
     A rule with a port outranks one without, so DNS to the gateway passes while
     every other packet to the gateway and all multicast do not;
  4. waits up to 120 s for the guest agent, then up to 60 s for the Docker Engine,
     and runs the CPU probe; a failed probe ends the cycle before a runner registers;
  5. registers a JIT runner, and runs the job container with `--pull never`, with
     the container's CPU quota and affinity covering all the VM's vCPUs and its
     memory limit equal to the VM's, and with the VM's Docker socket mounted;
  6. when the job ends, exports diagnostics for a diagnostics-enabled pool, stops the
     VM and deletes it, so no job VM outlives its job.
- **Who deletes what.** A job VM is deleted by its slot, on every path out of the
  cycle; by the stop sequence; by its shim when the CLI dies, within 15 s; and by the
  next `start`, which deletes a configured pool's leftovers without exporting their
  diagnostic evidence and reports the loss in the log. A build makes its VM under the
  build name, renames it to the golden name only when it is complete, and deletes
  every other golden VM only after that. A failed or interrupted build leaves the
  previous golden VM in place.

## Building the Golden VM

`build` on the axmc profile builds the golden VM and replaces the previous one only
when the new one is complete. It is needed after installing Tart and Softnet, after
any change of the library that raises the golden VM version or the image contract
version, when the runtime reports a missing or mismatched golden VM, and when the
freshness warnings ask for it. The last command-line release ran it as
`marshal build --profile axmc`, at its tag.

**Before you start**

- No instance and no other `build` may run: the build takes the same instance lock
  as `start`, and refuses saying so otherwise.
- Tart and Softnet must be installed and set up as
  [Installing Tart and Softnet](#installing-tart-and-softnet) and
  [Setting Up Softnet](#setting-up-softnet) say. The build checks both before it
  creates any VM, and exits 1 naming the section when one is missing or wrong.
- Sign in at the Mac's screen and run the build in that session, as
  [The Signed-In Session](#the-signed-in-session) says: the build boots a VM too.
- **Keep the Mac awake for the whole build.** `build` holds no keep-awake and
  ignores the sleep and network signals that stop a running instance. A Mac that goes
  to sleep during the build stops the VM's work, and the build then fails or has to
  be stopped. Turn off the Mac's sleep for the time being, or run `caffeinate -i` in
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
5. Upgrades the Ubuntu packages, checks Docker's signing key against its recorded
   fingerprint, creates the `docker` group with the id in `golden::pins::DOCKER_GROUP_ID`,
   installs Docker Engine at the pinned versions, installs the launcher, and disables the
   SSH login. A key whose fingerprint is not the recorded one fails the build before
   anything is installed from Docker.
6. Builds the runner image inside the VM, from the definition embedded in the library,
   with the arguments `build` gives it on axpc, under the configured `imageName`, and
   checks its image contract label. The image build runs `sudo -n docker`, because
   the `docker` group reaches guest-agent commands only after the agent restarts.
   This needs the VM to reach the network: Docker's repository, the Ubuntu archive,
   and the registries the Dockerfile pulls from, on Softnet's rules, which allow
   globally routable addresses.
7. Writes the VM's files to its disk with `sync`, stops it, and renames it to the
   golden name,
   `marshal-golden-linux-g<golden VM version>-c<image contract version>-r<runner version>-<time>`.
   This rename is the moment the new golden VM takes over.
8. Deletes every other golden VM and the retired base images from Tart's cache. A
   failure here is only a warning; the next build tries again, and the newest golden
   VM is the one that is used.

`build` writes its steps to its standard error, and the image build's own output
line by line, masked as a job's output is.

**If it fails or is interrupted.** A stop, by a signal or a client's `stop`, and a
failure at any step before the rename delete the build VM within the 30 s stop
sequence and leave the previous golden VM as it was, so `start` is not affected.
After the rename the new golden VM stays, and a stop only skips the clean-up in step 8. If the build's process is killed or crashes mid-build, a helper process stops and deletes the
build VM within 15 s; if that fails too, the next `build` deletes it by its
name. `tart list` shows whatever is left: `marshal-build-linux-<time>` is a build VM
that is safe to delete with `tart delete`, and `marshal-golden-linux-...` is a golden
VM.

**Which host.** `build` on the axmc profile is only for macOS; on Windows it refuses,
saying that the axmc profile runs only on macOS.

## Running the Host

### The host configuration

[`hosts/macos/runner-host.example.json`](../../hosts/macos/runner-host.example.json)
is the configuration to copy. It holds placeholders only: replace the owners,
repositories, names and token paths, and keep the files outside any checkout. It is
the JSON form the last command-line release reads.
[`hosts/macos/config.example.toml`](../../hosts/macos/config.example.toml) is the
same configuration in the TOML form the resident agent reads from
`~/Library/Application Support/marshal/config.toml`, as
[The Agent's Configuration](./cli.md#the-agents-configuration) describes. Each
entry is a pool of `slots` runners for one repository:

- `os` is `linux` or `macos`, and `diskGb` is the per-VM disk cap. A VM has whole
  vCPUs, so `cpus` is an integer from 1 to 64 on every pool, and a Linux pool's
  `diskGb` is at least 20, the golden VM's disk size.
- `labels` replaces the routing label `axmc`. A Linux runner registers with
  `self-hosted`, `linux`, `ARM64` and the routing label, so a workflow routes a job
  with `runs-on: [axmc, linux]`.
- `imageName`, when present, names the runner image inside the golden VM; without it
  the image is `actions-runner:local`.
- `tokenPath` names a file that holds one repository's token, for the command line;
  the TOML form has none, and the agent has no token store on macOS yet. Create the token as
  [Windows Runner Host](./windows-runner-host.md#the-fine-grained-token) describes,
  one fine-grained token per repository with the Administration permission. On macOS
  the file must be a regular file owned by you with mode `0600`: `chmod 600 <file>`.
- `budget` holds the Mac's `cpus`, `memoryGb` and `diskGb` for all pools together.
  The disk total counts every slot's `diskGb`, plus a fixed 40 GB for the golden VM
  and Tart's image cache when any pool is Linux. The example's two slots of 4
  vCPUs, 8 GB and 40 GB fit its budget of 8 CPUs, 16 GB and 120 GB exactly.
  [CLI](./cli.md#profiles) has every rule, its message, and the migration from the
  earlier rules.

The last command-line release's `validate --profile axmc` checks a JSON file on any
machine.

The VMs run beside the macOS system, and Apple's licence allows two macOS guests at
once. `validate` limits macOS pools to two VMs and does not count Linux VMs. Whether
Linux VMs count against Apple's limit is open, and no host run tests it; see
[Verification Status](#verification-status).

### Starting, watching and stopping

Before the first `start` on axmc, confirm that every self-hosted job in each
repository the pools serve names a routing label: `axpc`, or `axmc` with an OS label
such as `linux`, as in `runs-on: [axmc, linux]`. Use the label your pools carry in
place of `axmc` when `labels` replaces it. A job that names neither, such as one that
names only `self-hosted`, can land on axmc. Search each repository's workflows for
`runs-on` and check every hit.

With the last command-line release, the golden VM is built once and the pools are
started from a terminal of the signed-in session, as that release's tag gives with
the commands. `start` validates the configuration, opens its log under
`~/Library/Logs/marshal/`, checks Tart and Softnet, checks the golden VM, prepares
every Linux pool, and runs every slot until it is stopped. It also holds off idle
sleep and watches for a sleep or a network change; see
[Sleep, Wake, and Network Changes](#sleep-wake-and-network-changes). It refuses when
the configuration is invalid, when a prerequisite refuses, when the golden VM is
missing or mismatched, or when no pool can start. Nothing starts it at sign-in.

Its clients `status`, `restart`, `stop` and `diagnostics` report the state, each pool
and slot, the golden VM and its age, any warning and a stop reason; stop every job,
delete its VM and read the configuration again; stop the instance within 30 s and end
every VM; and finalise the diagnostic VMs still retained.
[CLI](./cli.md#running-a-host) gives each one's fields and waits. In the status, a
slot is `registering` while its VM is cloned, booted and probed,
`running container <name>` while it holds a job, `backing off` after a failure, and
`held: <reason>` when a prerequisite refuses its next boot. A slot that is held boots
nothing and tries again on its backoff, while jobs on other slots finish; fix the
cause, such as Softnet's SUID bit after a `brew upgrade`, and the slot boots again. A
pool of `os: "macos"` shows `skipped: macOS VM pools are not supported yet`.

A client's `stop`, a signal, or closing the terminal of `start` runs the same stop
sequence, which stops the job containers and
deletes every VM within 30 s. A second Ctrl+C skips the grace period: a diagnostic VM
is then deleted without its export. When `start` is killed hard,
each VM's shim deletes its VM within 15 s, and the next `start` takes the instance
lock at once and deletes whatever is left. One instance runs per machine: a `start`
while one runs is refused, and a `start` with the same configuration resumes a stopped
instance.

### Health checks

- **GitHub:** each repository's **Settings, Actions, Runners** lists the sum of its
  pools' `slots` runners, **Idle** while no job is queued, with the labels
  `self-hosted`, `linux`, `ARM64` and the routing label.
- **The instance:** the status shows `state: running`, a `pool:` line for every
  entry, `image-contract:` as the CLI's number rather than `mismatch` or `missing`, and
  `golden-vm:` with the golden VM's name. A `warning:` line is a freshness warning, a
  newer patch of Tart or Softnet, or a keep-awake hold that is lost.
- **The machine:** `tart list --source local` shows the golden VM and, for each slot,
  one running `marshal-job-<pool>-<slot>-<time>` VM, which is an idle runner or one
  with a job. A VM left after a stop is a leftover.
- **The log:** `~/Library/Logs/marshal/marshal-<time>-<pid>.log` keeps the newest ten
  runs. `Using Tart <v> and Softnet <v>.` records the versions in use.

## Sleep, Wake, and Network Changes

While runners run, the runtime holds off the Mac's idle sleep. A sleep and its wake, or
a change of the primary network, stops every runner, deletes every VM and moves the
instance to stopped, with the reason in its status and its log. The runners are never left to carry on across a sleep or onto
another network, because a VM that lived through either has a clock and a network
that no longer match its job. Only a new start or a `restart` runs them again. This
section applies to the axmc profile; the Windows host is not affected.

### Keeping the Mac Awake

- While the instance is running or resuming, and while it is stopping until the last
  VM is deleted, the runtime runs `caffeinate -i -w <its own process ID>`. That stops
  idle sleep, and ends with that process, even if it is killed hard.
- Stopped, the instance holds nothing: the Mac may sleep when it is idle.
  `build` never holds it either; keep the Mac awake for a build as
  [Building the Golden VM](#building-the-golden-vm) says.
- If `caffeinate` cannot start, or ends early, the runtime logs
  `keep-awake is not held (<cause>), so the Mac may sleep when idle; trying again`
  once, shows it as a `warning:` line in the status, and tries again every 10
  seconds until it holds. A Mac that sleeps in the meantime stops every runner at its
  wake, as below, so a lost hold costs the running jobs and nothing else.
- The hold stops idle sleep only. Closing a laptop's lid, choosing Sleep from the
  Apple menu, or running out of battery still sleeps the Mac and stops every runner.

### What Stops Every Runner

The runtime learns of a sleep only when the Mac wakes, so a sleep and its wake are one
stop. It compares how far the wall clock and the monotonic clock moved between two
samples one second apart, which macOS advances differently across a sleep, and
judges a gap of more than 3 seconds a sleep. A manual change of the clock by more
than 3 seconds counts as a sleep too, which only errs on the safe side. A sleep
shorter than that goes unseen, and leaves the VMs and the network as they were.

The primary network is the interface that holds the IPv4 default route, that route's
router, and the router's hardware address. It is read with `route -n get default` and
`arp -n <router>` every 5 seconds, each within 2 seconds, and only for IPv4, because
Softnet carries only IPv4. The runtime records it when the instance starts running and
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

When `start` begins, or an instance resumes, with no IPv4 default route, as on
an IPv6-only network, the runtime clones no VM until a primary network has held for two
checks in a row, and then records it. The status shows
`network: waiting for a network (no IPv4 default route)` meanwhile, and a stop still
ends the wait.

### The Stop Reasons

The status shows the stop as `state: stopped` and a `reason:` line, and the log of
`start` records it as `The instance is stopped: <reason>.`:

- `the Mac slept, so every runner was stopped`
- `the primary network changed (<old interface> to <new interface>), so every runner was stopped`,
  for example `(en0 to en0)` for another Wi-Fi network behind another router,
  `(en0 to utun4)` for a VPN that took the default route, and `(en0 to none)` when no
  interface holds the default route any more.

The reason names only interfaces, so a change of the router, or of the router's
hardware address, on the same interface also reads `(en0 to en0)`, with the same
name twice. The text does not say which of the interface, the router or the hardware
address differed; `route -n get default` shows the router that holds the route now.

A reason names interfaces and never a network's name or an address. Before it says
stopped, the instance fails every running job, deletes every job VM within the same
30-second stop sequence a `stop` uses, and leaves the golden VM alone.

### Resuming

Nothing resumes on its own: a Mac that wakes, or a network that comes back, leaves
the instance stopped. A new `start` with the same configuration, by the same user,
or a `restart`, which the stopped instance
answers by reading the configuration again, checking Tart, Softnet and the golden VM,
and starting every pool. The resume records the primary network and arms the sleep
check afresh, so the time spent stopped counts as neither a sleep nor a change. Do
this only on a network you trust, because the signals only stop the runners; they
never judge a network.

## Removing the VMs of a Pool No Longer Configured

The runtime deletes leftover job VMs only for pools that are configured, so a VM of a
pool that was removed from the configuration stays. Cleanup at `start` matches a job
VM by the exact name form `marshal-job-<pool>-<slot>-<yyyyMMddHHmmssfff>` and a
configured pool's name, and it never touches a golden VM, a build VM or any other VM.
List the local VMs, and delete a removed pool's job VMs by name with Tart:

```bash
tart list --source local
tart delete marshal-job-<pool>-<slot>-<time>
```

Delete only VMs whose names begin `marshal-job-` followed by the removed pool's name
and a slot number, and none that is running; a running one is a VM of an instance
that still uses that name, so stop the instance first. Never delete the golden VM by
hand while the runtime runs; `build` replaces it.

## Verification Status

**Nothing in this document has been run on a Mac.** No real Tart VM, guest agent,
Softnet, `caffeinate`, `route` or `arp` has run under the runtime, and no claim above
about their behaviour rests on a run. By the maintainer's decision
[recorded on #74](https://github.com/axross/runners/issues/74#issuecomment-6784246305),
this document's procedures were added with no host run. The one host run planned
comes with the arm64 runner image, and checks only that a Linux job that uses
`docker run` runs in a Tart VM with no Docker Desktop, that closing the lid or
switching networks stops the runners, and that `stop` leaves no job VM while the
golden VM stays. Its evidence, with the literal output of `tart --version` and
`softnet --version`, is posted on #74. An item below moves to a verified statement,
with a link to the evidence, only when a run's evidence covers it.

**The host run.** Jobs come from a workflow in a private repository the maintainer
chooses, since this repository is public and never uses self-hosted runners. After
the routing check in [Starting, watching and stopping](#starting-watching-and-stopping),
the run goes in this order:

1. Confirm that no Docker Desktop is installed, and record the macOS version, the
   literal output of `tart --version` and `softnet --version`, and Softnet's SUID bit
   as [Setting Up Softnet](#setting-up-softnet) lists it.
2. Install the last command-line release's macOS binary with the macOS procedure at
   its tag, since no later build carries one, and record its version and the commit
   it was built from.
3. Build the golden VM, then start the pools, with that release's `build` and
   `start`, as its tag's copy of
   [Starting, watching and stopping](#starting-watching-and-stopping) gives, with the
   sizing of [the example configuration](../../hosts/macos/runner-host.example.json).
   Record the golden VM's and Tart's cache's disk use.
4. Run a job with `runs-on: [axmc, linux]` and a step that uses `docker run`, and
   confirm with `tart list` that the job's VM runs while it does. Record the
   runner's labels and the slot's turnaround. This step also covers the mount of the
   VM's Docker socket and the match of the two `docker` groups.
5. With no external display attached, close the lid for about 15 s during a job, and
   switch networks during another job. Confirm that each stops the runners, with the
   reason in the status, and that `tart list` then shows no job VM. The Mac
   takes several seconds to enter sleep after the lid closes, so a close of about 5 s
   can sleep for less than the 3 s a sleep needs to be seen; on the first host run it
   slept about 3 s and nothing stopped.
6. Stop the instance with its `stop`, then confirm with `tart list` that no job or
   build VM is left
   and that the golden VM stays.

**What is checked without a Mac.** The CLI's tests run against fake `tart`, `docker`,
`softnet`, `caffeinate`, `route` and `arp` programs and a scripted signal source:

- the version parsing and sorting of both programs, and the judgment of Softnet's
  file and the search for it on `PATH`;
- the VM runtime's slot cycle, probes, run arguments, release and VM shim, and the
  diagnostics export from a VM;
- the golden VM build, with a failure scripted at each step and at a stop;
- `start` on the axmc profile as a process, through the test helper: the skipped macOS pools, each
  refusal before any VM boots, a held slot, and a refused resume;
- the signals: a wake and a primary-network change that stop the instance with their
  reasons, a resume that records the network afresh, no VM cloned without a default
  route, and the keep-awake hold's state.

[CLI](./cli.md) says what each test file covers.

**What no run has confirmed.** Each of these rests on a source or on the output
of an Apple tool as the source prints it, and not on a run:

- The guest agent runs commands as `admin`, and `admin` is in the `docker` group in a
  job VM without `sudo` (from the guest agent's source, not from a running VM).
- In the build VM, the image build works with `sudo -n docker`, and the build creates
  the `docker` group on the pinned base, which has no `docker` group with another id.
- A job step's `docker run` works in the runner container against the VM's own Docker
  Engine: the VM's socket is mounted and the `runner` user can use it through the group
  match that [Security](../conventions/security.md#the-vm-local-docker-socket) describes,
  with no `--group-add`. The Docker client in the
  image is the pinned version, and the Engine accepts its API version.
- The image build fits the golden VM's 20 GB disk, with BuildKit's cache, and the
  golden VM, its build and Tart's cache fit the 40 GB reserve; record their sizes.
- `tart delete` removes a cache entry pulled by digest, which is how `build` removes a
  retired base pin. The retired list is empty today, so no run has needed it.
- `route -n get default` under a VPN that takes the default route prints a
  `gateway: link#<index>` or no `gateway:` line, which the CLI reads as an interface
  without a router; and `arp -n <router>` prints the form it reads.
- A sleep is detected: `Instant` stops across a sleep on the pinned toolchain, a short
  lid close of about 15 s and a long one of about 60 s each stop every slot within 10 s
  of the wake, and a network change stops them within 15 s.
- `caffeinate -i -w <pid>` holds the Mac awake while runners run and releases it after
  `stop`, as `pmset -g assertions` shows.
- A locked login keychain makes a boot fail with the logged text that names
  [The Signed-In Session](#the-signed-in-session), and locking the screen after
  `start` does not stop a new job's VM from booting.
- Softnet's rules hold: from a VM, every blocked target that answers from the Mac is
  unreachable, and DNS and an HTTPS request to the internet succeed; with Softnet's
  SUID bit removed, `start` refuses and `tart run` boots no VM. No host run tests
  this, and the maintainer accepted the isolation as unverified
  ([decision record](https://github.com/axross/runners/issues/74#issuecomment-6784246305)).
- The VM has the pool's vCPUs, memory and disk, and no shared directory, Rosetta share,
  sound card or USB device, and the `admin` password login over SSH is refused.
- Whether Linux VMs count against Apple's limit of two VMs. No host run tests it,
  and `validate` counts only macOS VMs against the limit.
- `stop` ends every VM within 30 s, a hard-killed `start` leaves no running job VM
  after the next `start`, and a slot's next VM is cloned, booted, probed and
  registered within 60 s of the previous VM's deletion; record the turnaround.
- The modes of the control socket, the state, log and diagnostic files, and the
  refusal of another OS user, on macOS.
