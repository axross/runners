# Windows Runner Host

How to run ephemeral GitHub Actions runners for several repositories from one
Windows machine with Docker Desktop, with the CLI, `marshal`, from an unconfigured
machine to a healthy host: prerequisites, installing `marshal`, the
per-repository token and its file's access control, the host configuration,
building the image, running and stopping the pools, health checks, rebuilding and
updating, network access, and recovery. It ends with the way back to the PowerShell
supervisor, which stays in the repository as a rollback path until Phase 4c of the
roadmap removes it. It covers the machine and the container that run a job; what a
job installs and builds is each consumer repository's own concern. The files are
under [`images/actions-runner/`](../../images/actions-runner/README.md) and, for the
rollback, [`hosts/windows-docker-desktop/`](../../hosts/windows-docker-desktop/README.md).
[CLI](./cli.md) owns what each `marshal` command does, and the trust rules the host
is reviewed against are in [Security](../conventions/security.md).

**Verification status.** CI builds the image and runs a smoke test on it for every
pull request, runs the configuration validation under PowerShell 7 and under
Windows PowerShell 5.1, and runs the CLI's tests on GitHub-hosted Linux, Windows,
and macOS runners; [CLI](./cli.md) says what each covers. The configuration
fixtures are shared by both validators and live in
[`tests/host-configuration/`](../../tests/host-configuration). The steps below that
use Docker Desktop, `icacls`, and a released `marshal` on a real machine are
described from the CLI's specification and the tools' documentation, and most were
then checked on axpc ([#56](https://github.com/axross/runners/issues/56)). Most of
those checks rest on the maintainer's report on #56 rather than on posted output,
and each place below that depends on one says which. Verified on axpc:

- installing a released binary (maintainer's report,
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

- the install's refusals, which were not run because installation is to move to
  winget and Homebrew ([#70](https://github.com/axross/runners/issues/70));
- the token-file access control step's result, whose `icacls` evidence was not
  collected;
- Ctrl+Break in the `start` window, which could not be tested;
- that a second Ctrl+C keeps the diagnostic container, and the timing of the
  diagnostic containers' kill when the window closes, which the report did not give;
- the Job Object against Docker Desktop, after a hard kill of `start`, which was not
  run, by the maintainer's decision;
- the instance lock refusing a `start` by another Windows user, and the control pipe
  rejecting a client on another machine, which were not run, by the maintainer's
  decision.

## Prerequisites

- **Windows 11 Pro, version 23H2 or later**, the minimum for Docker Desktop's WSL 2
  backend on Windows 11 (see
  [Docker's system requirements](https://docs.docker.com/desktop/setup/install/windows-install/)),
  with hardware virtualization enabled and the WSL 2 feature on.
- **The machine stays powered on, signed in, and online.** Locked is fine; signed
  out is not. `marshal start` runs in the foreground in a terminal of the signed-in
  user, and nothing starts it at sign-in, so after a reboot or a sign-out the pools
  stay down until you run `marshal start` again. A job whose labels match no runner
  waits in the queue until GitHub cancels it.
- **Docker Desktop** with the WSL 2 backend. This procedure does not use the
  Hyper-V backend or a Docker Engine inside a WSL distribution.
- **GitHub CLI (`gh`) 2.102 or later, signed in** (`gh auth status`), which the
  install procedure uses to download and verify `marshal`.
- **Windows PowerShell 5.1** (shipped with Windows) or PowerShell 7, for the inline
  access control steps and the install procedure.
- **No PowerShell supervisor or its scheduled tasks.** A host that still runs them
  stops them first; see [Installing marshal](#installing-marshal).
- **A checkout of this repository**, until Phase 4c, in a stable location, because
  [the rollback](#rolling-back-to-the-powershell-supervisor) runs its scripts and
  their scheduled tasks run them from there. Nothing else uses it: the CLI needs no
  checkout, and `build` uses the image definition embedded in the binary.

## Installing Docker Desktop

1. Install Docker Desktop and accept the WSL 2 backend.
2. Under **Settings, General**, enable **Start Docker Desktop when you sign in to
   your computer**, which is off by default. `marshal start` waits for the Docker
   daemon, trying every 5 s, so it does not matter which of the two starts first.
   `marshal build` does not wait: Docker must answer within 30 s.
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

## Installing marshal

On a host that still runs the PowerShell supervisor, first stop it and remove its
tasks as [Returning to the CLI](#returning-to-the-cli) describes. Never run
`marshal start` or `marshal build` while either task is registered: `start`'s stale
cleanup removes the supervisor's job containers, and the scripts do not take the
CLI's instance lock.

Install `marshal` from a release with the Windows procedure in
[CLI](./cli.md#installing-updating-and-rolling-back), never by building it on the
host. The procedure refuses a binary whose checksum, commit on main, or attestation
from `release.yaml` on main does not verify, or whose `--version` does not print the
release's version, before it replaces anything. Those refusals are not yet verified
on axpc ([#56](https://github.com/axross/runners/issues/56)): the refusal checks
were not run there, because installation is to move to winget and Homebrew
([#70](https://github.com/axross/runners/issues/70)). It installs the binary at
`%LOCALAPPDATA%\Programs\marshal\marshal.exe`. Add that directory to your user
`Path` under **Settings, System, About, Advanced system settings, Environment
Variables**, then open a new terminal and confirm that `marshal --version` prints
`marshal <version>`. The install itself is verified on axpc: the block for `0.1.0`
ran to completion (maintainer's report,
[#56](https://github.com/axross/runners/issues/56#issuecomment-6511344317)), and
`marshal --version` printed `marshal 0.1.0`
([#56](https://github.com/axross/runners/issues/56#issuecomment-6511048211)).

The same procedure updates `marshal` with a newer tag and rolls it back with the
previous one; it stops a running instance first. After an update, rebuild the image
and start again, as [Updating the Runner](#updating-the-runner) describes. The
release binaries are not code-signed. On axpc the installed copy had no
`Zone.Identifier`, so no Mark of the Web
([#56](https://github.com/axross/runners/issues/56#issuecomment-6511048211)), and
SmartScreen did not prompt at the install or at its first run from the terminal,
with "Check apps and files" On and Smart App Control Off
([#56](https://github.com/axross/runners/issues/56#issuecomment-6511120441)). So
with Smart App Control Off, the unsigned binary the install procedure places, which
carries no Mark of the Web, is not blocked on axpc. A copy that carries one, such as
a browser download, or a host with Smart App Control On, could be.

## The Fine-Grained Token

`marshal start` asks GitHub for a single-use runner registration through
[`POST /repos/{owner}/{repo}/actions/runners/generate-jitconfig`](https://docs.github.com/en/rest/actions/self-hosted-runners),
which needs the **Administration** repository permission set to **Read and
write**. Create one **fine-grained personal access token per target repository**,
under **Settings, Developer settings, Fine-grained personal access tokens**:

- **Repository access:** only that one repository.
- **Permissions:** Administration, read and write, and nothing else.

Save each token in a file on this machine, holding the raw token text, at the
`tokenPath` of the repository's pool entries, and keep these files outside the checkout.
The token rules are in the Per-Repository Isolation on a Runner Host section of
[Security](../conventions/security.md).

`marshal start` reads the file again at every registration, so a rotated token
applies to the next job in every slot with no restart. Rotate a token by deleting
the file and creating it again with the new token, then restrict it as below: a new
file inherits its folder's permissions until the step resets and restricts them. Do
not edit the file in place.

### Restricting a Token File

The CLI does not check a token file's access control list on Windows, because no
safe call reads it, so this step is the only thing that restricts the file. Run it
for every configured token file at install, and for the new file at every token
rotation. It does what the rollback's `register-scheduled-tasks.ps1` does: it resets
the file's permissions to its folder's defaults, removes inherited permissions, then
grants the host user read access, so an entry granted by hand to another account
does not survive. Run it as the user who runs `marshal start`, with every token path
listed:

```powershell
& {
  $ErrorActionPreference = 'Stop'
  $user = "$env:USERDOMAIN\$env:USERNAME"
  foreach ($token in @('C:\path\to\example-repo.token')) {
    if (-not (Test-Path -LiteralPath $token -PathType Leaf)) { throw "no token file at $token" }
    icacls $token /reset
    if ($LASTEXITCODE -ne 0) { throw "icacls could not reset the permissions of $token" }
    icacls $token /inheritance:r
    if ($LASTEXITCODE -ne 0) { throw "icacls could not remove inherited permissions from $token" }
    icacls $token /grant:r "${user}:(R)"
    if ($LASTEXITCODE -ne 0) { throw "icacls could not restrict $token to $user" }
    icacls $token
  }
}
```

The last `icacls` of each file lists its access rules; it MUST list only your user,
with `(R)`, and no inherited entry. A rule for anyone else means the step did not
run on that file. This step's effect is not yet verified on axpc
([#56](https://github.com/axross/runners/issues/56)): its `icacls` evidence was not
collected there.

## The Host Configuration

The configuration is a JSON file kept outside the checkout; every `marshal` command
that reads it takes its path as `--config`. Start from
[`runner-host.example.json`](../../hosts/windows-docker-desktop/runner-host.example.json),
and see the Host configuration section of the
[host README](../../hosts/windows-docker-desktop/README.md) for every field. The rules that matter operationally:

- **One entry per execution pool**, each with its own `name`, `slots`,
  `labels`, and `volumes`. Every entry uses the global `imageName`.
  `tokenPath` is an absolute Windows path, a drive
  letter and backslash or a UNC path; a relative path is rejected, because the
  file a relative path names would depend on the directory a command runs from.
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
  lock is machine-wide: one `marshal start` runs on the machine, whichever
  configuration and whichever user it runs with.
- **Every job container has CPU and memory limits.** `cpus` is a number above 0
  and at most 64, 2 when absent. `memoryGb` is an integer from 1 to 256, 8 when
  absent. Each job container of the entry starts with `--cpus` and `--memory`
  set to them and `--memory-swap` equal to `--memory`, so it gets no swap beyond
  its memory. Docker takes `--cpus` as a CPU-time quota and `--memory` as
  a size with a unit suffix such as `g`; see
  [Docker's resource constraints](https://docs.docker.com/engine/containers/resource_constraints/).
  The limit applies to each container, not to an entry's slots together or to the
  host. The short container that resets volume ownership at startup has none.

Check a configuration before using it, and after every edit. The command calls
neither Docker nor GitHub, prints the global image and each entry's labels, name,
container name pattern, CPU and memory limits, planned affinity
cardinality/position, and volume names, and exits 1 naming the field of every
problem. It selects no CPU IDs and does not check that a token file exists or who
can read it:

```powershell
marshal validate --config C:\path\to\runner-host.json --profile axpc
```

### Split a repository into general and Android pools

Keep the general entry's existing name, slots, resource limits, labels, mounts,
and diagnostic settings. If its `labels` list is non-empty, explicitly include
`axpc` alongside its existing aliases to preserve the effective registration.
Leave the global image unchanged. Add an entry for the same repository with a
distinct non-nested name, the same token path, a dedicated label in `labels`,
and `slots: 1`. For example, the added entry can be:

```json
{
  "owner": "example-owner",
  "repository": "example-repo",
  "name": "example-android",
  "tokenPath": "C:\\path\\to\\example-repo.token",
  "labels": ["example-android-route"],
  "slots": 1,
  "volumes": []
}
```

Use operator-local values, not these example names. Neither the image name nor
the entry name identifies a workload to `marshal`. Set resource limits
only from approved host evidence; a dedicated slot does not reserve host memory.
No shared SDK, workspace, or new persistent cache is needed. Existing storage
choices follow the [shared-storage rules in Security](../conventions/security.md#shared-runner-storage-is-a-cache-poisoning-surface).
Diagnostics remain independent per entry, with private
retention keyed by its name even if two entries use the same private sink.

Deployment and workflow execution require separate scoped authorization:

1. Record the existing configuration, `marshal status`, actual slots, image IDs
   and tagged rollback images. Do not rebuild the image during this change.
2. Establish a quiet maintenance window: finish affected jobs, including old
   workflow revisions still using the general label. Do not restart a busy
   instance; `marshal restart` stops every entry's containers, not just Android's.
   Do not restart Docker or WSL for this change.
3. Validate the split configuration with `marshal validate`, and apply it with
   `marshal restart` only under the approved host-operation scope. Verify the
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
scope. In a quiet window, stop the instance with `marshal stop`. If it does not
stop, keep rollback pending rather than force an unsafe stop. Restore the saved
host configuration/images, keeping the explicit `axpc` migration in restored
general custom-label lists, because `marshal` reads a non-empty list as a
replacement for `axpc`. Validate with `marshal validate`.
Restore consumer routing under its separate authorization while the instance
remains stopped, then run `marshal start` and read back actual
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

There is no JSON switch to disable affinity, and every `marshal` release applies
it. Going back to placement without affinity means the PowerShell path under
[Rolling Back to the PowerShell Supervisor](#rolling-back-to-the-powershell-supervisor),
with the saved pre-affinity host scripts and their compatible configuration and
image, in its own authorized maintenance window. Stop job admission and drain work,
stop the instance with `marshal stop`, and recover marked diagnostic containers
before replacing scripts or images. In that section's steps, this case replaces
step 3's checkout of the default branch and step 4's rebuild with restoring the
saved scripts and image; still allow the scripts to run as step 3 says. Do not
rebuild during the rollback: right after step 5, run
`Disable-ScheduledTask -TaskName actions-runner-weekly-rebuild`, and keep the task
disabled until the readback below is confirmed. Validate offline, and only then
start the supervisor. Confirm the restored source/image
identities and requested/effective CPU sets and quotas before resuming admission
and re-enabling the weekly rebuild task; an empty requested cpuset is not an empty effective set. Restore no WSL allocation
or Windows process-affinity setting: this feature changes neither. A failed runtime
readback blocks rollout, not independent pool work.

## Moving to the New Configuration Format

A host whose configuration still has `hostPrefix` or a per-entry `prefix` is
rejected, naming the field. To move it:

1. Note the `hostPrefix`, and each entry's prefix, `<hostPrefix>-<owner>-<repository>`
   in lowercase unless the entry set `prefix`. Stop the host. A host still on the
   PowerShell supervisor of that time stops it with Ctrl+C in its window, then ends
   and removes the `<hostPrefix>-supervisor` and `<hostPrefix>-weekly-rebuild`
   tasks:

   ```powershell
   Unregister-ScheduledTask -TaskName <hostPrefix>-supervisor -Confirm:$false
   Unregister-ScheduledTask -TaskName <hostPrefix>-weekly-rebuild -Confirm:$false
   ```

2. Rewrite the file: give every entry a `name`, and delete `hostPrefix` and every
   `prefix`. An entry whose `name` equals its previous prefix, which is possible
   only when the prefix is a valid `name`, keeps its volumes; any other `name`
   starts from empty volumes.
3. Run `marshal validate` on the file until it passes.
4. Remove what the earlier names left, skipping an entry whose `name` equals its
   previous prefix. Remove a volume only once no container uses it:

   ```powershell
   docker ps -a --filter "name=<previous prefix>-"
   docker rm -f <container name>
   docker volume rm <previous prefix>-<suffix>
   ```

5. Continue from [Installing marshal](#installing-marshal) on a host without it, or
   run `marshal start` on one that has it.

Job containers had no CPU or memory limit before. Without `cpus` and `memoryGb`,
each is now capped at 2 CPUs and 8 GB, so set both on an entry whose jobs need
more.

## Building the Runner Image

Build the image once at install, after every update of `marshal`, and at each
routine rebuild. `build` is refused while an instance exists, so stop the instance
first:

```powershell
marshal stop
marshal build --config C:\path\to\runner-host.json
marshal start --config C:\path\to\runner-host.json
```

`marshal stop` prints that no instance is running, and exits 1, when there is none;
that is fine here. `build` builds the image definition embedded in the installed
`marshal`, under the configured global `imageName`, with
`docker build --pull --no-cache`, and reads nothing from a checkout or the working
directory. The runner version and base image digest come from that definition, so
a newer runner needs a newer release; see [Updating the Runner](#updating-the-runner).
The cache is ignored so that the operating system packages are installed again and
pick up their updates. The build goes to a temporary tag, and only a successful
build is tagged as the configured name; a failed build leaves the previous image in
place. After a successful build `build` removes the earlier images it or the
PowerShell rebuild script built that the new build left untagged: the prune is
limited to images carrying the label `com.github.axross.runners.image=actions-runner`,
so it does not touch other dangling images or images built by hand. It does not
reclaim installation stages, build caches, or tagged rollback images. A failed prune
is a warning, not a failed build. [CLI](./cli.md#build) gives every step and its
exit codes.

While it builds, `marshal status` in another terminal answers `state: building`, and
`marshal stop` or Ctrl+C aborts it within 30 s; an abort before the retag leaves the
configured name on its previous image. CI runs `marshal build` only against a small
test definition on Linux; a full build of the image on Docker Desktop is verified on
axpc (maintainer's report,
[#56](https://github.com/axross/runners/issues/56#issuecomment-6511344317)).

The Android NDK contract, Java/SDK/Gradle inputs, and Build sections of the
[image README](../../images/actions-runner/README.md) own version/checksum
maintenance, the consumer fallback contract, and additional image-build costs.

For an authorized image update, rollback, or fixed-image comparison:

1. Pause the routine rebuild, and do not rebuild by hand during the window.
2. Record the stable current image ID with
   `docker image inspect <configured-image-name> --format '{{.Id}}'`. Before an
   update, retain it with `docker tag <saved-image-id> actions-runner:rollback`;
   this tag keeps it out of the dangling-image prune.
3. Build the approved new image with the `marshal` release that embeds it, or roll
   back by tagging the saved image ID as the configured image name again. Record
   the image ID actually used. Existing containers finish on their original image;
   subsequent containers use the retagged or rebuilt one. The image check before
   each job accepts the retagged image only when its contract label matches the
   installed `marshal`; see [CLI](./cli.md#the-image-check).
4. A retag alone lasts only until the next `marshal build`, which builds the
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

Start the pools in a terminal you keep open, and leave it running:

```powershell
marshal start --config C:\path\to\runner-host.json
```

`start` validates the configuration as `validate` does, writes a log file under
`%LOCALAPPDATA%\marshal\logs\` as well as to the terminal, waits for Docker, checks
the image, prepares every pool, and then runs every slot until it is stopped. It
exits 1 when the configuration is invalid, when the image cannot run jobs, or when
no pool can start. Nothing starts it at sign-in: after a reboot or a sign-out the
pools stay down until you run `marshal start` again.

From another terminal, as the same user:

| Command               | What it does                                                                                                                                                           |
| --------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `marshal status`      | Prints the instance's state, each pool's and each slot's state, and the image lines, with any freshness warning                                                        |
| `marshal restart`     | Stops every job container, reads the configuration file again, checks the image, and runs the pools; use it after editing the configuration                            |
| `marshal stop`        | Stops the instance with the stop sequence, within 30 s, and exits 0 once the instance has ended                                                                        |
| `marshal diagnostics` | Finalises the diagnostic containers the instance still retains, and reports each pool's bundles; see [Collecting private diagnostics](#collecting-private-diagnostics) |

[CLI](./cli.md#running-a-host) gives each command's fields, waits, and exit codes.
A `restart` that finds an invalid file or an image that cannot run jobs leaves the
instance stopped with the reason, which `status` shows; fix it, then run
`marshal start` with the same `--config`, which resumes the stopped instance.

Stop the host with `marshal stop`, or with Ctrl+C in the `start` window, which runs
the same stop sequence. A second Ctrl+C skips the grace period: the ordinary
containers are removed at once, each diagnostic container is ended with
`docker kill` and kept without its export, and `start` exits 1, naming each kept
container. Ctrl+Break does the same as Ctrl+C. On axpc one Ctrl+C stopped the
instance with exit 0, and a second Ctrl+C force-stopped it with exit 1 (maintainer's
report, [#56](https://github.com/axross/runners/issues/56#issuecomment-6511420845)).
That the second Ctrl+C kept the diagnostic container is not yet verified on axpc
([#56](https://github.com/axross/runners/issues/56#issuecomment-6511420845)): the
report did not say. Ctrl+Break is not yet verified on axpc
([#56](https://github.com/axross/runners/issues/56#issuecomment-6511420845)): it
could not be tested.

Do not close the `start` window to stop it. Closing it stops the instance with
forced removal within about 4.5 s: ordinary containers are removed, and diagnostic
containers are ended and kept for the next `start` to finalise. A sign-out or
shutdown does not reach `start` at all, so its containers wait for the next `start`.
Closing the window is verified on axpc, as working as described (maintainer's
report, [#56](https://github.com/axross/runners/issues/56#issuecomment-6511420845)).
Its timing, and the diagnostic containers' kill within it, are not yet verified on
axpc ([#56](https://github.com/axross/runners/issues/56#issuecomment-6511420845)):
the report gave no timing.

When `start` ends however it ends, even by `taskkill /F`, the Job Object it puts
itself in ends every `docker` client it started. Docker Desktop keeps a container
running after its client is killed, so a job can run on until the next `start`
removes its container. The Job Object against Docker Desktop is not yet verified on
axpc ([#56](https://github.com/axross/runners/issues/56#issuecomment-6511420845)):
the hard-kill check was not run, by the maintainer's decision.

One instance runs per machine. A `start` by another Windows user while an instance
runs is refused, with a message that the lock is held by another OS user or by an
instance still starting or ending; that refusal is not yet verified on axpc
([#56](https://github.com/axross/runners/issues/56#issuecomment-6511420845)): the
second-user check was not run, by the maintainer's decision. The other terminals
reach the instance through a control pipe that only its own user can open and that
rejects remote clients; the rejection of a client on another machine is not yet
verified on axpc
([#56](https://github.com/axross/runners/issues/56#issuecomment-6511420845)): the
remote check was not run, by the maintainer's decision.
[Security](../conventions/security.md#one-instance-per-machine-reached-only-by-its-user)
records what this protects.

## How a Container Picks Up a Job

For each execution pool, `start` runs that entry's `slots` as separate loops. A
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
   [Security](../conventions/security.md) owns how `start` hands it to
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
`start` that runs the pools fails as a whole, it is started again after 10 s, 30 s,
and 60 s; a fourth failure within 10 minutes stops the instance, with the restart
limit as its reason in `marshal status`. [CLI](./cli.md#the-runtime-layer) lists
where this differs from the PowerShell supervisor.

## Collecting private diagnostics

Diagnostics are off by default. Enable them only for the entry whose evidence
you need, after separate authorization to operate that host. The default
example enables neither diagnostics nor raw records. No resource allocation,
build command, tool version, registration or mount changes with this opt-in.
The sink is not exposed to the job as a bind mount or volume.

Collect private evidence as follows:

1. Pre-create a directory on a local Windows filesystem, outside the checkout.
   Restrict its owner and all allowed access rules to the account that runs
   `marshal start`. `marshal` MUST NOT create the sink, relax permissions, or
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

2. Set `diagnostics` to `true` and `diagnosticDirectory` to that directory in
   the target entry. Leave `diagnosticRawRecords` absent or `false` unless
   private daemon/crash retention is separately wanted. Set it to `true` only
   with diagnostics enabled. Run `marshal validate`, then apply the change with
   `marshal restart` in an authorized quiet window. Validation rejects a sink
   inside this project's checkout without Docker, GitHub or token reads, but does
   not check that the sink exists, is private, or is writable.
3. After a container stops, `marshal start` exports its evidence into a new
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
5. Keep all raw records private. `marshal` never prints or logs them.
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
18 s and the removal by 24 s after the container ended, and a stop of the instance
finalises the diagnostic containers within its 30 s. A full, inaccessible or unsafe
sink, a failed or partial copy, a missing collector, or an unavailable final sample
is a diagnostic gap, logged as
`Diagnostic gap in pool <pool>, container <name>: <text>`, never the runner's
failure. A removal that fails leaves the container retained for the next
finalisation, which `marshal diagnostics` runs on demand; `diagnostics` prints
`pool <name>: <k> bundles written, <r> of 10 remaining` per pool and exits 1 while a
container it acted on stays. [CLI](./cli.md#diagnostics) owns the timeline, the
exporter, and the startup finalisation.

Each diagnostic container carries only the immutable
`runners.diagnostic-lifecycle=1` label, not a registration or private path. When
`start` prepares a pool, it finds the labelled containers that an earlier run of
the pool left, such as those a second Ctrl+C or a closed window kept. With the
pool's diagnostics on, it exports and removes each one with the pool's current
settings before any of the pool's slots registers. Ordinary unlabelled containers
keep their ordinary removal even when the entry now enables diagnostics. Prior
evidence is never automatically deleted. Operators MUST review and privately
archive or remove prior bundles before collecting beyond the quota. Retention
outside this feature is their decision. A diagnostic job and its export on Docker
Desktop are verified on axpc: the bundle was written with its `complete.txt`, and
`marshal diagnostics` reported the counts and exited 0 (maintainer's report,
[#56](https://github.com/axross/runners/issues/56#issuecomment-6511420845)).

### Recover before disabling diagnostics

Recover an entry's diagnostic containers before turning its diagnostics off, and do
not start another runner against the retained evidence. When an entry's
diagnostics are already off and labelled containers of it remain, `start` gives them
one bounded `docker stop`, warns for each, and refuses that entry, naming this
section; the other entries run. `marshal diagnostics` then names each such container
on a line of its own and exits 1. `start` never removes or exports them while the
entry's diagnostics are off, and does not silently turn collection on.

Recover them with the CLI alone, after separate host authorization:

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
   `diagnosticDirectory` set to the sink from step 1, and run `marshal validate`.
   Then run `marshal start`, or, when an instance is already running,
   `marshal restart` in an authorized quiet window, because `restart` stops every
   entry's job containers. The entry's preparation adopts each leftover container, exports it, and
   removes it before any slot registers, and logs one line for each:
   `Diagnostic container <name> that an earlier run left: exported, removed.`, with
   `export gap` or `not removed` in place of either word. It removes the container
   even when the export fails, reporting an `export gap`; that is why step 1 comes
   first.
3. **Confirm.** First wait until the `start` log has step 2's line for every
   container step 1 recorded: `diagnostics` acts only on containers no other path
   holds, so while the preparation is still adopting them it can exit 0 too early.
   Treat a line with `export gap` or `not removed` as not recovered: an
   `export gap` container is already removed, and its evidence is lost. Run step 1's
   `docker ps -a` listing again and confirm that none of the names step 1 recorded
   is still listed. Other names can appear: step 2 started the entry's slots with
   diagnostics on, so each live slot's job container has the same name pattern and
   label. Those are expected, and must not be removed. Then run
   `marshal diagnostics` from another terminal, which exits 0 when no container
   stays retained. An exit of 1 means a container is still retained, such as one
   whose removal failed: run `marshal diagnostics` again, or abandon it as in
   step 5.
4. **Then disable.** Run `marshal stop`, set `diagnostics` to `false`, remove
   `diagnosticDirectory` and `diagnosticRawRecords`, run `marshal validate`, and
   then run `marshal start`.
5. **Abandon, only by choice.** If you choose to abandon a container's evidence,
   stop the instance with `marshal stop`, then remove only that identified,
   labelled container with `docker rm -f <name>`, never with a prune, and start
   again.

This procedure is source guidance, not authorization to operate a host. The
PowerShell supervisor's own recovery, which the rollback needs, is under
[Recovering Diagnostic Containers With PowerShell](#recovering-diagnostic-containers-with-powershell).

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
[`runner-host.example.json`](../../hosts/windows-docker-desktop/runner-host.example.json);
the volume's name is the entry's `name` plus the suffix.

| Suffix      | Mount path             | Why it exists                                                                          |
| ----------- | ---------------------- | -------------------------------------------------------------------------------------- |
| `toolcache` | `/opt/hostedtoolcache` | Language runtimes that the `setup-*` actions install and `RUNNER_TOOL_CACHE` points at |
| `cargo`     | `/home/runner/.cargo`  | Cargo's registry downloads and the binaries it installs                                |
| `rustup`    | `/home/runner/.rustup` | Rust toolchains                                                                        |
| `npm`       | `/home/runner/.npm`    | npm's content-addressed package cache                                                  |

Any job routed to the entry's labels can write to every one of them, and no
other entry mounts them. When `start` prepares a pool, it creates the entry's
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
If either applies, the entry lists `"volumes": []`, so each job starts from the
image alone, or those workflows, including the fork pull request workflows, run on
a GitHub-hosted runner. That includes `npm` and every other volume in the table,
not only the toolchain volumes. The host cannot check this; the operator does. The
rule and its reason are in the Shared Runner Storage Is a Cache-Poisoning Surface
section of [Security](../conventions/security.md).

## Removing an Entry

Remove the entry from the configuration, then run `marshal validate` and
`marshal restart`. A volume outlives its entry: the entry's volumes and any stopped
containers stay until removed by hand with `docker volume rm` and `docker rm`. The
isolation rules the host enforces between repositories are in
[Security](../conventions/security.md).

## Health Checks

- **GitHub:** a healthy host shows, under each repository's **Settings, Actions,
  Runners**, the sum of its entries' `slots` runners, **Idle** when no job is queued and **Active** when
  one is running, because each slot registers a runner before it starts a
  container. Each registration carries exactly `self-hosted`, `linux`, `x64`, and
  the entry's labels, `axpc` by default. Fewer runners than that sum, and none in
  particular, mean `start` is not running or one or more pools are failing. Compare
  runner-name prefixes with each entry's `name` and `slots` to identify the affected
  pool; see Recovery.
- **The instance:** `marshal status` reports `state: running`, a
  `pool: <name>: running` line for every entry, and `image-contract: <n>` rather
  than a mismatch or `missing`. A `pool: <name>: skipped: <reason>` line names a
  pool that did not start. A `warning:` line is a freshness warning; see
  [Rebuilding the Image](#rebuilding-the-image-for-operating-system-updates).
- **Machine:** `docker ps` shows `slots` running containers per entry, named
  `<name>-<index>-<timestamp>`, whether idle or busy, since an idle container is
  a runner waiting for a job. None means the same as above.
  `docker inspect` on a container shows its limits under `HostConfig`: `NanoCpus`
  is the CPU-time quota in billionths of a CPU, `CpusetCpus` is the requested
  CPU set, and `Memory` and `MemorySwap` are both `memoryGb` gigabytes in bytes.
  Requested settings are not effective readback. Each job container's output in the
  `start` log has the guard's bounded `CPU affinity verified` line, which reports
  the kernel-granted set; the authorized real-host check also resolves the actual
  cgroup cpuset/quota layout.
- **Token files:** `icacls <token file>` lists only the host user, with `(R)`; see
  [Restricting a Token File](#restricting-a-token-file). The step's effect is not
  yet verified on axpc ([#56](https://github.com/axross/runners/issues/56)): its
  `icacls` evidence was not collected there.
- **End to end:** for each pool, a workflow run whose `runs-on` matches that
  entry's routing label starts executing rather than sitting queued.

## Rebuilding the Image for Operating-System Updates

No task rebuilds the image, so the operating-system packages in it are only as
fresh as its last `marshal build`. Rebuild it weekly, and also whenever the `start`
log (at start, a resume, or a restart) or `marshal status` shows a build-age
warning: from 7 days
`` the image is <d> days old and should be rebuilt with `marshal build` ``, and from
21 days a warning that its operating-system packages are that old. An image that
`marshal build` did not build, such as one from the PowerShell rebuild script, has
an unknown build age and a warning that says so. At a quiet time:

```powershell
marshal stop
marshal build --config C:\path\to\runner-host.json
marshal start --config C:\path\to\runner-host.json
```

`build` is refused while an instance exists, so a rebuild stops the pools for the
length of the build: a job in flight fails, and queued jobs wait. A failed build
leaves the previous image, and `start` runs on it.

The runner-release-age warning,
`runner <v> was released <d> days ago, ...`, is different: a rebuild does not clear
it. The runner version comes from the image definition embedded in the installed
`marshal`, so a rebuild with the same release builds the same runner. That warning
calls for a runner bump, below.

## Updating the Runner

A newer runner reaches the host only through a new `marshal` release, because the
binary embeds the image definition:

1. Merge the Dependabot pull request that bumps the `FROM` line in
   `images/actions-runner/Dockerfile`, with the edits it needs by hand; see
   [Development Workflow](./development-workflow.md#bumping-the-runner). Keep its
   `build(deps)` title when merging: it releases a patch, as
   [Version Rules](./development-workflow.md#version-rules) describes.
2. The maintainer dispatches `release.yaml` on main, as
   [Each Release](./development-workflow.md#each-release) describes.
3. Update `marshal` to that release with the procedure in
   [CLI](./cli.md#installing-updating-and-rolling-back), which stops the instance
   first.
4. Run `marshal build`, then `marshal start`, as in
   [Building the Runner Image](#building-the-runner-image). Containers already
   running when the update stopped the instance have ended; the next ones start on
   the new image.

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
image build, whenever `marshal build` runs, also reaches the Ubuntu package archives
that the base image's apt sources name, over port 80 or 443 (by default
`archive.ubuntu.com` and `security.ubuntu.com`; check the base image's sources
before filtering). Linux x64 tool provisioning also needs HTTPS to
`dl.google.com` for the checksum-pinned Android SDK/NDK/CMake archives,
`github.com` and `release-assets.githubusercontent.com` for Temurin, and
`services.gradle.org` for the Gradle distribution, including its redirects to
GitHub release-asset storage. The Dockerfile itself needs no access to Docker
Hub. No inbound port is needed.

## Recovery

- **A container left by an unclean stop** (a closed window, a hard kill, a crash,
  a reboot mid-job) is handled when the next `start` prepares its pool, using each
  entry's own name pattern and the immutable diagnostic label. Ordinary containers
  are removed before the slots start. Labelled containers are exported and removed
  while the entry's diagnostics are on; with them off they refuse the entry until
  [Recover before disabling diagnostics](#recover-before-disabling-diagnostics) is
  done. By hand, `docker ps -a --filter "name=<name>-"` lists candidates and
  `docker rm -f <name>` removes an ordinary one.
- **Docker Desktop restarts mid-job:** the container is gone, the job fails on
  GitHub's side, and re-running the workflow is the recovery. `start` keeps
  running: each slot's next attempt fails into its backoff while Docker does not
  answer, remeasures affinity, and resumes by itself when Docker is back.
- **The pools are down after a reboot or a sign-out:** nothing starts `start` at
  sign-in. Run `marshal start` again once Docker Desktop is up, or straight away,
  since `start` waits for Docker.
- **`marshal stop` exits 1:** the instance still held its lock 35 s after the
  request. `marshal status` shows whether it is still ending; Ctrl+C twice in the
  `start` window removes its containers at once.
- **One pool's entry will not start:** a missing or blank token file, a
  volume that cannot be created, or stale containers that cannot be listed or that
  carry the diagnostic label with diagnostics off skip that entry only. `start` logs
  a warning naming the entry and the reason (never a token), `marshal status` shows
  `pool: <name>: skipped: <reason>`, and the other startable pools run, including a
  sibling for the same repository. Shared dependencies, such as that repository's
  token or the Docker daemon, can affect several pools. `start` exits 1 only when
  no entry can start. Fix the cause and run `marshal restart` in an authorized quiet
  window; an entry skipped at start is not retried until the pools are prepared
  again.
- **A rejected configuration:** `start` and `validate` print every offending field,
  and `start` exits before starting anything. A `restart` with a rejected file
  leaves the instance stopped with the errors as its reason. Fix the file, run
  `marshal validate`, then `marshal start` with the same `--config`.
- **An image that cannot run jobs** (missing, or with another contract):
  `start` exits 1, and a running instance stops with that reason. Run `marshal stop`,
  `marshal build`, then `marshal start`.
- **The restart limit:** a fourth failure of the pools' runtime within 10 minutes
  stops the instance, with the limit as its reason. Read the `start` log, fix the
  cause, then run `marshal start` with the same `--config`.
- **A slot logging CPU discovery/cleanup failures:** registration is withheld.
  Check the intended Docker context/local image and available CPUs in a separately
  authorized maintenance window. Resolve the exact marked slot probe if deletion
  cannot be confirmed; never remove unrelated containers or prune broadly.
  A topology or readback mismatch must not be bypassed with unrestricted startup.
- **A slot logging registration failures:** the message carries GitHub's status. A
  401 or 404 usually means the token expired or lacks Administration access to that
  repository; replace the file and restrict it as
  [Restricting a Token File](#restricting-a-token-file) describes. Other slots
  continue their own registration loops, but a shared-token problem can affect
  every pool for that repository.

## Rolling Back to the PowerShell Supervisor

This section exists only until Phase 4c of the roadmap removes the PowerShell
scripts under
[`hosts/windows-docker-desktop/`](../../hosts/windows-docker-desktop/README.md). It
is the way back if `marshal` misbehaves, and it is the only part of this procedure
that runs those scripts. The PowerShell path is not exercised while the host runs
`marshal`; [What Was Verified of the PowerShell Path](#what-was-verified-of-the-powershell-path)
says what is known of it.

Never run `marshal start` or `marshal build` while the supervisor or its rebuild
task is registered. `start`'s stale cleanup removes the supervisor's job
containers, and the scripts do not take the CLI's instance lock, so a `build` and
the rebuild task could replace the same image at once.

1. At a quiet time, stop the CLI with `marshal stop`. It exits 0 once the instance
   has stopped. It prints `No instance is running.` and exits 1 when there is none;
   that is fine here. Any other exit 1 means the instance still holds its lock: see
   [Recovery](#recovery), and do not go on until it has stopped.
2. List the labelled containers with
   `docker ps -a --filter "label=runners.diagnostic-lifecycle=1" --format "{{.Names}}"`.
   If a diagnostics-enabled entry left any, recover them first: with `marshal`,
   through steps 1 to 3 of
   [Recover before disabling diagnostics](#recover-before-disabling-diagnostics)
   followed by `marshal stop`, or with
   [the PowerShell procedure](#recovering-diagnostic-containers-with-powershell).
3. Put the checkout on the default branch, then, in a PowerShell window, follow
   [Allowing the Scripts to Run](#allowing-the-scripts-to-run).
4. From `hosts\windows-docker-desktop\` in the checkout, build the image from the
   checkout's definition:

   ```powershell
   .\rebuild-image.ps1 -ConfigPath C:\path\to\runner-host.json
   ```

5. From an elevated prompt, register the tasks, which also restricts every
   configured token file, as
   [Registering the Scheduled Tasks](#registering-the-scheduled-tasks) describes.
6. Start the supervisor with `Start-ScheduledTask -TaskName actions-runner-supervisor`,
   or sign out and in. `Get-ScheduledTask actions-runner-supervisor | Select State`
   then reports `Running`, and the Health Checks above hold for GitHub and the
   machine.

### Allowing the Scripts to Run

Windows PowerShell 5.1's default execution policy refuses local script files, so
the `.\` commands in this section fail until the prompt allows them. In each
PowerShell window you run them from, including the elevated one, run this once
before the first command:

```powershell
Set-ExecutionPolicy -Scope Process -ExecutionPolicy Bypass
```

It lasts only for that window and changes no machine or user setting. The
scheduled tasks pass `-ExecutionPolicy Bypass` to their own processes, so they
need nothing from you.

### Registering the Scheduled Tasks

From an elevated PowerShell prompt, once the token files exist and the image has
been built:

```powershell
.\register-scheduled-tasks.ps1 -ConfigPath C:\path\to\runner-host.json
```

This registers two tasks for the signed-in user, with fixed names:

- **`actions-runner-supervisor`** runs `supervisor.ps1` at sign-in and is set to
  restart if it fails, as a backstop for the supervisor process exiting. Whether
  Task Scheduler restarts a task that ends with a non-zero exit code is not
  verified on a real host.
- **`actions-runner-weekly-rebuild`** runs `rebuild-image.ps1` weekly, Sunday 03:00
  by default, with `docker build --pull --no-cache` from the checkout's Dockerfile,
  and runs a missed rebuild when the machine is next available.

Both run as the current user at the limited run level, never elevated. Both point
at the checkout and the configuration file, so moving either breaks them until the
script runs again. The script also restricts every configured token file to that
user, exactly as [Restricting a Token File](#restricting-a-token-file) does. Run it
again after editing the configuration, replacing a token file, or moving the
checkout. The supervisor validates with
`.\supervisor.ps1 -ConfigPath C:\path\to\runner-host.json -ValidateOnly`, which
prints the same summary as `marshal validate`, and reads the configuration only when
it starts.

### The Supervisor's Recovery

The supervisor runs each entry's slots as background jobs, with the cycle under
[How a Container Picks Up a Job](#how-a-container-picks-up-a-job); a slot job that
dies is restarted after its own backoff, which starts over once a job has run for
ten minutes. [CLI](./cli.md#the-runtime-layer) lists where `marshal` differs.

- **A container left by an unclean stop** (a forced stop of the supervisor task, a
  crash, a restart mid-job) is handled by the next supervisor start, as `start`
  does: ordinary leftovers are removed, and labelled ones are exported and removed
  while the entry's diagnostics are on. With them off, the supervisor gives them a
  bounded stop, refuses to delete or export them, warns, and gives that entry no
  slots until the recovery below.
- **Docker Desktop restarts mid-job:** the supervisor process keeps running and
  resumes by itself when Docker is back, with no task restart.
- **Stopping the supervisor:** Ctrl+C in its window is the clean path: it
  `docker stop`s the current containers, then stops every slot's job. Ending the
  task in Task Scheduler, or a shutdown, skips it and leaves running containers to
  finish or to be removed at the next start; a job in flight fails either way.
- **One pool's entry will not start:** the same causes as with `start` skip that
  entry, and the supervisor exits 1 only when no entry can start. Fix the cause and
  restart the supervisor task in an authorized quiet window; an entry skipped at
  start is not retried while the supervisor runs.
- **A rejected configuration:** the supervisor prints every offending field and
  exits before starting anything; fix the file and run it with `-ValidateOnly`
  first.

### Recovering Diagnostic Containers With PowerShell

The supervisor's exporter takes a named mutex that `marshal` does not take, so stop
any CLI instance with `marshal stop` before this export, and do not run
`marshal start` until step 4 is done. Disable the supervisor task before private
recovery or changing the opt-in. Do not start another runner against the retained
evidence. After separate host authorization, recover one marked container at a
time:

1. Keep the supervisor task disabled. Match the candidate's name to the entry's
   `<name>-<index>-<timestamp>` pattern and confirm its immutable label with
   `docker ps -a --filter "name=<name>-" --filter "label=runners.diagnostic-lifecycle=1" --format "{{.Names}}"`.
   Never dump configuration or registration values.
2. Pre-create/recheck the private sink as above, and resolve its quota by
   privately archiving prior bundles if needed. From the host scripts directory,
   under the supervisor account, use the same bounded lifetime boundary. Replace
   the placeholders and set raw collection only by explicit choice:

   ```powershell
   . .\diagnostic-export.ps1
   Initialize-DiagnosticProcessContainment
   $container = '<name>-<index>-<timestamp>'
   $entryName = '<name>'
   $sink = 'C:\path\to\private-evidence'
   $clock = [Diagnostics.Stopwatch]::StartNew()
   if (-not (Invoke-BoundedDiagnosticStop -Names @($container) -Clock $clock)) { throw 'Stop unconfirmed; retain container and investigate privately.' }
   Invoke-BoundedDiagnosticExport -Name $container -EntryName $entryName -Directory $sink -RawRecords $false -Clock $clock
   if (-not (Invoke-BoundedContainerRemoval -Name $container -Clock $clock)) { throw 'Removal unconfirmed; keep task disabled.' }
   ```

3. A failed export in this explicit procedure retains the container. Review
   partial bundles privately and resolve the gap before retrying; do not bypass
   containment or copy the complete filesystem/logs. If choosing to abandon
   evidence, explicitly remove only the identified container with
   `Invoke-BoundedContainerRemoval` and a fresh stopwatch, never an unbounded prune.
4. Verify no marked containers remain for that entry. Set `diagnostics` to
   `false`, remove `diagnosticDirectory` and `diagnosticRawRecords`, validate the
   configuration, and only then authorize/re-enable the supervisor task.

This procedure is source guidance, not authorization to operate a host. The
exporter runs storage work in a separate PowerShell process inside a Windows Job
Object, within an 18 s export deadline and a 24 s removal deadline, and assigns its
new bundle to the supervisor account; it never repairs the sink's owner or rules.

### Returning to the CLI

To move a host from the PowerShell supervisor to `marshal`, the first time or after
a rollback, stop the PowerShell host in this order:

1. Disable both tasks, so neither the sign-in trigger nor the restart-on-failure
   starts the supervisor again:

   ```powershell
   Disable-ScheduledTask -TaskName actions-runner-supervisor
   Disable-ScheduledTask -TaskName actions-runner-weekly-rebuild
   ```

2. Wait until `Get-ScheduledTask actions-runner-weekly-rebuild | Select State` no
   longer reports `Running`, so no rebuild is under way.
3. Stop the supervisor with Ctrl+C in its window.
4. If an entry has diagnostics on, list the labelled containers and recover any
   that remain with
   [the PowerShell procedure](#recovering-diagnostic-containers-with-powershell).
5. Remove both tasks:

   ```powershell
   Unregister-ScheduledTask -TaskName actions-runner-supervisor -Confirm:$false
   Unregister-ScheduledTask -TaskName actions-runner-weekly-rebuild -Confirm:$false
   ```

Afterwards `Get-ScheduledTask -TaskName 'actions-runner-*'` finds no task, no
`powershell.exe` process runs `supervisor.ps1`, and `docker ps -a` lists no job
container of the configured entries. Then continue from
[Installing marshal](#installing-marshal): restrict the token files, validate,
build, and start.

### What Was Verified of the PowerShell Path

CI runs the configuration validation test under PowerShell 7 and under Windows
PowerShell 5.1 (`windows-latest`), and the host tests under PowerShell 7. The
supervisor's runtime paths were exercised once, in the authoring session, on
PowerShell 7 on Linux, with a stub `docker` executable and a stub HTTP endpoint
standing in for GitHub; that harness is not committed, so nothing re-runs it. The
paths it covered are the registration request and its error reporting, the
environment hand-off to the container, stale-container cleanup, a slot's backoff, a
dead slot job's restart backoff, skipping an entry that cannot start, and the
shutdown stop. That run was made before entry names and container limits were
added, so the worker start-up and the stale-container cleanup as they are now are
covered only by the unit tests and by the check of the slot job's argument order.
Windows PowerShell 5.1 running the supervisor, Task Scheduler, and `icacls` are
described from the tools' documentation, not recorded as observed here. The same
holds for the container limits: a test checks the `docker run` arguments, and what
Docker Desktop enforces and `docker inspect` reports is described from Docker's
documentation.
