# Windows Runner Host

How to run ephemeral GitHub Actions runners for several repositories from one
Windows machine with Docker Desktop, with the Windows app, Marshal, from an
unconfigured machine to a healthy host: prerequisites, installing Marshal, the
per-repository token, the host configuration, building the image, running and
stopping the pools, health checks, rebuilding and updating, network access, recovery,
and rolling back to the previous release. It covers the machine and the container
that run a job; what a job installs and builds is each consumer repository's own
concern. The image is defined under
[`images/actions-runner/`](../../images/actions-runner/README.md), and the example
host configuration, with the reference for every field, is under
[`hosts/windows-docker-desktop/`](../../hosts/windows-docker-desktop/README.md).
[App](./app.md) owns what the app does, [CLI](./cli.md) the library its agent runs,
and the trust rules the host is reviewed against are in
[Security](../conventions/security.md).

**The command line.** Since W1's installer pull request there is no command line: a
release carries Marshal and its per-user MSI, and the app's menu and window are how a
host is run. The axpc host ran its pools with the command line, `marshal`, until then.
Its procedures, with their commands, token files and `--config` path, are at the
[last command-line release](./cli.md#the-last-command-line-release)'s tag, and a host
moves from it to the app with that release's uninstall procedure, then
[Installing Marshal](#installing-marshal). The verification below that names a command
was made with the command line; the app shares its runtime, image check and
diagnostics, and its own run on axpc is recorded on
[#73](https://github.com/axross/runners/issues/73).

**Verification status.** CI builds the image and runs a smoke test on it for every
pull request, on linux/amd64 and on linux/arm64, and runs the CLI's tests on GitHub-hosted Linux, Windows, and macOS
runners; [CLI](./cli.md) says what each covers. The CLI's tests check the
configuration rules against the fixtures in
[`tests/host-configuration/`](../../tests/host-configuration). The steps below that
used Docker Desktop, `icacls`, and a released `marshal` on a real machine were
described from the CLI's specification and the tools' documentation, and most were
then checked on axpc ([#56](https://github.com/axross/runners/issues/56)). Most of
those checks rest on the maintainer's report on #56 rather than on posted output,
and each place below that depends on one says which. Verified on axpc, with the
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
  ([#56](https://github.com/axross/runners/issues/56)); CI runs them against
  stand-ins;
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
- every procedure below with the app, which the verification on
  [#73](https://github.com/axross/runners/issues/73) records.

## Prerequisites

- **Windows 11 Pro, version 23H2 or later**, the minimum for Docker Desktop's WSL 2
  backend on Windows 11 (see
  [Docker's system requirements](https://docs.docker.com/desktop/setup/install/windows-install/)),
  with hardware virtualization enabled and the WSL 2 feature on.
- **The machine stays powered on, signed in, and online.** Locked is fine; signed
  out is not. Marshal runs as the signed-in user, from its notification-area icon,
  and nothing starts it at sign-in yet, so after a reboot or a sign-out the pools
  stay down until you start Marshal from the Start menu again. A job whose labels match no runner
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
runner; see Automatic CPU Affinity below.

## Repository Settings (Set by Hand)

For each target repository, under **Settings, Actions, General, Fork pull
request workflows**, choose the option that requires approval for workflow runs
from outside collaborators. Do not list a public repository whose fork pull
requests run on these labels until that is set. The requirement and its reason
are in the Per-Repository Isolation on a Runner Host section of
[Security](../conventions/security.md). A job runs on this host when every label
in its `runs-on` is one the registration carries, so `runs-on: axpc` selects it,
and so does `runs-on: self-hosted`.

## Installing Marshal

Install Marshal from a release by opening its MSI, with the Windows procedure in
[CLI](./cli.md#installing-updating-and-rolling-back), never by building it on the
host. Download `marshal-<version>-windows-x64.msi` from the release page and, if you
choose, run the optional check block on it first; that block refuses a file whose
checksum, commit on main, or attestation from `release.yaml` on main does not
verify, or whose `marshal-app.exe` does not match the release's `SHA256SUMS` line
for the app, and installs nothing. On a host that runs the command line, first
uninstall it with the last command-line release's procedure, as
[The Last Command-Line Release](./cli.md#the-last-command-line-release) describes;
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
uninstalling and installing the older one, as [Rolling Back](#rolling-back)
describes; quit Marshal first. After an update, rebuild the image and start again,
as [Updating the Runner](#updating-the-runner) describes. Nothing on the host needs
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

## The Fine-Grained Token

Marshal asks GitHub for a single-use runner registration through
[`POST /repos/{owner}/{repo}/actions/runners/generate-jitconfig`](https://docs.github.com/en/rest/actions/self-hosted-runners),
which needs the **Administration** repository permission set to **Read and
write**. Create one **fine-grained personal access token per target repository**,
under **Settings, Developer settings, Fine-grained personal access tokens**:

- **Repository access:** only that one repository.
- **Permissions:** Administration, read and write. Actions, read, is optional: with
  it the agent can name the job a busy runner runs, at the cost the token permission
  section of [Security](../conventions/security.md#a-token-carries-only-its-pool-kinds-permissions)
  names. Nothing else.

Enter each token in Marshal's window: **Open Marshal**, the **Tokens** tab, then
**Set Token** on the repository's row, paste the token and **Save**. Marshal keeps it
in the Windows Credential Manager, one entry per repository, and never shows it
again; the pools of that repository share it. The token rules are in
[The Agent Keeps Tokens in the Operating System's Store](../conventions/security.md#the-agent-keeps-tokens-in-the-operating-systems-store)
and the Per-Repository Isolation on a Runner Host section of
[Security](../conventions/security.md).

The agent reads the token from the store at every registration, so a rotated token
applies to the next job in every slot with no restart. Rotate a token with
**Replace Token** on its row, and remove one with **Remove Token**.

### Adding Actions: read for Job Names

Marshal's window names the job each busy runner runs, and links to it, only when the
repository's token also has **Actions**, **Read-only**. Without it the window still
shows the runner as running a job, and the pool says "Job names hidden". To add it, on
GitHub open **Settings, Developer settings, Fine-grained personal access tokens**,
choose the repository's token, **Edit**, and under **Repository permissions** set
**Actions** to **Read-only**, then **Update**. Editing a token's permissions keeps its
value, so nothing changes on this machine: the token stays where it is stored, and the
next job's name shows within a minute of a runner becoming busy, without a restart.

Actions: read also lets anyone holding the token read the repository's workflow runs,
their logs and their artifacts; the token permission section of
[Security](../conventions/security.md#a-token-carries-only-its-pool-kinds-permissions)
names that cost. Leave it off to keep the token to Administration alone.

### Token Files of the Command Line

The command line read each token from a file at the pool's `tokenPath`, which its
own access control step restricted. Marshal reads no token file and imports none:
enter each repository's token in the window once, as above. Keep the old token files
until the app's runners work, because rolling back to the command line reads them,
then delete them. Their procedure is at the last command-line release's tag.

## The Host Configuration

Marshal reads its configuration from `%LOCALAPPDATA%\marshal\config.toml`, which its
first start migrates from a `config.json` in that folder, the command line's JSON
form. Start from
[`config.example.toml`](../../hosts/windows-docker-desktop/config.example.toml), and
see the Host configuration section of the
[host README](../../hosts/windows-docker-desktop/README.md) for every field, which
names each by its JSON key, and [Schema Version 1](./cli.md#schema-version-1) for
the TOML key each JSON key becomes, such as `runners` for `slots` and `memory_gb` for
`memoryGb`; [The Agent's Configuration](./cli.md#the-agents-configuration) says how
the agent loads it. The rules below name the JSON keys. The rules that matter
operationally:

- **One entry per execution pool**, each with its own `name`, `slots`,
  `labels`, and `volumes`. Every entry uses the global `imageName`. The TOML form
  has no token key: the pool's token is the repository's entry in the Credential
  Manager.
- **Unknown fields are rejected, not ignored.** A misspelled field such as `label`
  for `labels` fails validation naming the field, so a typo cannot silently drop
  a setting.
- **Labels select the pool.** Missing or empty `labels` default to `axpc`;
  a non-empty list replaces that default while retaining `self-hosted`, `linux`,
  and `x64`. Before rollout, add `axpc` to each existing non-empty general
  custom-label list that lacks it; otherwise ordinary `axpc` jobs no longer
  match that entry. Label and token isolation rules are in the
  Per-Repository Isolation on a Runner Host
  section of [Security](../conventions/security.md).
- **Every entry has a `name`** that starts its container and runner names,
  `<name>-<index>-<timestamp>` with a 1-based slot index, and its volume names,
  `<name>-<suffix>`. It is lowercase letters, digits, and hyphens, starting with
  a letter or digit, and at most 64 characters, because it starts every runner
  name. GitHub documents no limit for runner names; 64 is this project's
  assumption. Choose it short and recognisable. A name that collides with another
  entry's is rejected, and the error names both. The collision rule is in the
  Per-Repository Isolation on a Runner Host section of
  [Security](../conventions/security.md).
- **One configuration per machine is the supported setup**, because the instance
  lock is machine-wide: one Marshal agent runs the pools on the machine, whichever
  user runs it.
- **Every job container has CPU and memory limits.** `cpus` is a number above 0
  and at most 64, 2 when absent. `memoryGb` is an integer from 1 to 256, 8 when
  absent. Each job container of the entry starts with `--cpus` and `--memory`
  set to them and `--memory-swap` equal to `--memory`, so it gets no swap beyond
  its memory. Docker takes `--cpus` as a CPU-time quota and `--memory` as
  a size with a unit suffix such as `g`; see
  [Docker's resource constraints](https://docs.docker.com/engine/containers/resource_constraints/).
  The limit applies to each container, not to an entry's slots together or to the
  host. The short container that resets volume ownership at startup has none.

Marshal checks the file when it starts and at every **Start all runners**, and calls
neither Docker nor GitHub to do it. A file that breaks a rule is not used: the window
names every problem, by its key and line, in its stopped banner when Marshal has no
configuration in use, and in a footer warning beside the last valid configuration
when it has one, which it keeps running on, as
[The Configuration File](./app.md#the-configuration-file) describes. Marshal does not
watch the file, so an edit applies at the next **Start all runners**: after every
edit, **Stop all runners**, then **Start all runners**, and read the window. It does
not check that a pool has a token; the Tokens tab shows which have none.

### Split a repository into general and Android pools

Keep the general entry's existing name, slots, resource limits, labels, mounts,
and diagnostic settings. If its `labels` list is non-empty, explicitly include
`axpc` alongside its existing aliases to preserve the effective registration.
Leave the global image unchanged. Add an entry for the same repository with a
distinct non-nested name, a dedicated label in `labels`,
and `runners = 1`. The two pools share the repository's token. For example, the
added pool can be:

```toml
[[pool]]
name = "example-android"
repository = "example-owner/example-repo"
labels = ["example-android-route"]
runners = 1
```

Use operator-local values, not these example names. Neither the image name nor
the entry name identifies a workload to Marshal. Set resource limits
only from approved host evidence; a dedicated slot does not reserve host memory.
No shared SDK, workspace, or new persistent cache is needed. Existing storage
choices follow the [shared-storage rules in Security](../conventions/security.md#shared-runner-storage-is-a-cache-poisoning-surface).
Diagnostics remain independent per entry, with private
retention keyed by its name even if two entries use the same private sink.

Deployment and workflow execution require separate scoped authorization:

1. Record the existing configuration, the window's pools, actual slots, image IDs
   and tagged rollback images. Do not rebuild the image during this change.
2. Establish a quiet maintenance window: finish affected jobs, including old
   workflow revisions still using the general label. Do not restart a busy
   instance; **Stop all runners** stops every entry's containers, not just Android's.
   Do not restart Docker or WSL for this change.
3. Apply the split configuration with **Stop all runners**, then
   **Start all runners**, only under the approved host-operation scope, and confirm
   that the window names no configuration problem. Verify the
   actual registrations: one Android slot, no `axpc` on Android, no dedicated
   label on general runners, and no extra dedicated-label registrations on
   this or another host. If runner-inventory access is unavailable, obtain
   operator evidence instead of expanding permissions. Inspect immutable image
   identities and enforced limits; a source test is not this readback.
   Routing alone needs no image rebuild; rebuilding remains a separately
   authorized operation.
4. Enable consumer routing only after the pool is verified. Change only the
   selected jobs' `runs-on` under that repository's approved plan. A missing
   dedicated runner leaves jobs queued; do not fall back to general runners.
5. With permission for named non-publishing executions, queue two Android
   requests in distinct existing concurrency groups. Verify that their actual
   execution intervals do not overlap and their existing artifact/alignment
   checks pass. Run general CI alongside Android on its unchanged slots;
   where at least two slots exist, verify two representative general jobs
   overlap. Do not change cancellation rules or execute release publication
   as a test. Stop after a failure and retain evidence rather than retrying
   unboundedly.
6. Record queue/build durations, effective labels/images/limits, actual host
   concurrency, diagnostic gaps and rollback in the approved evidence
   destination after human sanitization. Serialization success does not prove
   OOM, and missing counters are not zero. Host pressure can still affect one
   Android build beside general CI.
7. After the validation window, resume the routine under
   [Rebuilding the Image](#rebuilding-the-image-for-operating-system-updates).

For rollback, first finish affected jobs and obtain the required operation
scope. In a quiet window, choose **Stop all runners**. If the runners do not stop,
keep rollback pending rather than force an unsafe stop. Restore the saved
host configuration/images, keeping the explicit `axpc` migration in restored
general custom-label lists, because Marshal reads a non-empty list as a
replacement for `axpc`.
Restore consumer routing under its separate authorization while the runners
remain stopped, then choose **Start all runners**, confirm that the window names no
configuration problem, and read back actual
registrations, labels, slot counts and image IDs. Check that no old dedicated
job or extra dedicated-label registration bypasses the restored routing.
Preserve general capacity and private evidence; remove
only identified obsolete pool registrations/containers, never prune unrelated
storage. Do not rebuild until the intended image is restored. No host change is
authorized by this procedure alone.

## Automatic CPU Affinity

Every ordinary and diagnostic job container receives `--cpuset-cpus` alongside
its unchanged `--cpus` quota. This changes ordinary execution placement without
adding a JSON field or an opt-in. A quota of 1.5 keeps quota 1.5 and selects two
allowed logical CPUs: affinity cardinality is ceiling(`cpus`), an allocation
policy, not an equivalence between CPU time and CPU IDs. Quota alone does not
restrict scheduling to a particular set.

The configuration reader enumerates entries in file order and slots in index
order. Each slot advances a cumulative position by its cardinality. Before
every registration attempt, a short probe reads its own `Cpus_allowed_list` on
the slot's effective local image and Docker context. The probe has no CPU set,
network, registration, mounts, or image pull. The slot chooses consecutive IDs
from that sorted measured inventory, starting at position modulo inventory size
and wrapping. Sparse IDs are preserved, not replaced with a range from a host
count. With a stable common inventory, selections are disjoint when their total
cardinality fits and can overlap when overcommitted. Topology changes between
observations can also create overlap. No CPU is reserved or exclusive.

Discovery waits at most 30 seconds, and removing a leftover probe at most 5
seconds; [CLI](./cli.md#cpu-placement) gives the exact budgets. Each slot uses one
stable probe name, `<name>-cpu-probe-<index>`, and immutable marker
`runners.cpu-probe=1`. A retry or restart removes that exact marked probe before
creating another. Malformed/empty output, insufficient capacity, timeout, or
unconfirmed cleanup fails the attempt before JIT registration. An unavailable
daemon can prevent confirmed deletion; the deadline bounds waiting, not guaranteed
daemon cleanup. No broad prune or job-container deletion is used for probes.

Docker enforces the requested cpuset; before the runner or diagnostic
launcher starts, a Bash guard compares its kernel `Cpus_allowed_list` with the
selected canonical set. Missing/unreadable/mismatched readback exits 125 without
launching the runner or falling back unrestricted. Per-attempt discovery and
this check protect against stale topology after Docker/WSL changes. The short
volume-ownership helper has no new CPU policy. Labels, slots, images, memory,
swap, mounts, consumer tools/build flags, and diagnostic opt-ins remain their
own controls. Affinity does not guarantee compiler process counts, lower memory
use, or an OOM remedy; the Android failure cause remains unconfirmed.

The image smoke checks actual Docker `HostConfig`, parent/child process sets,
effective cgroup cpuset and quota, quota-only controls, distributed selections,
and refusal controls. `nproc` and `getconf` are recorded as separate processor
API observations, not worker counts. Missing topology/layout coverage fails as
incomplete evidence. Hosted Linux enforcement is not Docker Desktop/WSL proof.
The real-host check is a separately authorized post-merge follow-up on
[the affinity issue](https://github.com/axross/runners/issues/25), not fulfilled
by source tests. Before an authorized rollout, save private source/configuration
and image rollback material, establish a quiet maintenance window, and verify
all entries' requested/process/cgroup sets, quotas, diagnostics, capacity, and
topology-change behavior on the intended context. Publish only sanitized source
and image identities, settings/readbacks, gaps, and observed throughput/pressure
there. Do not publish tokens, full inspect output, or raw private diagnostics.

There is no configuration switch to disable affinity, and every release applies
it, so there is no supported way back to placement without affinity. The
PowerShell scripts that once offered one remain only in this repository's history,
unsupported, and a fault in affinity is fixed forward in a new release. The feature
changes no WSL allocation and no Windows process-affinity setting. A failed runtime
readback blocks rollout, not independent pool work.

## Moving to the New Configuration Format

A configuration that still has `hostPrefix` or a per-entry `prefix` is rejected,
naming the field, including the `config.json` Marshal migrates at its first start.
To move it:

1. Note the `hostPrefix`, and each entry's prefix, `<hostPrefix>-<owner>-<repository>`
   in lowercase unless the entry set `prefix`. Quit Marshal if it runs.
2. Rewrite the file: give every entry a `name`, and delete `hostPrefix` and every
   `prefix`. An entry whose `name` equals its previous prefix, which is possible
   only when the prefix is a valid `name`, keeps its volumes; any other `name`
   starts from empty volumes.
3. Remove what the earlier names left, skipping an entry whose `name` equals its
   previous prefix. Remove a volume only once no container uses it:

   ```powershell
   docker ps -a --filter "name=<previous prefix>-"
   docker rm -f <container name>
   docker volume rm <previous prefix>-<suffix>
   ```

4. Continue from [Installing Marshal](#installing-marshal) on a host without it, or
   start Marshal on one that has it, and confirm that the window names no
   configuration problem.

Job containers had no CPU or memory limit before. Without `cpus` and `memoryGb`,
each is now capped at 2 CPUs and 8 GB, so set both on an entry whose jobs need
more.

## Building the Runner Image

Build the image once at install, after every update of Marshal, and at each routine
rebuild. Building is refused while runners run, so from Marshal's menu choose
**Stop all runners**, then **Build image**, and when the build has finished,
**Start all runners**. A build needs a configuration in use; it uses that
configuration's image name and reads no file.

The build builds the image definition embedded in the installed
Marshal, under the configured global `imageName`, with
`docker build --pull --no-cache`, and reads nothing from a checkout or the working
directory. The runner version and base image digest come from that definition, so
a newer runner needs a newer release; see [Updating the Runner](#updating-the-runner).
The cache is ignored so that the operating system packages are installed again and
pick up their updates. The build goes to a temporary tag, and only a successful
build is tagged as the configured name; a failed build leaves the previous image in
place. After a successful build `build` removes the earlier images it built that the new
build left untagged: the prune is limited to images carrying the label `com.github.axross.runners.image=actions-runner`,
so it does not touch other dangling images or images built by hand. It does not
reclaim installation stages, build caches, or tagged rollback images. A failed prune
is a warning, not a failed build. [CLI](./cli.md#build) gives every step, and
[The Agent's Configuration](./cli.md#the-agents-configuration) what the agent's build
logs.

While it builds, the window says the image is building, and **Stop build** or
**Stop all runners** aborts it within 30 s; an abort before the retag leaves the
configured name on its previous image. CI runs the library's build only against a
small test definition on Linux; a full build of the image on Docker Desktop was
verified on axpc with the command line's `build` (maintainer's report,
[#56](https://github.com/axross/runners/issues/56#issuecomment-6511344317)).

The image carries a Docker client at a pinned version, which `build` installs from
Docker's apt repository after checking its signing key. The Docker runtime mounts no
Docker socket into a job container, so the client reaches no engine here; the image
has it for the axmc host, whose job VMs mount their own. The image contract version
is 2 with it, so an image built before the release that carries contract 2 is refused
until the image is built once; [CLI](./cli.md#the-image-contract) has the check.

The Android NDK contract, Java/SDK/Gradle inputs, and Build sections of the
[image README](../../images/actions-runner/README.md) own version/checksum
maintenance, the consumer fallback contract, and additional image-build costs.

For an authorized image update, rollback, or fixed-image comparison:

1. Pause the routine rebuild, and do not rebuild by hand during the window.
2. Record the stable current image ID with
   `docker image inspect <configured-image-name> --format '{{.Id}}'`. Before an
   update, retain it with `docker tag <saved-image-id> actions-runner:rollback`;
   this tag keeps it out of the dangling-image prune.
3. Build the approved new image with the release that embeds it, or roll
   back by tagging the saved image ID as the configured image name again. Record
   the image ID actually used. Existing containers finish on their original image;
   subsequent containers use the retagged or rebuilt one. The image check before
   each job accepts the retagged image only when its contract label matches the
   installed Marshal; see [CLI](./cli.md#the-image-check).
4. A retag alone lasts only until the next image build, which builds the
   definition the installed release embeds. Install the release intended for
   subsequent builds before resuming the routine rebuild.
5. Remove the rollback tag with `docker image rm actions-runner:rollback` only
   after the update is accepted and that recovery image is no longer needed;
   this is not a general image/cache prune.

For build-cache reclamation, inventory `docker system df -v` and identify the
builder holding the installation stages first. Obtain separate approval for
its unused-cache operation. For the daemon builder, an age-filtered option is
`docker builder prune --all --filter until=168h`; it is not image-label-scoped
and can invalidate other projects' build caches. This command's syntax is
checked locally, but its real-host behavior and any automatic garbage-collection
limit are unverified; do not assume the image prune bounds that storage.

No registration or cache-volume change is needed. Rollout and rollback are
separately authorized operator actions, not effects of an image-source pull
request.

## Running the Host

Start Marshal from the Start menu entry Marshal. A launch starts its agent, which runs
the pools, and the notification-area icon shows their state; the agent keeps running
when the window or the app closes. The agent checks the configuration as
[The Host Configuration](#the-host-configuration) says, writes its log under
`%LOCALAPPDATA%\marshal\logs\`, waits for Docker, checks the image, prepares every
pool, and then runs every slot until it is stopped. With a configuration that cannot
be used, an image that cannot run jobs, or no pool able to start, it stays running
and stopped, and the window names the problem. Nothing starts Marshal at sign-in yet:
after a reboot or a sign-out the pools stay down until you start it again.

From the notification-area icon's menu:

| Item                  | What it does                                                                                                                      |
| --------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| **Open Marshal**      | Opens the window: the agent's state, each pool's state, runners and diagnostics, the image and its freshness warnings, and tokens |
| **Start all runners** | Reads the configuration file again, checks the image, and runs the pools; use it after editing the configuration                  |
| **Stop all runners**  | Stops every runner with the stop sequence, within 30 s, and leaves the agent running and stopped                                  |
| **Build image**       | Builds the runner image while the runners are stopped; see [Building the Runner Image](#building-the-runner-image)                |
| **Quit Marshal**      | Stops every runner as Stop all runners does, cancelling a build first, then ends the agent and the app                            |

[App](./app.md) says what each item and the window do, and when an item is greyed.
The agent's states, waits and reasons are in [Agent State](./cli.md#agent-state).

Stop the host with **Stop all runners**, or with **Quit Marshal**. A stop gives the
ordinary containers their grace period, and finalises each diagnostic container, as
[Collecting private diagnostics](#collecting-private-diagnostics) describes. Closing
the window, or ending the app, leaves the agent and its runners running, and the next
launch reconnects to them. A sign-out or shutdown gives the agent no event it can act
on, so its containers wait for the next start, which removes them. If Marshal does
not answer, end `marshal-app.exe` in Task Manager; the next start removes the
containers it left.

However the agent ends, even from Task Manager, the Job Object it runs in ends every
`docker` client it started. Docker Desktop keeps a container running after its client
is killed, so a job can run on until the next start removes its container. The Job
Object against Docker Desktop is not yet verified on axpc
([#56](https://github.com/axross/runners/issues/56#issuecomment-6511420845)): the
hard-kill check was not run, by the maintainer's decision.

One agent runs per machine. While it runs, another Windows user's Marshal is refused,
and its window says that another Marshal process holds the runner lock; that refusal
is not yet verified on axpc
([#56](https://github.com/axross/runners/issues/56#issuecomment-6511420845)): the
second-user check was not run, by the maintainer's decision. The app reaches the agent
through a control pipe that only its own user can open and that rejects remote
clients; the rejection of a client on another machine is not yet verified on axpc
([#56](https://github.com/axross/runners/issues/56#issuecomment-6511420845)): the
remote check was not run, by the maintainer's decision.
[Security](../conventions/security.md#one-instance-per-machine-reached-only-by-its-user)
records what this protects.

## How a Container Picks Up a Job

For each execution pool, the agent runs that entry's `slots` as separate loops. A
sibling pool for the same repository has its own slots. Each slot follows this
cycle:

1. Check the image's contract, then remove the slot's leftover probe and
   discover/select current allowed CPUs before registration. A failure backs off
   without registering a runner.
2. Request a single-use registration from GitHub with that repository's token, for
   a uniquely named runner carrying the entry's labels. A real registration with
   GitHub is verified on axpc, each pool registering its platform and configured
   labels (maintainer's report,
   [#56](https://github.com/axross/runners/issues/56#issuecomment-6511344317)).
3. Start a throwaway container from the image with the entry's volumes and
   the affinity guard followed by `/home/runner/run.sh`. The registration reaches the runner through the
   `ACTIONS_RUNNER_INPUT_JITCONFIG` environment variable, which the runner reads
   at start in the pinned version (`CommandSettings.cs` in `actions/runner`).
   [Security](../conventions/security.md) owns how the agent hands it to
   Docker. Anyone with access to the local Docker daemon can still read it from
   `docker inspect` until the container is removed, and the registration is
   single-use.
4. The runner takes one matching job and exits. Normally `--rm` removes the
   container. A diagnostic entry's container exports its bounded evidence first,
   then is removed, also when the export fails.
   The slot then returns to step 1. Container exit is not the GitHub job result;
   a runner can exit successfully after reporting a failed consumer job.

A failed image check, CPU discovery, registration request or container that exits
non-zero delays the next attempt, doubling from 5 seconds to a 300-second ceiling;
a container that ran a job to completion is replaced at once. When the part of
the agent that runs the pools fails as a whole, it is started again after 10 s, 30 s,
and 60 s; a fourth failure within 10 minutes stops the runners, with the restart
limit as the reason the window shows. [CLI](./cli.md#names-and-the-slot-loop)
gives the slot loop and its backoff, and [CLI](./cli.md#instance-state) the
restarts.

## Collecting private diagnostics

Diagnostics are off by default. Enable them only for the entry whose evidence
you need, after separate authorization to operate that host. The default
example enables neither diagnostics nor raw records. No resource allocation,
build command, tool version, registration or mount changes with this opt-in.
The sink is not exposed to the job as a bind mount or volume.

Collect private evidence as follows:

1. Pre-create a directory on a local Windows filesystem, outside any checkout of
   this repository.
   Restrict its owner and all allowed access rules to the account that runs
   Marshal. Marshal MUST NOT create the sink, relax permissions, or
   repair its access controls, and on Windows it does not check them, so this step
   is the only protection the evidence has. For a new, empty example directory,
   review the path before running these commands as that account:

   ```powershell
   $sink = 'C:\example-evidence'
   New-Item -ItemType Directory -Path $sink
   $sid = [Security.Principal.WindowsIdentity]::GetCurrent().User
   $acl = New-Object Security.AccessControl.DirectorySecurity
   $acl.SetOwner($sid)
   $acl.SetAccessRuleProtection($true, $false)
   $rule = New-Object Security.AccessControl.FileSystemAccessRule $sid, 'FullControl', 'ContainerInherit, ObjectInherit', 'None', 'Allow'
   $acl.AddAccessRule($rule)
   Set-Acl -LiteralPath $sink -AclObject $acl
   ```

2. Set `diagnostics` to `true` and `diagnostic_directory` to that directory in
   the target pool. Leave `diagnostic_raw_records` absent or `false` unless
   private daemon/crash retention is separately wanted. Set it to `true` only
   with diagnostics enabled. Apply the change with **Stop all runners**, then
   **Start all runners**, in an authorized quiet window, and confirm that the
   window names no configuration problem. Validation rejects a sink
   inside a checkout of this repository without Docker, GitHub or token reads, but does
   not check that the sink exists, is private, or is writable.
3. After a container stops, the agent exports its evidence into a new
   `<name>-bundle-<32 hexadecimal digits>` directory in the sink, then removes the
   container, also when the export fails. Inspect the bundle privately.
   `identity.txt` holds only the actual immutable image ID, CPU/memory/swap/affinity
   readbacks, container exit and Docker's OOM flag. `metrics.txt` holds the bounded
   observer output, and `raw-<n>.log` files the raw records when they are on;
   [the image README](../../images/actions-runner/README.md)
   owns fields, sampling limits, and the exact raw-record locations. A
   `complete.txt` marker, written last, means copying finished, not that every
   observation was available or that the incident's cause is known.
4. Treat any directory without `complete.txt` as incomplete. A missing
   `final=observed` metric marks unavailable final observations after abrupt
   termination or a collection bound. Review every `gap=` and `unavailable`
   field. Missing counters MUST NOT be read as zero. A counter delta is
   container-wide, not a kill attributed to a daemon. Docker's `OOMKilled=false`
   does not exclude a killed child. Correlate time samples and process PIDs with
   separately authorized host kernel/WSL evidence; the container cannot read
   enclosing-host kill records.
5. Keep all raw records private. Marshal never shows or logs them.
   Retained contents are untrusted operator evidence, MUST NOT become build
   inputs, and MUST NOT be published without human sanitization. Public
   summaries MUST omit credentials, registrations, private paths and
   machine-identifying values; an allowlisted metric file is not automatic
   permission to publish it.

The sink admits at most ten bundles per entry, counting every `<name>-bundle-`
entry, partials included, and at most 40 MiB of payload per bundle, less a small
reservation for the bundle's own files. A bundle inherits the sink's access rules.
The export reads only the collector's regular files, by name and within fixed
sizes, and refuses links, special files, unsafe paths, and duplicates; it never
copies a container's whole filesystem or its logs. At a job end the export ends by
18 s and the removal by 24 s after the container ended, and **Stop all runners**
finalises the diagnostic containers within its 30 s. A full, inaccessible or unsafe
sink, a failed or partial copy, a missing collector, or an unavailable final sample
is a diagnostic gap, logged as
`Diagnostic gap in pool <pool>, container <name>: <text>`, never the runner's
failure. A removal that fails leaves the container retained for the next
finalisation, at the next stop or start of the runners. The window's Diagnostics
section of each pool shows its bundles written, the room left of 10, and each
container it still keeps. [CLI](./cli.md#diagnostics) owns the timeline, the
exporter, and the startup finalisation.

Each diagnostic container carries only the immutable
`runners.diagnostic-lifecycle=1` label, not a registration or private path. When
the agent prepares a pool, it finds the labelled containers that an earlier run of
the pool left, such as those an ended agent or the command line kept. With the
pool's diagnostics on, it exports and removes each one with the pool's current
settings before any of the pool's slots registers. Ordinary unlabelled containers
keep their ordinary removal even when the entry now enables diagnostics. Prior
evidence is never automatically deleted. Operators MUST review and privately
archive or remove prior bundles before collecting beyond the quota. Retention
outside this feature is their decision. A diagnostic job and its export on Docker
Desktop were verified on axpc with the command line: the bundle was written with its
`complete.txt`, and its `diagnostics` reported the counts and exited 0 (maintainer's
report, [#56](https://github.com/axross/runners/issues/56#issuecomment-6511420845)).

### Recover before disabling diagnostics

Recover an entry's diagnostic containers before turning its diagnostics off, and do
not start another runner against the retained evidence. When an entry's
diagnostics are already off and labelled containers of it remain, the agent gives
them one bounded `docker stop`, warns for each, and refuses that entry, naming this
section; the other entries run, and the window names the entry's reason. The agent
never removes or exports them while the entry's diagnostics are off, and does not
silently turn collection on.

Recover them with Marshal alone, after separate host authorization:

1. **Check the sink first.** Confirm that the entry's sink exists and that its
   access control step under
   [Collecting private diagnostics](#collecting-private-diagnostics) was done. List
   the containers to recover, keeping only names that are the entry's `name`, a
   hyphen, a slot number, a hyphen, and 17 digits, and record those names:

   ```powershell
   docker ps -a --filter "name=<name>-" --filter "label=runners.diagnostic-lifecycle=1" --format "{{.Names}}"
   (Get-ChildItem -LiteralPath 'C:\path\to\private-evidence' -Filter '<name>-bundle-*').Count
   ```

   The second command counts the entry's bundles in the sink. Archive enough of
   them privately, outside the sink, that the count is below 10 minus the number of
   containers to recover. The export refuses a bundle once the entry has 10, and the
   entry's next jobs after step 2 take quota too, so the count leaves them room. The
   next step removes a container even when its export fails, so a sink that fills
   during the recovery loses that container's evidence.
   Never dump configuration or registration values.

2. **Recover.** Keep `diagnostics` on for that entry, or turn it on again with
   `diagnostic_directory` set to the sink from step 1. Then choose
   **Start all runners**, or, when the runners are already running,
   **Stop all runners** first, in an authorized quiet window, because a stop stops
   every entry's job containers. The entry's preparation adopts each leftover container, exports it, and
   removes it before any slot registers, and logs one line for each:
   `Diagnostic container <name> that an earlier run left: exported, removed.`, with
   `export gap` or `not removed` in place of either word. It removes the container
   even when the export fails, reporting an `export gap`; that is why step 1 comes
   first.
3. **Confirm.** First wait until the agent's log has step 2's line for every
   container step 1 recorded.
   Treat a line with `export gap` or `not removed` as not recovered: an
   `export gap` container is already removed, and its evidence is lost. Run step 1's
   `docker ps -a` listing again and confirm that none of the names step 1 recorded
   is still listed. Other names can appear: step 2 started the entry's slots with
   diagnostics on, so each live slot's job container has the same name pattern and
   label. Those are expected, and must not be removed. Then confirm that the
   entry's Diagnostics section in the window keeps no container. A container still
   kept, such as one whose removal failed, is finalised again at the next
   **Stop all runners**; or abandon it as in step 5.
4. **Then disable.** Choose **Stop all runners**, set `diagnostics` to `false`,
   remove `diagnostic_directory` and `diagnostic_raw_records`, then choose
   **Start all runners** and confirm that the window names no configuration
   problem.
5. **Abandon, only by choice.** If you choose to abandon a container's evidence,
   choose **Stop all runners**, then remove only that identified, labelled
   container with `docker rm -f <name>`, never with a prune, and start the runners
   again.

This procedure is source guidance, not authorization to operate a host.

The CLI's diagnostic tests run on Linux, macOS, and Windows hosted runners, and
CI's image smoke command exercises the image's lifecycle and export with synthetic
jobs, no production registration, networking or shared mounts; neither is Docker
Desktop/WSL runtime behavior. Required hosted PR image evidence is still needed
when a local machine has no Docker daemon. Real-host diagnostic overhead and
retention remain unmeasured until a separately authorized Windows check.

Diagnostics alone MUST NOT be described as fixing daemon disappearance or
establishing an OOM kill, JVM crash, actual slots or peak usage. Missing runtime
evidence remains unknown.
Real-host diagnosis, memory/slot/CPU-affinity comparisons selected independently
from measured pressure and host headroom, and ordinary setup-action/SDK-manager/
wrapper reuse verification are separately approved follow-ups. Do not infer a
resource remedy or tool-reuse correction from fixture results, and do not rerun
the consumer workflow, alter host settings, roll out an image or inject build
options under this diagnostic-source change.

## Cache Volumes

An entry's `volumes` are named Docker volumes that outlive the throwaway
containers, so a job finds the toolchains and downloads an earlier job left. That
makes each a shared writable surface, and [Security](../conventions/security.md)
owns the rules that govern it. Each is declared here with the reason it exists.
The suffixes are those of
[`config.example.toml`](../../hosts/windows-docker-desktop/config.example.toml);
the volume's name is the entry's `name` plus the suffix.

| Suffix      | Mount path             | Why it exists                                                                          |
| ----------- | ---------------------- | -------------------------------------------------------------------------------------- |
| `toolcache` | `/opt/hostedtoolcache` | Language runtimes that the `setup-*` actions install and `RUNNER_TOOL_CACHE` points at |
| `cargo`     | `/home/runner/.cargo`  | Cargo's registry downloads and the binaries it installs                                |
| `rustup`    | `/home/runner/.rustup` | Rust toolchains                                                                        |
| `npm`       | `/home/runner/.npm`    | npm's content-addressed package cache                                                  |

Any job routed to the entry's labels can write to every one of them, and no
other entry mounts them. When the agent prepares a pool, it creates the entry's
volumes and resets their ownership with a short root container that mounts only
those volumes.

An entry keeps only the volumes in the table above. The host does not enforce
that: it accepts any suffix and mount path, so the operator does. A volume
for anything else, such as a compiler or build-system cache whose contents a later
job links or executes without re-verifying them, is unsupported because no rule in
[Security](../conventions/security.md) allows it.
The Android SDK and `~/.android` MUST stay container-local, never cache-volume
mount points: a job must not replace the image's verified NDKs or leave SDK
metadata for a later job to trust.

Before listing volumes for a repository, check which of its workflows hold a
deployment secret, and whether the repository accepts pull requests from forks.
If either applies, the pool lists no `[[pool.volumes]]`, so each job starts from the
image alone, or those workflows, including the fork pull request workflows, run on
a GitHub-hosted runner. That includes `npm` and every other volume in the table,
not only the toolchain volumes. The host cannot check this; the operator does. The
rule and its reason are in the Shared Runner Storage Is a Cache-Poisoning Surface
section of [Security](../conventions/security.md).

## Removing an Entry

Remove the pool from the configuration, then choose **Stop all runners** and
**Start all runners**, and confirm that the window names no configuration problem. A volume outlives its entry: the entry's volumes and any stopped
containers stay until removed by hand with `docker volume rm` and `docker rm`. The
isolation rules the host enforces between repositories are in
[Security](../conventions/security.md).

## Health Checks

- **GitHub:** a healthy host shows, under each repository's **Settings, Actions,
  Runners**, the sum of its entries' `slots` runners, **Idle** when no job is queued and **Active** when
  one is running, because each slot registers a runner before it starts a
  container. Each registration carries exactly `self-hosted`, `linux`, `x64`, and
  the entry's labels, `axpc` by default. Fewer runners than that sum, and none in
  particular, mean Marshal's runners are not running or one or more pools are
  failing. Compare
  runner-name prefixes with each entry's `name` and `slots` to identify the affected
  pool; see Recovery.
- **Marshal:** the window shows the runners running, every pool running, and no
  image problem. A pool that did not start shows its reason. A footer warning about
  the image is a freshness warning; see
  [Rebuilding the Image](#rebuilding-the-image-for-operating-system-updates).
- **Machine:** `docker ps` shows `slots` running containers per entry, named
  `<name>-<index>-<timestamp>`, whether idle or busy, since an idle container is
  a runner waiting for a job. None means the same as above.
  `docker inspect` on a container shows its limits under `HostConfig`: `NanoCpus`
  is the CPU-time quota in billionths of a CPU, `CpusetCpus` is the requested
  CPU set, and `Memory` and `MemorySwap` are both `memoryGb` gigabytes in bytes.
  Requested settings are not effective readback. Each job container's output in the
  agent's log has the guard's bounded `CPU affinity verified` line, which reports
  the kernel-granted set; the authorized real-host check also resolves the actual
  cgroup cpuset/quota layout.
- **Tokens:** the Tokens tab shows a token set for every repository a pool names.
- **End to end:** for each pool, a workflow run whose `runs-on` matches that
  entry's routing label starts executing rather than sitting queued.

## Rebuilding the Image for Operating-System Updates

No task rebuilds the image, so the operating-system packages in it are only as
fresh as its last build. Rebuild it weekly, and also whenever the window or the
agent's log shows a build-age warning: from 7 days that the image should be rebuilt,
and from 21 days that its operating-system packages are that old. An image that
Marshal did not build, such as one built by CI or by hand, has an unknown build age
and a warning that says so. At a quiet time, choose **Stop all runners**, then
**Build image**, then **Start all runners**.

Building is refused while runners run, so a rebuild stops the pools for the length
of the build: a job in flight fails, and queued jobs wait. A failed build leaves the
previous image, and the runners run on it.

The runner-release-age warning,
`runner <v> was released <d> days ago, ...`, is different: a rebuild does not clear
it. The runner version comes from the image definition embedded in the installed
Marshal, so a rebuild with the same release builds the same runner. That warning
calls for a runner bump, below.

## Updating the Runner

A newer runner reaches the host only through a new release, because the app embeds
the image definition:

1. Merge the Dependabot pull request that bumps the `FROM` line in
   `images/actions-runner/Dockerfile`, with the edits it needs by hand; see
   [Development Workflow](./development-workflow.md#bumping-the-runner). Keep its
   `build(deps)` title when merging: it releases a patch, as
   [Version Rules](./development-workflow.md#version-rules) describes.
2. The maintainer dispatches `release.yaml` on main, as
   [Each Release](./development-workflow.md#each-release) describes.
3. Quit Marshal from its menu, which stops its runners, then install that release
   with the procedure in [CLI](./cli.md#installing-updating-and-rolling-back). A
   host that still runs the command line moves to the app first, as
   [Installing Marshal](#installing-marshal) describes.
4. Start Marshal, build the image from its menu, then start the runners. Containers
   already running when Marshal stopped them have ended; the next ones start on the
   new image.

Merge runner bumps promptly. GitHub's runner updates itself at start when a newer
release exists, which an ephemeral container repeats for every job when the image
is stale, and a runner with automatic updates disabled MUST be updated within 30
days of a new release; see
[GitHub's self-hosted runner reference](https://docs.github.com/en/actions/reference/runners/self-hosted-runners).
A current image avoids both.

## Network Access

Run this host only on a network you would trust the listed repositories'
workflows with; [Security](../conventions/security.md) states the accepted risk
behind that.

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

## Recovery

- **A container left by an unclean stop** (a closed window, a hard kill, a crash,
  a reboot mid-job) is handled when the agent next prepares its pool, using each
  entry's own name pattern and the immutable diagnostic label. Ordinary containers
  are removed before the slots start. Labelled containers are exported and removed
  while the entry's diagnostics are on; with them off they refuse the entry until
  [Recover before disabling diagnostics](#recover-before-disabling-diagnostics) is
  done. By hand, `docker ps -a --filter "name=<name>-"` lists candidates and
  `docker rm -f <name>` removes an ordinary one.
- **Docker Desktop restarts mid-job:** the container is gone, the job fails on
  GitHub's side, and re-running the workflow is the recovery. The agent keeps
  running: each slot's next attempt fails into its backoff while Docker does not
  answer, remeasures affinity, and resumes by itself when Docker is back.
- **The pools are down after a reboot or a sign-out:** nothing starts Marshal at
  sign-in yet. Start it from the Start menu once Docker Desktop is up, or straight
  away, since the runners wait for Docker.
- **Marshal does not answer, or a stop does not end:** the window says the agent is
  not answering, or keeps saying it is stopping. End `marshal-app.exe` in Task
  Manager; the next start removes the containers it left.
- **One pool's entry will not start:** a missing token, a
  volume that cannot be created, or stale containers that cannot be listed or that
  carry the diagnostic label with diagnostics off skip that entry only. The agent
  logs a warning naming the entry and the reason (never a token), the window shows
  the pool's reason, and the other startable pools run, including a sibling for the
  same repository. A pool waiting for its token starts once the token is set in the
  Tokens tab. Shared dependencies, such as that repository's token or the Docker
  daemon, can affect several pools. When no entry can start, the runners stay
  stopped with that reason. Fix the cause, then choose **Stop all runners** and
  **Start all runners** in an authorized quiet window; an entry skipped at start is
  not retried until the pools are prepared again.
- **A rejected configuration:** the window names every offending key and keeps the
  last valid configuration in use, or stays stopped when there is none. Fix the
  file, then choose **Start all runners**.
- **An image that cannot run jobs** (missing, or with another contract): the
  runners stay stopped, or stop, with that reason. Choose **Build image**, then
  **Start all runners**.
- **The restart limit:** a fourth failure of the pools' runtime within 10 minutes
  stops the runners, with the limit as the reason. Read the agent's log, fix the
  cause, then choose **Start all runners**.
- **A slot logging CPU discovery/cleanup failures:** registration is withheld.
  Check the intended Docker context/local image and available CPUs in a separately
  authorized maintenance window. Resolve the exact marked slot probe if deletion
  cannot be confirmed; never remove unrelated containers or prune broadly.
  A topology or readback mismatch must not be bypassed with unrestricted startup.
- **A slot logging registration failures:** the message carries GitHub's status. A
  401 or 404 usually means the token expired or lacks Administration access to that
  repository; replace it with **Replace Token** in the Tokens tab, as
  [The Fine-Grained Token](#the-fine-grained-token) describes. Other slots
  continue their own registration loops, but a shared-token problem can affect
  every pool for that repository.

- **The window says job names are hidden** while the token has Actions: read: the
  agent's log records GitHub's answer to the workflow-runs request, its status code,
  its message and its `X-Accepted-GitHub-Permissions` header. A 403 there means the
  token still lacks the permission for that repository; check the token's
  **Repository access** and **Actions** setting as
  [Adding Actions: read for Job Names](#adding-actions-read-for-job-names) describes.
  Marshal asks again when a runner next becomes busy, at most once a minute.
- **The window says GitHub status is unavailable:** the running agent is an older one
  without GitHub status, or, for now, has no room for another subscription and the
  window asks again every 60 s. Runner states on this computer are unaffected.

## Rolling Back

Roll back when a release misbehaves on the host. Rolling back means uninstalling the
installed release and installing the previous one: it needs an earlier release to go
back to, so a host on the first release has none, and a fault in it is fixed forward
in a new release. Pick a quiet time, because quitting Marshal stops its runners and
a job in flight fails.

1. Roll back with the Windows procedure in
   [CLI](./cli.md#updating-repairing-uninstalling-and-rolling-back): remove the tokens
   in the window only if you will not install Marshal again, quit Marshal, uninstall,
   then install the previous release's MSI, because the MSI refuses a lower version
   while a newer one is installed.
2. When the previous release is the last command-line release, install it with that
   release's procedure instead, at its tag, as
   [The Last Command-Line Release](./cli.md#the-last-command-line-release) describes,
   and follow that tag's copy of this section from there. Keep the token files until
   the command line's runners work; the command line reads them, and the app's
   tokens stay in Credential Manager.
3. Start the release you installed, rebuild the image and start the runners. The
   build uses the image definition embedded in the installed release, so it gives
   back the image that release expects; an image of another contract keeps the
   runners from running jobs, as [Recovery](#recovery) says.
4. Confirm that **Settings, Apps** lists the version you installed, and that the
   [Health Checks](#health-checks) hold for GitHub and the machine.

A diagnostic container that the stopped runners retained survives the install. If
the release you go back to may not finalise it as the installed one does, recover the
marked containers first, as
[Recover before disabling diagnostics](#recover-before-disabling-diagnostics)
describes.
