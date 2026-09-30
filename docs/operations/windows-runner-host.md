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

**Verification status.** The image builds in CI on every pull request. The host
scripts have been exercised on PowerShell 7 on Linux against a stub `docker` and
a stub GitHub endpoint, which covers the registration, environment, cleanup,
backoff, and shutdown paths, and the configuration validation has a test in CI.
A run on Windows PowerShell 5.1 and Docker Desktop, serving a real repository, has
not been done yet; it is the maintainer's post-merge check, and until it is done
the Windows-specific steps below (Task Scheduler, `icacls`, Docker Desktop's
settings) are described from the tools' documentation, not observed.

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

Rotate a token by replacing the file's contents and running
`register-scheduled-tasks.ps1` again, which re-tightens the file's permissions.
The supervisor reads the file on every registration, so a rotated token applies to
the next job in every slot with no restart.

## The Host Configuration

The configuration is a JSON file kept outside the checkout; every script takes its
path as `-ConfigPath`. Start from
[`runner-host.example.json`](../../hosts/windows-docker-desktop/runner-host.example.json),
and see the [host README](../../hosts/windows-docker-desktop/README.md#host-configuration)
for every field. The rules that matter operationally:

- **One entry per target repository**, each with its own `tokenPath`, `slots`,
  custom `labels`, and `volumes`. An owner and repository pair listed twice is
  rejected.
- **At least one custom label per entry.** Registrations always carry
  `self-hosted`, `linux`, and `x64`; the custom label is what a workflow puts in
  `runs-on` to name this host, so a job's runner is identifiable. Choose one label
  per repository, or a shared one only for repositories you would trust equally.
- **Names derive from `hostPrefix`, the owner, and the repository** unless an
  entry sets `prefix`. Two entries whose prefixes are equal, or where one is the
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
build leaves the previous image in place.

## Registering the Scheduled Tasks

From an elevated PowerShell prompt, once the token files exist and the image has
been built:

```powershell
.\register-scheduled-tasks.ps1 -ConfigPath C:\path\to\runner-host.json
```

This registers two tasks for the signed-in user, named from `hostPrefix`:

- **`<hostPrefix>-supervisor`** runs `supervisor.ps1` at sign-in and restarts it
  if it exits, so a Docker Desktop restart that takes the supervisor down does not
  leave the machine without runners until the next sign-in.
- **`<hostPrefix>-weekly-rebuild`** runs `rebuild-image.ps1` weekly, Sunday 03:00
  by default, and runs a missed rebuild when the machine is next available.

Both run as the current user at the limited run level, never elevated. The script
also restricts every configured token file to that user: it removes inherited
permissions, then grants the user read access. Run it again after editing the
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
job to completion is replaced at once. A slot job that dies is restarted. Ctrl+C
in the supervisor's window stops every slot's job and `docker stop`s the current
containers.

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

- **GitHub:** a repository's **Settings, Actions, Runners** lists a runner only
  between its registration and the end of its one job, so an idle host shows none.
- **Machine:** `docker ps` shows up to `slots` containers per repository, named
  `<prefix>-<slot>-<timestamp>`, one per busy slot.
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
pull), and whatever each repository's jobs reach on a GitHub-hosted runner. No
inbound port is needed.

## Recovery

- **A container left by an unclean stop** (a forced stop of the supervisor task, a
  crash, a restart mid-job) is removed by the next supervisor start, which removes
  containers matching each entry's own name pattern before starting its slots. By
  hand, `docker ps -a --filter "name=<prefix>-"` lists candidates and
  `docker rm -f <name>` removes one.
- **Docker Desktop restarts mid-job:** the container is gone, the job fails on
  GitHub's side, and re-running the workflow is the recovery. The supervisor task
  restarts itself within a minute.
- **Stopping the supervisor:** Ctrl+C in its window is the clean path. Ending the
  task in Task Scheduler, or a shutdown, skips it and leaves running containers to
  finish or to be removed at the next start; a job in flight fails either way.
- **A rejected configuration:** the supervisor prints every offending field and
  exits before starting anything; fix the file and rerun it, or run it with
  `-ValidateOnly` first.
- **A slot logging registration failures:** the message carries GitHub's status. A
  401 or 404 usually means the token expired or lacks Administration access to that
  repository; replace the file. The other repositories' slots are unaffected.
