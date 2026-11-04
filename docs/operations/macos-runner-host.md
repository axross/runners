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

**Today.** `marshal` checks the host prerequisites below, and the library holds the
VM runtime that will use them: a job VM per job, cloned from the golden VM, with the
run contract's Docker commands run inside it. But `marshal start` and `marshal build`
with `--profile axmc` still refuse to run: the configuration is validated, and the
VM runtime is not wired into them yet. The procedures below that run Tart, Softnet
or `marshal` on a Mac have not been run there yet; see
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
  collects it, is not exported.

The base image and its digest and the steps of the golden VM's build are added here
with the command that uses them.

## Building the Golden VM

`marshal build --profile axmc` will build the golden VM and replace the previous
one only when the new one is complete. This section will hold its procedure: the
prerequisites, the disk it needs beyond the pools' (the 40 GB reserve that
`validate` counts), keeping the Mac awake for the whole build, and what a failed or
interrupted build leaves behind.

## Running the Host

`marshal start --profile axmc` will run every `os: "linux"` pool, one fresh VM per
job, and skip each `os: "macos"` pool with a warning until macOS VM pools exist.
This section will hold the host configuration for the Mac, starting and stopping the
pools, and `marshal status`. The example configuration is
[`hosts/macos/runner-host.example.json`](../../hosts/macos/runner-host.example.json).

## Sleep, Wake, and Network Changes

While runners run, `marshal` will hold off the Mac's idle sleep. A sleep and its
wake, or a change of the primary network, will stop every runner, delete every VM
and move the instance to stopped, with the reason in `marshal status`. This section
will hold what counts as a change of the primary network and how to resume.

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
shim, against fake `tart`, `docker` and `softnet` programs. The guest user comes from
the guest agent's source and has not been seen on a running VM. No real Tart VM, guest
agent or Softnet has run under `marshal`.
