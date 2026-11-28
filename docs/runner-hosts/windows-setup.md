---
type: Procedure
title: "Prepare a Windows runner host"
description: "Install Docker Desktop and Marshal, check the release available for axpc, and confirm the machine and network prerequisites."
---

# Prepare a Windows runner host

Marshal runs ephemeral GitHub Actions runners for one or more repositories on a Windows machine with Docker Desktop. Set up the host here, then [register its pools](./registration.md#the-fine-grained-token), [configure them](./windows-pools.md#the-host-configuration), [build the image](./windows-image.md#building-the-runner-image) and [start the runners](./windows-running.md#running-the-host). Each consumer repository owns what its jobs install and build. The image is defined under
[`images/actions-runner/`](../../images/actions-runner/README.md), and the example
host configuration, with the reference for every field, is under
[`hosts/windows-docker-desktop/`](../../hosts/windows-docker-desktop/README.md).
[App](../marshal/index.md) owns what the app does, [CLI](../marshal-cli/index.md) the library its agent runs,
and the trust rules the host is reviewed against are in
[Security](../security/index.md).

**The command line.** Since W1's installer pull request there is no command line: a
release carries Marshal and its per-user MSI, and the app's menu and window are how a
host is run. The axpc host still runs its pools with the last command-line release,
`marshal`, until it moves to the app. The command line's installation procedures are
in the guide at the last command-line release's tag, and the program tree that
release identifies holds its commands, token files and `--config` path, as
[The Last Command-Line Release](../marshal-cli/release-artifacts.md#the-last-command-line-release)
says. A host moves from it to the app with that release's uninstall procedure, then
[Installing Marshal](#installing-marshal). The host evidence that names a command
was made with the command line; the app shares its runtime, image check and
diagnostics, and its own run on axpc is recorded on
[#73](https://github.com/axross/runners/issues/73).

## Prerequisites

- **Windows 11 Pro, version 23H2 or later**, the minimum for Docker Desktop's WSL 2
  backend on Windows 11 (see
  [Docker's system requirements](https://docs.docker.com/desktop/setup/install/windows-install/)),
  with hardware virtualization enabled and the WSL 2 feature on.
- **The machine stays powered on, signed in, and online.** Locked is fine; signed
  out is not. Marshal runs as the signed-in user, from its notification-area icon.
  It starts at sign-in only while `start_at_login` is on in `config.toml`'s
  `[lifecycle]`; otherwise, after a reboot or a sign-out the pools stay down until you
  start Marshal from the Start menu again. A job whose labels match no runner
  waits in the queue until GitHub cancels it.
- **Docker Desktop** with the WSL 2 backend. This procedure does not use the
  Hyper-V backend or a Docker Engine inside a WSL distribution.
- **GitHub CLI (`gh`) 2.102 or later, signed in** (`gh auth status`), which the
  optional check of the installer uses.
- **Windows PowerShell 5.1** (shipped with Windows) or PowerShell 7, for the
  diagnostics sink's access control step and the install procedure.

## Installing Docker Desktop

1. Install Docker Desktop and accept the WSL 2 backend.
2. Under **Settings, General**, enable **Start Docker Desktop when you sign in to
   your computer**, which is off by default. Marshal's runners wait for the Docker
   daemon, trying every 5 s, so it does not matter which of the two starts first.
   Build image does not wait: Docker must answer within 30 s.
3. Confirm `docker info` exits 0 in a terminal.

With the WSL 2 backend, containers share the WSL 2 virtual machine's CPU and
memory, and Docker Desktop's **Settings, Resources** does not cap them. To cap
them, set `memory` and `processors` under `[wsl2]` in
`%UserProfile%\.wslconfig`, which applies to every WSL 2 distribution on the
machine, then run `wsl --shutdown` so the virtual machine restarts with the
limits. Start from the defaults, and lower an entry's `slots` if builds
starve when several jobs run together. A job that finds no free slot queues
rather than fails.

The per-container `cpus` and `memoryGb` limits of the host configuration bind
inside this virtual machine and reserve nothing in it. Slots times the limits,
summed over every entry, can exceed the virtual machine's allocation, and the
jobs then compete for what it has. CPU IDs come from the actual Docker Linux
environment, not a Windows CPU count or a configured WSL processor count. A slot
whose ceiling-based affinity cardinality exceeds that measured availability
fails before requesting registration and retries after its backoff. Docker can
also reject a quota or CPU set at launch. Neither path starts an unrestricted
runner. See [Automatic CPU Affinity](./windows-pools.md#automatic-cpu-affinity).

## Network Access

Run this host only on a network you would trust the listed repositories'
workflows with. The [LAN egress policy](../security/pool-isolation.md#lan-egress-from-runner-containers-accepted-risk) records the accepted risk.

The machine needs outbound HTTPS to `github.com` and `api.github.com`,
`*.actions.githubusercontent.com`, `ghcr.io` and its blob storage (the base image
pull), and whatever each repository's jobs reach on a GitHub-hosted runner. The
image build, whenever Marshal builds the image, also reaches the Ubuntu package archives
that the base image's apt sources name, over port 80 or 443 (by default
`archive.ubuntu.com` and `security.ubuntu.com`; check the base image's sources
before filtering). It also needs HTTPS to `download.docker.com` for the Docker
client's signing key and its apt package, which every build installs. Linux x64 tool
provisioning also needs HTTPS to
`dl.google.com` for the checksum-pinned Android SDK/NDK/CMake archives,
`github.com` and `release-assets.githubusercontent.com` for Temurin, and
`services.gradle.org` for the Gradle distribution, including its redirects to
GitHub release-asset storage. The Dockerfile itself needs no access to Docker
Hub. No inbound port is needed.

## Installing Marshal

Install Marshal from a release by opening its MSI, with the Windows procedure in
[CLI](../marshal-cli/install-on-windows.md#installing-updating-and-rolling-back), never by building it on the
host. Download `marshal-<version>-windows-x64.msi` from the release page. The last
command-line release's MSI has the same name pattern but is the command-line product,
not the app: it installs `marshal.exe` and adds its directory to the user `Path`. Install
the app from a release after it. If you choose, run the optional check block on the
file first; that block refuses a file whose
checksum, commit on main, or attestation from `release.yaml` on main does not
verify, or whose `marshal-app.exe` does not match the release's `SHA256SUMS` line
for the app, and installs nothing. On a host that runs the command line, first
uninstall it with the last command-line release's procedure, as
[The Last Command-Line Release](../marshal-cli/release-artifacts.md#the-last-command-line-release) describes;
otherwise quit a running Marshal. Then open the MSI. It installs
`%LOCALAPPDATA%\Programs\marshal\marshal-app.exe` and its licence notices for your
user with no administrator rights, adds a Start menu entry named Marshal, adds
nothing to `Path`, and lists `Marshal` with the publisher `axross` under
**Settings, Apps**. Start Marshal from that entry.

Uninstall from **Settings, Apps** or with the uninstall block, which keeps the
configuration, the stored tokens, any old token files, the logs under
`%LOCALAPPDATA%\marshal\logs\` and the web view's data folder. Remove the tokens in
the window and quit Marshal first, as that procedure says. The MSI, the check block
and the uninstall block have not run on axpc; CI runs them against the release dry
run's MSI in its `Install Blocks` job.

The same procedure updates Marshal with a newer MSI and rolls it back by
uninstalling and installing the older one, as [Rolling Back](./windows-recovery.md#rolling-back) describes; quit Marshal first. After an update, rebuild the image and start again,
as [Updating the Runner](./windows-image.md#updating-the-runner) describes. Nothing on the host needs
a checkout of this repository: the build uses the image definition embedded in the
app. The MSI is not code-signed, so SmartScreen may show "Windows protected your
PC" for a browser download, which you pass with "More info", then "Run anyway", and
Smart App Control, when On, may block it; both are expected, not yet observed. On
axpc, a command-line binary fetched with `gh release download` had no
`Zone.Identifier`, so no Mark of the Web
([#56](https://github.com/axross/runners/issues/56#issuecomment-6511048211)), and
SmartScreen did not prompt at the install or at its first run from the terminal,
with "Check apps and files" On and Smart App Control Off
([#56](https://github.com/axross/runners/issues/56#issuecomment-6511120441)). A
copy that carries a Mark of the Web, such as a browser download, or a host with
Smart App Control On, could be blocked.

## What has run on axpc

CI builds the image and runs a smoke test on it for every
pull request, on linux/amd64 and on linux/arm64, and runs the CLI's tests on GitHub-hosted Linux, Windows, and macOS
runners. The [runtime test coverage](../marshal-cli/pool-runtime.md#what-the-tests-cover) identifies the checks for the backend and slot lifecycle. The [configuration tests](../marshal-cli/configuration-fixtures.md#fixtures-and-tests) apply the validation rules to the fixtures in
[`tests/host-configuration/`](../../tests/host-configuration). The checks that
used Docker Desktop, `icacls`, and a released `marshal` on a real machine were
described from the CLI's specification and the tools' documentation, and most were
then checked on axpc ([#56](https://github.com/axross/runners/issues/56)). Most of
those checks rest on the maintainer's report on #56 rather than on posted output,
and the individual procedures identify those observations. Verified on axpc, with the
command line:

- installing `v0.1.0` from its bare binary with the manual procedure (maintainer's
  report,
  [#56](https://github.com/axross/runners/issues/56#issuecomment-6511344317)), with
  no Mark of the Web on it and no SmartScreen prompt while Smart App Control is Off
  ([#56](https://github.com/axross/runners/issues/56#issuecomment-6511048211),
  [#56](https://github.com/axross/runners/issues/56#issuecomment-6511120441));
- a real registration with GitHub, a full `marshal build` of the image on Docker
  Desktop, and every pool running with its labels (maintainer's report,
  [#56](https://github.com/axross/runners/issues/56#issuecomment-6511344317));
- jobs running, `marshal restart`, one and a second Ctrl+C in the `start` window,
  closing the window, and a diagnostic job and its export on Docker Desktop
  (maintainer's report,
  [#56](https://github.com/axross/runners/issues/56#issuecomment-6511420845));
- `marshal stop` ending the instance in about 4 s with a job running, exit 0
  ([#56](https://github.com/axross/runners/issues/56#issuecomment-6511338525)).

Not yet verified on axpc, each for the reason given:

- Marshal's MSI install, the optional check block with its extraction, the
  uninstall block, and the prompts an unsigned MSI from a browser download raises,
  which have not run on axpc. CI runs the blocks against the release dry run's MSI in
  its `Install Blocks` job; the verification on
  [#73](https://github.com/axross/runners/issues/73) records the rest, and until
  then this document describes the prompts as expected;
- the command line's MSI install, migration block and check block, and its
  bare-binary block's refusals, at the last command-line release's tag, which were
  not run on axpc
  ([#56](https://github.com/axross/runners/issues/56)); CI ran them against
  stand-ins until W1's installer pull request;
- rolling back to a previous release, which no host has done;
- the command line's token-file access control step's result, whose `icacls`
  evidence was not collected;
- with the command line, Ctrl+Break in the `start` window, which could not be tested,
  and that a second Ctrl+C keeps the diagnostic container and the timing of the
  diagnostic containers' kill when the window closes, which the report did not give;
- the Job Object against Docker Desktop, after a hard kill of the command line's
  `start` or of Marshal's agent, which was not run, by the maintainer's decision;
- the instance lock refusing a second Windows user, and the control pipe rejecting a
  client on another machine, which were not run, by the maintainer's decision;
- the host procedures with the app, which the verification on
  [#73](https://github.com/axross/runners/issues/73) records.
