---
type: Procedure
title: "macOS Runner Host"
description: "Prepare Tart, Softnet and a signed-in session on Apple silicon, and distinguish the historical CLI from the app release route."
---

# macOS Runner Host

On the maintainer's Apple silicon Mac (`axmc`), the `marshal-cli` runtime runs each job in a fresh Tart VM cloned from a golden Ubuntu or macOS image. Softnet provides networking. The host does not need Docker Desktop. Prepare the tools and session here, then [build the golden VM](./golden-build.md#building-the-golden-vm) and [start the pools](./mac-pools.md#starting-watching-and-stopping). The operator procedures are tracked in [#74](https://github.com/axross/runners/issues/74).
The titles for installing Tart and Softnet, setting up Softnet and the signed-in
session are fixed, because the library's messages name them; a title changes only
with the code that names it. [CLI](../marshal-cli/index.md) owns how the runtime works, and the trust
rules the host is reviewed against are in [Security](../security/index.md). The
Windows equivalent is [Windows Runner Host](./index.md#windows-with-docker-desktop).

**No supported app release for axmc yet.** W1's app release baseline builds no
`marshal` command line and carries no macOS file; M2's Mac app remains the planned
release route. During bounded F0 recovery, the temporary release workflow instead
builds the fixed historical CLI, including the macOS bare binary and legacy pkg.
[recovery CLI installation and rollback](../runner-recovery/install.md#installing-updating-and-rolling-back) owns the actual-version installation lifecycle
procedures. The recovery is not a supported parallel CLI product, and no recovery
release version or native installer evidence is recorded
yet. App source and normal builds remain; app publication resumes only after
successful public F0 evidence and a separate reviewed restoration.

A Mac that already runs the command line keeps it. [The Last Command-Line Release](../marshal-cli/release-artifacts.md#the-last-command-line-release) identifies the fixed historical tree, which includes M1,
so its `start` and `build` run axmc's pools in Tart VMs; `v0.1.0` predates M1 and
refuses `--profile axmc`. Runtime command procedures remain in that tree's copy
of the macOS host guide. In the current host procedures, `start` and `build` name the library's entry points of
those names, which the historical CLI runs as commands and which M2's Mac app is to
run.

`start` on the axmc profile runs the `os: "linux"` and `os: "macos"` pools of the
configuration through the library; see
[The Golden macOS VM and macOS Pools](./macos-vm.md#the-golden-macos-vm-and-macos-pools).
[Verification Status](./mac-verification.md#verification-status) says which procedures have run on a Mac.

## Installing Tart and Softnet

The runtime installs, bundles and repairs neither program. It calls `tart` as an
external program, and Tart runs the `softnet` that comes first on the `PATH` it
gets, which is the `PATH` of the process that runs the runtime, or, for the Mac
app's agent, the fixed `/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin`, since an
app opened from the Finder gets launchd's short `PATH`. The app's agent therefore
finds Tart and Softnet only when Homebrew installed them in `/opt/homebrew/bin`.
Both programs need
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

**A boot failure that names this section.** A boot failure's text names this
section when Tart's output carries `Softnet process terminated prematurely` or
`softnet not found in PATH`. Tart could not find `softnet` on `PATH`, which
[Installing Tart and Softnet](#installing-tart-and-softnet) covers, or Softnet's
process failed or ended early. The text keeps Softnet's and Tart's own error, such
as `failed to initialize proxy: failed to create vmnet network ...`. A slot deletes
the VM and tries again on its backoff; a `build` fails instead, so run it again.
When the failure repeats, check this section's setup and the installed Softnet
version.

**What the network allows.** Every VM boots with the two rules in
[The Golden VM Contract](./linux-vm.md#the-golden-vm-contract), on top of Softnet's defaults. A
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

[Security](../security/vm-isolation.md#softnet-confines-a-job-vms-network) records the
reasoning, and the accepted surface that remains.

## The Signed-In Session

`start` and `build` on the Mac need the signed-in graphical session of the user who
owns the runners; the last command-line release ran them from a terminal in it. Locking the screen
afterwards does not stop the runners. SSH and headless use are out of scope.

Since macOS 15, Apple's Virtualization framework starts a VM only while the user's
login keychain is unlocked. Without it, Tart's documentation lists errors such as
`SecKeyCreateRandomKey_ios failed`, `Failed to generate keypair` and
`Interaction is not allowed with the Security Server`. A boot failure whose `tart run`
printed one of them names this section. These texts may reach only the system log:
in the one public report,
[cirruslabs/tart#1067](https://github.com/cirruslabs/tart/issues/1067), `tart run`
printed only Virtualization's generic `The virtual machine failed to start.`, which
has other causes too, and such a boot failure names no cause. To check a boot
failure for a locked keychain, search the system log, widening `--last` to cover the
time of the logged failure:

```bash
log show --last 10m --predicate 'process == "tart" OR process CONTAINS "Virtualization"' \
  | grep -E 'SecKeyCreateRandomKey_ios|Failed to generate keypair|Interaction is not allowed'
```

No host run has tried this command yet. The slot deletes the failed VM and tries
again on its backoff, so unlocking the keychain is enough. FileVault rules out
automatic login, so after a restart, sign in at the screen and then start the
runners again.
