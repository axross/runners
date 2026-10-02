# Windows Runner Host

How to run ephemeral GitHub Actions runners for several repositories from one
Windows machine with Docker Desktop, from an unconfigured machine to a healthy
host: prerequisites, the per-repository token, the host configuration, the
scheduled tasks, health checks, updating, and recovery. It covers the machine and
the container that run a job; what a job installs and builds is each consumer
repository's own concern. The files are under
[`hosts/windows-docker-desktop/`](../../hosts/windows-docker-desktop/README.md)
and [`images/actions-runner/`](../../images/actions-runner/README.md), and the
trust rules they are reviewed against are in
[Security](../conventions/security.md).

**Verification status.** CI builds the image and runs a smoke test on it for every
pull request, and runs the configuration validation test under PowerShell 7 and
under Windows PowerShell 5.1 (`windows-latest`). The supervisor's runtime paths
were exercised once, in the authoring session, on PowerShell 7 on Linux, with a
stub `docker` executable and a stub HTTP endpoint standing in for GitHub; that
harness is not committed, so nothing re-runs it. The paths it covered are the
registration request and its error reporting, the environment hand-off to the
container, stale-container cleanup, a slot's backoff, a dead slot job's restart
backoff, skipping an entry that cannot start, and the shutdown stop. Windows
PowerShell 5.1 running the supervisor, Docker Desktop, Task Scheduler, and
`icacls` are unverified until the maintainer's post-merge check on a real host;
the steps below that involve them are described from the tools' documentation,
not observed. The same holds for the container limits: a test checks the
`docker run` arguments, and what Docker Desktop enforces and `docker inspect`
reports is described from Docker's documentation.

## Prerequisites

