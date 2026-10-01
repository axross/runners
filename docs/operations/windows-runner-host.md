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
not observed.

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

Docker Desktop's **Settings, Resources** caps the CPU and memory containers may
use. Start from its defaults, and lower a repository's `slots` if builds starve
when several jobs run together. A job that finds no free slot queues rather than
fails.

## Repository Settings (Set by Hand)

Each target repository MUST require approval for workflow runs from outside
collaborators, under **Settings, Actions, General, Fork pull request workflows**.
A self-hosted runner executes whatever a workflow checks out, and an unapproved
fork's run would otherwise execute arbitrary code on this machine before anyone
reviews it. Do not list a public repository whose fork pull requests run on these
labels without that setting. A job selects this host by listing one of the
repository's custom labels in `runs-on`; a repository that does not list the
label never runs on it.

## The Fine-Grained Token

The supervisor asks GitHub for a single-use runner registration through
[`POST /repos/{owner}/{repo}/actions/runners/generate-jitconfig`](https://docs.github.com/en/rest/actions/self-hosted-runners),
which needs the **Administration** repository permission set to **Read and
write**. Create one **fine-grained personal access token per target repository**,
under **Settings, Developer settings, Fine-grained personal access tokens**:

- **Repository access:** only that one repository.
- **Permissions:** Administration, read and write, and nothing else.

A token that covers several repositories, or a classic token, would let a leak
register runners for all of them; the per-repository token keeps the blast radius
to one. The token MUST NOT be stored as a GitHub Actions secret: a secret that can
register runners for the automation that reads it is not a boundary. It lives only
in a file on this machine, holding the raw token text, at the `tokenPath` of the
repository's entry. Keep these files outside the checkout.

The file is read-only for the host user, so rotate a token by deleting the file
and creating it again with the new token, then run `register-scheduled-tasks.ps1`
again: a new file inherits its folder's permissions until the script resets and
restricts them. Do not edit the file in place. The supervisor reads the file on
every registration, so a rotated token applies to the next job in every slot with
no restart.

## The Host Configuration

The configuration is a JSON file kept outside the checkout; every script takes its
path as `-ConfigPath`. Start from
[`runner-host.example.json`](../../hosts/windows-docker-desktop/runner-host.example.json),
and see the [host README](../../hosts/windows-docker-desktop/README.md#host-configuration)
for every field. The rules that matter operationally:

- **One entry per target repository**, each with its own `tokenPath`, `slots`,
  custom `labels`, and `volumes`. An owner and repository pair listed twice is
  rejected. `tokenPath` is an absolute Windows path, a drive letter and backslash
  or a UNC path; a relative path is rejected because a scheduled task's working
  directory is not the checkout.
- **Unknown fields are rejected, not ignored.** A misspelled field such as `label`
  for `labels` fails validation naming the field, so a typo cannot silently drop
  a setting.
- **At least one custom label per entry.** Registrations always carry
  `self-hosted`, `linux`, and `x64`; the custom label is what a workflow puts in
  `runs-on` to name this host, so a job's runner is identifiable. Choose one label
  per repository, or a shared one only for repositories you would trust equally.
- **Names derive from `hostPrefix`, the owner, and the repository** unless an
  entry sets `prefix`, and the prefix is at most 64 characters, because it starts
  every runner name. GitHub documents no limit for runner names; 64 is this
  project's assumption, so a long owner and repository pair needs a short
  `prefix`. Two entries whose prefixes are equal, or where one is the
  other plus a hyphen, are rejected, because one would claim the other's
  containers and volumes. Across two supervisors on one machine, keep their
  `hostPrefix` values from being prefixes of each other; the script cannot see
  the other configuration.
- **Volumes belong to one entry.** A volume's name is the entry's prefix plus its
  `suffix`, so two entries never share one, and there is no way to configure it.

Check a configuration before using it. The command calls neither Docker nor
GitHub, prints each repository's labels, container prefix, and volume names, and
exits 1 naming the field of every problem:

```powershell
.\supervisor.ps1 -ConfigPath C:\path\to\runner-host.json -ValidateOnly
```

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

This registers two tasks for the signed-in user, named from `hostPrefix`:

- **`<hostPrefix>-supervisor`** runs `supervisor.ps1` at sign-in and is set to
  restart if it fails, as a backstop for the supervisor process exiting. Whether
  Task Scheduler restarts a task that ends with a non-zero exit code is not
  verified on a real host.
- **`<hostPrefix>-weekly-rebuild`** runs `rebuild-image.ps1` weekly, Sunday 03:00
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
   at start in the pinned version (`CommandSettings.cs` in `actions/runner`). The
   supervisor sets it only in the slot job's own process environment and passes
   `docker run -e ACTIONS_RUNNER_INPUT_JITCONFIG` with no value, so the client
   copies it from there. It is never on a command line, in a log line, or in an
   image layer. Anyone with access to the local Docker daemon can still read it
   from `docker inspect` until the container is removed, and the registration is
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
is also what makes them a cache-poisoning surface (see
[Security](../conventions/security.md#shared-runner-storage-is-a-cache-poisoning-surface)),
so each is declared here with the reason it exists and who can write to it. The
suffixes are those of
[`runner-host.example.json`](../../hosts/windows-docker-desktop/runner-host.example.json);
the volume's name is the entry's prefix plus the suffix.

| Suffix      | Mount path                   | Why it exists                                                                          |
| ----------- | ---------------------------- | -------------------------------------------------------------------------------------- |
| `toolcache` | `/opt/hostedtoolcache`       | Language runtimes that the `setup-*` actions install and `RUNNER_TOOL_CACHE` points at |
| `gradle`    | `/home/runner/.gradle`       | Gradle's downloaded dependencies and wrapper distributions                             |
| `cargo`     | `/home/runner/.cargo`        | Cargo's registry downloads and the binaries it installs                                |
| `rustup`    | `/home/runner/.rustup`       | Rust toolchains                                                                        |
| `ccache`    | `/home/runner/.cache/ccache` | Compiled object files that `ccache` reuses                                             |
| `npm`       | `/home/runner/.npm`          | npm's content-addressed package cache                                                  |

**Who can write to every one of them:** any job routed to the entry's labels.
That includes a pull request's run and a default-branch run of the same
repository, which share the entry's volumes; a pull request from a fork is the
same job, held only by the repository's approval setting under
[Repository Settings](#repository-settings-set-by-hand). Every job on one entry's
labels is therefore one trust level, and a volume is never shared with another
entry.

Most of these volumes hold content that a later job executes or links (runtimes,
installed binaries, wrapper distributions, object files), which the security
convention otherwise forbids. The
[bounded exception](../conventions/security.md#shared-runner-storage-is-a-cache-poisoning-surface)
allows it on this host for two conditions, and the accepted risk is that any job
on the entry's labels can poison a toolchain a later job of the same repository
executes:

- the entry is one repository, with its volumes used by no other entry; the
  configuration enforces this;
- **no workflow that holds a deployment secret runs on a label whose volumes a
  less-trusted job can write.** A repository with such a workflow either lists no
  volumes (`"volumes": []`, so each job starts from the image alone) or runs that
  workflow on a GitHub-hosted runner. The host cannot check this; the operator
  does.

## Isolation Between Repositories

The host enforces these for every entry, and a change that weakens one is a
[Security](../conventions/security.md#per-repository-isolation-on-a-runner-host)
finding:

- A registration is made with the entry's own token and carries its own labels,
  and a just-in-time runner takes jobs only from the repository it was registered
  for.
- Cache volumes are named per entry and mounted only into that entry's containers.
  At startup the supervisor creates them and resets their ownership with a
  short root container that has only that entry's volumes.
- Stale-container cleanup removes only containers whose names match the entry's own
  `<prefix>-<slot>-<timestamp>` pattern.
- Containers run unprivileged, with no Docker socket and no host path mounted,
  and `--pull never`, so a missing local image is an error and never a pull of a
  same-named public image.

A volume outlives its entry: if an entry is removed from the configuration, its
volumes and any stopped containers stay until removed by hand with
`docker volume rm` and `docker rm`.

## Health Checks

- **GitHub:** a healthy host shows, under each repository's **Settings, Actions,
  Runners**, `slots` runners, **Idle** when no job is queued and **Active** when
  one is running, because each slot registers a runner before it starts a
  container. Fewer than `slots`, and none in particular, mean the supervisor is not
  running or that repository's slots are failing; see Recovery.
- **Machine:** `docker ps` shows `slots` running containers per repository, named
  `<prefix>-<slot>-<timestamp>`, whether idle or busy, since an idle container is
  a runner waiting for a job. None means the same as above.
  `Get-ScheduledTask <hostPrefix>-supervisor | Select State` reports `Running`.
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

## Network Egress (Accepted Risk)

Containers start on Docker Desktop's default network, which gives them the same
route to the machine's local network that any process on the machine has. A job
can therefore reach a router, a NAS, or another machine there, none of which a
GitHub-hosted runner could. The maintainer has accepted this risk for now; see
[Security](../conventions/security.md#lan-egress-from-runner-containers-accepted-risk).
Blocking it, through a host firewall rule for the containers' subnet or a custom
Docker network, is a follow-up and not implemented. Run this host only on a
network you would trust the listed repositories' workflows with.

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
  hand, `docker ps -a --filter "name=<prefix>-"` lists candidates and
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