- **Windows 11 Pro, version 23H2 or later**, the minimum for Docker Desktop's WSL 2
  backend on Windows 11 (see
  [Docker's system requirements](https://docs.docker.com/desktop/setup/install/windows-install/)),
  with hardware virtualization enabled and the WSL 2 feature on.
- **The machine stays powered on, signed in, and online.** Locked is fine; signed
  out is not, because the supervisor task starts at sign-in. A job whose labels
  match no runner waits in the queue until GitHub cancels it.
- **Docker Desktop** with the WSL 2 backend. This procedure does not use the
  Hyper-V backend or a Docker Engine inside a WSL distribution.
- **A checkout of this repository** in a stable location. The scheduled tasks run
  the scripts from it, so moving or deleting it breaks them until
  `register-scheduled-tasks.ps1` is run again.
- **Windows PowerShell 5.1** (shipped with Windows) or PowerShell 7 to run the
  scripts. The scheduled tasks use `powershell.exe`.

## Installing Docker Desktop

1. Install Docker Desktop and accept the WSL 2 backend.
2. Under **Settings, General**, enable **Start Docker Desktop when you sign in to
   your computer**, which is off by default. The supervisor waits for the Docker
   daemon, so it does not matter which of the two starts first.
3. Confirm `docker info` exits 0 in a terminal.

With the WSL 2 backend, containers share the WSL 2 virtual machine's CPU and
memory, and Docker Desktop's **Settings, Resources** does not cap them. To cap
them, set `memory` and `processors` under `[wsl2]` in
`%UserProfile%\.wslconfig`, which applies to every WSL 2 distribution on the
machine, then run `wsl --shutdown` so the virtual machine restarts with the
limits. Start from the defaults, and lower a repository's `slots` if builds
starve when several jobs run together. A job that finds no free slot queues
rather than fails.

The per-container `cpus` and `memoryGb` limits of the host configuration bind
inside this virtual machine and reserve nothing in it. Slots times the limits,
summed over every entry, can exceed the virtual machine's allocation, and the
jobs then compete for what it has.

## Repository Settings (Set by Hand)

For each target repository, under **Settings, Actions, General, Fork pull
request workflows**, choose the option that requires approval for workflow runs
from outside collaborators. Do not list a public repository whose fork pull
requests run on these labels until that is set. The requirement and its reason
are in the Per-Repository Isolation on a Runner Host section of
[Security](../conventions/security.md). A job selects this host by listing one of
the repository's custom labels in `runs-on`; a repository that does not list the
label never runs on it.

## The Fine-Grained Token

The supervisor asks GitHub for a single-use runner registration through
[`POST /repos/{owner}/{repo}/actions/runners/generate-jitconfig`](https://docs.github.com/en/rest/actions/self-hosted-runners),
which needs the **Administration** repository permission set to **Read and
write**. Create one **fine-grained personal access token per target repository**,
under **Settings, Developer settings, Fine-grained personal access tokens**:

- **Repository access:** only that one repository.
- **Permissions:** Administration, read and write, and nothing else.

Save each token in a file on this machine, holding the raw token text, at the
`tokenPath` of the repository's entry, and keep these files outside the checkout.
The token rules are in the Per-Repository Isolation on a Runner Host section of
[Security](../conventions/security.md).

The file is read-only for the host user, so rotate a token by deleting the file
and creating it again with the new token, then run `register-scheduled-tasks.ps1`
again: a new file inherits its folder's permissions until the script resets and
restricts them. Do not edit the file in place. The supervisor reads the file on
every registration, so a rotated token applies to the next job in every slot with
no restart.

## Allowing the Scripts to Run

Windows PowerShell 5.1's default execution policy refuses local script files, so
the `.\` commands in the sections below fail until the prompt allows them. In each
PowerShell window you run them from, including the elevated one, run this once
before the first command:

```powershell
Set-ExecutionPolicy -Scope Process -ExecutionPolicy Bypass
```

It lasts only for that window and changes no machine or user setting. The
scheduled tasks pass `-ExecutionPolicy Bypass` to their own processes, so they
need nothing from you.

## The Host Configuration

The configuration is a JSON file kept outside the checkout; every script takes its
path as `-ConfigPath`. Start from
[`runner-host.example.json`](../../hosts/windows-docker-desktop/runner-host.example.json),
and see the Host configuration section of the
[host README](../../hosts/windows-docker-desktop/README.md) for every field. The rules that matter operationally:

- **One entry per target repository**, each with its own `tokenPath`, `slots`,
  custom `labels`, and `volumes`. An owner and repository pair listed twice is
  rejected, and so are two entries that name the same token file, compared
  without regard to case. `tokenPath` is an absolute Windows path, a drive
  letter and backslash or a UNC path; a relative path is rejected because a
  scheduled task's working directory is not the checkout.
- **Unknown fields are rejected, not ignored.** A misspelled field such as `label`
  for `labels` fails validation naming the field, so a typo cannot silently drop
  a setting.
- **At least one custom label per entry.** Choose one label per repository, or a
  shared one only for repositories you would trust equally. What a registration
  carries is in the Per-Repository Isolation on a Runner Host section of
  [Security](../conventions/security.md).
- **Every entry has a `name`** that starts its container and runner names,
  `<name>-<index>-<timestamp>` with a 1-based slot index, and its volume names,
  `<name>-<suffix>`. It is lowercase letters, digits, and hyphens, starting with
  a letter or digit, and at most 64 characters, because it starts every runner
  name. GitHub documents no limit for runner names; 64 is this project's
  assumption. Choose it short and recognisable. Two entries whose names are equal,
  or where one is the other followed by a hyphen, are rejected, and the error
  names both. The collision rule is in the Per-Repository Isolation on a Runner
  Host section of [Security](../conventions/security.md).
- **One configuration per machine is the supported setup**, because the scheduled
  tasks have fixed names and registering a second configuration replaces the
  first's tasks.
- **Every job container has CPU and memory limits.** `cpus` is a number above 0
  and at most 64, 2 when absent. `memoryGb` is an integer from 1 to 256, 8 when
  absent. Each job container of the entry starts with `--cpus` and `--memory`
  set to them and `--memory-swap` equal to `--memory`, so it gets no swap beyond
  its memory. Docker takes `--cpus` as a decimal number of CPUs and `--memory` as
  a size with a unit suffix such as `g`; see
  [Docker's resource constraints](https://docs.docker.com/engine/containers/resource_constraints/).
  The limit applies to each container, not to an entry's slots together or to the
  host. The short container that resets volume ownership at startup has none.

Check a configuration before using it. The command calls neither Docker nor
GitHub, prints each repository's labels, name, container name pattern, CPU and
memory limits, and volume names, and exits 1 naming the field of every problem:

```powershell
.\supervisor.ps1 -ConfigPath C:\path\to\runner-host.json -ValidateOnly
```

## Moving to the New Configuration Format

A host whose configuration still has `hostPrefix` or a per-entry `prefix` is
rejected, naming the field. To move it:

1. Note the `hostPrefix`, and each entry's prefix, `<hostPrefix>-<owner>-<repository>`
   in lowercase unless the entry set `prefix`. Stop the supervisor with Ctrl+C in
   its window, then end and disable the `<hostPrefix>-supervisor` and
   `<hostPrefix>-weekly-rebuild` tasks in Task Scheduler.
2. Rewrite the file: give every entry a `name`, and delete `hostPrefix` and every
   `prefix`. An entry whose `name` equals its previous prefix, which is possible
   only when the prefix is a valid `name`, keeps its volumes; any other `name`
   starts from empty volumes.
3. Run `supervisor.ps1 -ValidateOnly` on the file until it passes.
4. From an elevated prompt, run `register-scheduled-tasks.ps1`, which registers
   `actions-runner-supervisor` and `actions-runner-weekly-rebuild`, then remove the
   earlier tasks:

   ```powershell
   Unregister-ScheduledTask -TaskName <hostPrefix>-supervisor -Confirm:$false
   Unregister-ScheduledTask -TaskName <hostPrefix>-weekly-rebuild -Confirm:$false
   ```

5. Remove what the earlier names left, skipping an entry whose `name` equals its
   previous prefix. Remove a volume only once no container uses it:

   ```powershell
   docker ps -a --filter "name=<previous prefix>-"
   docker rm -f <container name>
   docker volume rm <previous prefix>-<suffix>
   ```

6. Start the `actions-runner-supervisor` task, or sign out and in.

Job containers had no CPU or memory limit before. Without `cpus` and `memoryGb`,
each is now capped at 2 CPUs and 8 GB, so set both on an entry whose jobs need
more.

## Building the Runner Image

From `hosts\windows-docker-desktop\` in the checkout, once, and after every
change to `images\actions-runner\`:

```powershell
.\rebuild-image.ps1 -ConfigPath C:\path\to\runner-host.json
```

It builds the checkout's [`Dockerfile`](../../images/actions-runner/Dockerfile) under
the configured `imageName`, with `docker build --pull --no-cache`. The runner
version and base image digest come from that Dockerfile; the script fetches no
newer runner and does not update the checkout. The cache is ignored so that the
operating system packages are installed again and pick up their updates; a failed
build leaves the previous image in place. After a successful build the script
removes the earlier images it built that the new build left untagged, so weekly
rebuilds do not fill the disk. The prune is limited to images carrying the build
label the script sets, so it does not touch other dangling images or images built
by hand; an image from before the label existed stays until removed with
`docker image rm`. A failed prune is a warning, not a failed rebuild.

## Registering the Scheduled Tasks

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
  by default, and runs a missed rebuild when the machine is next available.

Both run as the current user at the limited run level, never elevated. The script
also restricts every configured token file to that user: it resets the file's
permissions to its folder's defaults, removes inherited permissions, then grants
the user read access, so an entry added to the file by hand does not survive. Run it again after editing the
configuration, replacing a token file, or moving the checkout. Sign out and in, or
start the supervisor task from Task Scheduler, to start it now.

## How a Container Picks Up a Job

For each repository, the supervisor runs that entry's `slots` as separate
background jobs. A slot loops forever:

1. Wait for Docker to answer `docker info`.
2. Request a single-use registration from GitHub with that repository's token, for
   a uniquely named runner carrying the entry's labels.
3. Start a throwaway container from the image with the entry's volumes and
   `/home/runner/run.sh`. The registration reaches the runner through the
   `ACTIONS_RUNNER_INPUT_JITCONFIG` environment variable, which the runner reads
   at start in the pinned version (`CommandSettings.cs` in `actions/runner`).
   [Security](../conventions/security.md) owns how the supervisor hands it to
   Docker. Anyone with access to the local Docker daemon can still read it from
   `docker inspect` until the container is removed, and the registration is
   single-use.
4. The runner takes one matching job and exits, and `--rm` removes the container.
   The slot then returns to step 1.

A failed registration request or a container that exits non-zero delays the next
attempt, doubling from 5 seconds to a 300-second ceiling; a container that ran a
job to completion is replaced at once. A slot job that dies is restarted after its
own delay on the same schedule, and the delay starts over once a job has run for
ten minutes before dying. Ctrl+C in the supervisor's window `docker stop`s the
current containers, then stops every slot's job.

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
other entry mounts them. At startup the supervisor creates the entry's volumes
and resets their ownership with a short root container that mounts only those
volumes.

An entry keeps only the volumes in the table above. The host does not enforce
that: it accepts any suffix and mount path, so the operator does. A volume
for anything else, such as a compiler or build-system cache whose contents a later
job links or executes without re-verifying them, is unsupported because no rule in
[Security](../conventions/security.md) allows it.

Before listing volumes for a repository, check which of its workflows hold a
deployment secret, and whether the repository accepts pull requests from forks.
If either applies, the entry lists `"volumes": []`, so each job starts from the
image alone, or those workflows, including the fork pull request workflows, run on
a GitHub-hosted runner. That includes `npm` and every other volume in the table,
not only the toolchain volumes. The host cannot check this; the operator does. The
rule and its reason are in the Shared Runner Storage Is a Cache-Poisoning Surface
section of [Security](../conventions/security.md).

## Removing a Repository

A volume outlives its entry: if an entry is removed from the configuration, its
volumes and any stopped containers stay until removed by hand with
`docker volume rm` and `docker rm`. The isolation rules the host enforces between
repositories are in [Security](../conventions/security.md).

## Health Checks

- **GitHub:** a healthy host shows, under each repository's **Settings, Actions,
  Runners**, `slots` runners, **Idle** when no job is queued and **Active** when
  one is running, because each slot registers a runner before it starts a
  container. Fewer than `slots`, and none in particular, mean the supervisor is not
  running or that repository's slots are failing; see Recovery.
- **Machine:** `docker ps` shows `slots` running containers per repository, named
  `<name>-<index>-<timestamp>`, whether idle or busy, since an idle container is
  a runner waiting for a job. None means the same as above.
  `Get-ScheduledTask actions-runner-supervisor | Select State` reports `Running`.
  `docker inspect` on a container shows its limits under `HostConfig`: `NanoCpus`
  is the CPU limit in billionths of a CPU, and `Memory` and `MemorySwap` are both
  `memoryGb` gigabytes in bytes.
- **End to end:** a workflow run whose `runs-on` lists the custom label starts
  executing rather than sitting queued.

## Updating the Runner

The host does not update its own checkout. To move to a newer runner or image:

1. Pull the checkout by hand, normally after merging the Dependabot pull request
   that bumps the `FROM` line in `images/actions-runner/Dockerfile`.
2. Run `rebuild-image.ps1`, or wait for the weekly task. Containers already running
   finish their job on the old image; the next one starts on the new image.

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
image build, run weekly and on demand, also reaches the Ubuntu package archives
that the base image's apt sources name, over port 80 or 443 (by default
`archive.ubuntu.com` and `security.ubuntu.com`; check the base image's sources
before filtering). The Dockerfile itself needs no access to Docker Hub. No inbound
port is needed.

## Recovery

- **A container left by an unclean stop** (a forced stop of the supervisor task, a
  crash, a restart mid-job) is removed by the next supervisor start, which removes
  containers matching each entry's own name pattern before starting its slots. By
  hand, `docker ps -a --filter "name=<name>-"` lists candidates and
  `docker rm -f <name>` removes one.
- **Docker Desktop restarts mid-job:** the container is gone, the job fails on
  GitHub's side, and re-running the workflow is the recovery. The supervisor
  process keeps running: each slot waits for `docker info` to answer, backs off
  after the failed run, and resumes by itself when Docker is back, with no task
  restart. The task's restart setting is only a backstop for the supervisor
  process exiting.
- **Stopping the supervisor:** Ctrl+C in its window is the clean path. Ending the
  task in Task Scheduler, or a shutdown, skips it and leaves running containers to
  finish or to be removed at the next start; a job in flight fails either way.
- **One repository's entry will not start:** a missing or empty token file, a
  volume that cannot be created, or stale containers that cannot be listed skip
  that entry only. The supervisor logs a warning naming the entry and the reason
  (never a token), gives that entry no slots, and serves the others. It exits 1
  only when no entry can start. Fix the cause and restart the supervisor task; an
  entry skipped at start is not retried while the supervisor runs.
- **A rejected configuration:** the supervisor prints every offending field and
  exits before starting anything; fix the file and rerun it, or run it with
  `-ValidateOnly` first.
- **A slot logging registration failures:** the message carries GitHub's status. A
  401 or 404 usually means the token expired or lacks Administration access to that
  repository; replace the file. The other repositories' slots are unaffected.
