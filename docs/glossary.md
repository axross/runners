# Glossary

The vocabulary this project uses, grouped by the domain that defines it. A
`specs/` document, once the project has one, pairs with a heading of the same
name here.

**A general word MUST NOT carry a complex, domain-specific meaning on its own;
where it would, the name MUST be a compound that bounds the meaning instead** -
`Runner Host`, not `Machine`; `Runner Image`, not `Image` alone where a container
image of any other kind could be meant. The one exception is an identifier scoped
tightly enough that its surroundings already disambiguate it, such as a local
variable or a private helper.

## Runners

**Runner** - a GitHub Actions self-hosted runner: the process that takes a
workflow job from GitHub and executes it.

**Ephemeral Runner** - a runner that takes exactly one job and is then removed,
so nothing a job leaves behind carries into the next. The opposite is a
persistent runner, which this project does not use.

**JIT Config** - the just-in-time runner configuration GitHub returns when a
runner is registered through the API for a single repository. It is an encoded
credential: it is passed to the runner at start, never stored in the repository,
and is usable once.

**Runner Image** - the container image a runner runs in, built from a Dockerfile
under `images/<name>/`, such as `images/actions-runner/`. It carries the runner
and the tools jobs need, and declares the **Image Contract Version** it was built
for.

**Image Contract Version** - the positive integer a **Runner Image** declares in
its `com.github.axross.runners.contract` label, naming the run contract it was
built for: the argument lists, launcher, and labels the **CLI** runs it with, and
what they need from the image. The **CLI** reads its own from the Dockerfile it
embeds and runs jobs only in an image that declares the same. See
[operations/cli.md](./operations/cli.md#the-image-contract).

**Runner Host** - the machine that builds runner images and starts runner
containers, together with the host configuration under `hosts/<platform>/`. The first
platform is Windows with Docker Desktop, under `hosts/windows-docker-desktop/`.

**Host Configuration** - the machine-local JSON file, kept outside the repository,
that names a runner host's image and lists its **Execution Pools** for
**Target Repositories**. The
**CLI** reads it, checked against a **Host Profile**.

**Target Repository** - a GitHub repository a runner host serves. Its
**Execution Pools** share one repository-scoped token file.

**Execution Pool** - one entry of the **Host Configuration**, with its own
**Slots**, registration labels, and
container, runner, and cache-volume namespace.

**Routing Label** - the label a workflow uses in `runs-on` to select an
**Execution Pool**. Missing or empty host `labels` default to the host's own
label, `axpc` or `axmc`; a non-empty list supplies that pool's routing labels
instead. In a TOML host configuration the required `routing_label` key supplies
the default, so no host's label is built in. See
[operations/cli.md](./operations/cli.md#schema-version-1).

**Custom Label** - an optional runner label an entry of the **Host Configuration**
lists beside `self-hosted`, `linux`, and `x64`. A workflow
can use a custom label to select that pool for its **Target Repository**.

**Slot** - one concurrent runner position on a runner host. A host with two slots
can run two jobs at once; each slot starts a fresh ephemeral runner when its
previous one exits.

**Supervisor** - the host-side process that keeps each slot of every **Execution
Pool** filled: it requests a **JIT Config** with the target repository's token,
starts a runner container, waits for it to exit, and repeats. On axpc it is the
**CLI**'s `start`.

**CLI** - this project's Rust command line tool, under `cli/`. `validate` checks a
**Host Configuration**; `build` builds its **Runner Image** from the definition
the binary embeds; `start` runs a host's pools in the foreground as the
**Supervisor**, and `stop`, `restart`, `status`, and `diagnostics` talk to it.

**CLI Release** - a `v<version>` tag on main and the GitHub release that carries the
**CLI**'s two binaries, the unsigned Windows MSI and the signed and notarized macOS
pkg that carry them, and `SHA256SUMS`. The macOS binary is signed too. The maintainer
starts one by dispatching `release.yaml`, and semantic-release picks the version from
the commit subjects since the last `v*` tag. See
[operations/development-workflow.md](./operations/development-workflow.md#cutting-a-release).

**CLI Installer** - the Windows MSI, which is not code-signed, or the macOS pkg,
which is signed with Developer ID and notarized, of a **CLI Release**. It holds the
**CLI**'s binary unchanged and an operator opens it to install it. An
optional **Check Block** checks the download first, and the installer's version is
the release's `X.Y.Z`, or `0.0.0` for the dry run's. See
[operations/cli.md](./operations/cli.md#installing-updating-and-rolling-back).

**Release Environment** - the GitHub Environment named `release`, which holds the
macOS signing secrets and the `MACOS_TEAM_ID` variable. It admits `main` only and
needs the maintainer's approval for each run, and only `sign-macos` declares it. The
maintainer creates and changes it; agents never do. See
[operations/cli.md](./operations/cli.md#the-signing-contracts).

**Signing Identifier** - `app.axross.marshal`, the identifier one code object is signed
under: the **CLI**'s macOS binary until the Windows app's installer replaces the
command line in the release, and the Mac app once it ships. The macOS pkg is built
with it too. It and the team are fixed, so the designated requirement stays the
same from release to release, which is what the Keychain trust of later work
matches. See
[operations/cli.md](./operations/cli.md#the-signing-contracts).

**Check Block** - the optional block in the **CLI** document that checks a
downloaded **CLI Installer** against `SHA256SUMS`, the tag's commit on main, the
attestation, and the version of the binary inside it, and installs nothing. It
ends with `refused:` and its cause when a check fails.

**Placeholder Version** - `0.0.0-dev`, the version the **CLI**'s `Cargo.toml` and
`Cargo.lock` hold on main. A release build writes the release version over it in
the runner only, so the version is never committed. See
[operations/cli.md](./operations/cli.md#releases).

**Instance State** - where the one running `start` on a machine stands: running,
stopping, stopped with a reason, or resuming; or `building`, while a `build` holds
the instance lock instead. `status` reports it, and it decides which requests the
instance accepts. See
[operations/cli.md](./operations/cli.md#instance-state).

**Runtime Core** - the part of `start` that prepares the **Execution Pools** and
runs every **Slot**'s loop until a stop. The instance restarts it after a failure;
see [operations/cli.md](./operations/cli.md#the-instance-layer).

**Resident Agent** - the **CLI**'s long-running form, started stopped or running
and told what to do over its channel: it stops and starts all runners, builds the
**Runner Image**, quits, and stays up when its configuration, image or pools fail.
It takes the instance's lock, channel and log directory, so it and `start` never
run on one machine together. The **Marshal App**'s executable runs it in its agent
mode. Not to be confused with an **Agent Host**. See
[operations/cli.md](./operations/cli.md#the-agent).

**Marshal App** - the Windows notification-area app under `app/`: one executable that
shows the icon, its menu and the main window and, started again with `agent` as its
first argument, runs the **Resident Agent** in a separate hidden process that outlives
it. The app is a client of **Protocol Version 2** only; it never holds the lock, reads
the configuration, touches the **Token Store** or runs Docker. It is built in CI and
not yet installed or released. See [operations/app.md](./operations/app.md).

**Token Store** - where the **Resident Agent** keeps each **Target Repository**'s
token, by the repository's lower-case `owner/name`: the Windows Credential Manager
on Windows, with local persistence, and none yet elsewhere, where every pool is
skipped. See [operations/cli.md](./operations/cli.md#the-token-store).

**Protocol Version 2** - the line-based JSON messages the **Resident Agent**
serves on the instance's channel: `hello`, requests and responses, a subscribed
`state` topic with events, heartbeats and `ending`. A first line that is not a
`hello` is refused, so version 1's requests never reach the agent. See
[operations/cli.md](./operations/cli.md#protocol-version-2).

**Snapshot** - the value of the **Resident Agent**'s `state` topic and of its
`status` method: its state and reason, its pools and runners, its images and what a
stop left behind, from local state, cut to 64 KiB. It never holds a token. See
[operations/cli.md](./operations/cli.md#the-snapshot).

**Diagnostic Container** - a job container of an **Execution Pool** with
`diagnostics` on. It runs the runner under the image's collector, carries the
`runners.diagnostic-lifecycle=1` label, and has no `--rm`, so it stays after its job.
The **Supervisor** finalises it: it exports the collector's evidence into a bundle
in the pool's private sink, then removes it. Until then it is retained with the
settings it ran with. See
[operations/cli.md](./operations/cli.md#diagnostics).

**Runtime Seam** - the boundary in the **CLI** between the **Runtime Core** and
whatever a job runs in. The core keeps the configuration, registration, backoff and
the stop sequence; the runtime behind the seam waits for its engine, checks its
image, prepares and releases a job's host, runs the job, and exports diagnostics. A
pool's operating system, on the **Host Profile**'s host, selects the runtime: a
Docker Desktop container on axpc, a **Job VM** for a Linux pool on axmc. See
[operations/cli.md](./operations/cli.md#runtime-seam).

**Golden VM** - the Tart Ubuntu VM that `build --profile axmc` makes on the macOS
host and every **Job VM** is cloned from. It holds Docker Engine, the launcher and
the **Runner Image**, and its name records its golden VM version, the **Image
Contract Version**, the runner version and the build time, so they are read without
booting it. Only `build` deletes it. See
[operations/macos-runner-host.md](./operations/macos-runner-host.md#the-golden-vm-contract).

**Job VM** - the VM that runs one job of an axmc Linux **Execution Pool**: a clone
of the **Golden VM**, named `marshal-job-<pool>-<slot>-<time>`, booted on
**Softnet** with no shared directory, clipboard, audio, USB accessories or Rosetta,
and deleted when its job ends. A **Slot**'s Job VM is booted and idle while its
runner waits for a job. See [conventions/security.md](./conventions/security.md#a-linux-job-on-axmc-runs-in-a-per-job-vm).

**VM-Local Docker Socket** - the Docker socket, `/var/run/docker.sock`, of the Docker
Engine inside a **Job VM**, which the VM runtime mounts into that VM's runner
container from **Image Contract Version** 2, so a job step can run `docker`. It
belongs to a VM that one job uses and that is deleted afterwards, never to a host,
and the Docker runtime on axpc mounts no socket. See
[conventions/security.md](./conventions/security.md#the-vm-local-docker-socket).

**Build VM** - the temporary VM, named `marshal-build-linux-<time>`, in which
`build --profile axmc` makes a new **Golden VM**. It is renamed to the golden name
only when complete, and a failed or interrupted build deletes it, so the previous
Golden VM stays in place.

**Softnet** - the program Tart runs to filter a VM's network. It lets a **Job VM**
send only to globally routable IPv4 addresses and to its gateway, and `marshal`
passes it rules that leave the gateway only DNS and block all multicast. Its
privilege is a SUID bit on its binary, which `marshal` checks before every boot and
never sets. See
[operations/macos-runner-host.md](./operations/macos-runner-host.md#setting-up-softnet).

**Primary Network** - on the macOS host, the interface that holds the IPv4 default
route, that route's router, and the router's hardware address. The instance records
it when it starts running and stops every runner when it differs on two checks in a
row. It is IPv4 only, because **Softnet** carries only IPv4, and it names
interfaces, never network names or addresses.

**Host Profile** - the host a **Host Configuration** is checked for: `axpc`, the
Windows machine with Docker Desktop, or `axmc`, the macOS machine. A profile fixes
the fields, the platform labels, the **Routing Label**, and the path form. See
[operations/cli.md](./operations/cli.md).

**Shared Fixtures** - the configuration files under `tests/host-configuration/`
that the **CLI**'s tests read, with one `expectations.json` recording what each
must give.

**Shared Volume** - storage, such as a named Docker volume for a package cache,
mounted into more than one runner container over time. It is the surface the
cache-poisoning threat model in [conventions/security.md](./conventions/security.md)
covers.

## Agent Hosts

**Agent Host** - a long-running environment where an AI coding agent is reachable
from outside, configured under the planned `agent-hosts/{claude-code,amp}/`. The
first planned environment is WSL 2. Not to be confused with the authoring host
below.

**Remote Control** - Claude Code's `claude remote-control` mode, which lets a
session running on an agent host be driven from another device.

**Amp Runner** - an Amp process started with `amp --no-tui --runner-id`, which
waits for work on an agent host rather than showing an interface.

## Change Loop

**Authoring Host** - the agent product a change is written with, Claude Code,
Codex, or Amp, or "manual" when no agent is involved. It determines the
independent-review provider and is recorded with that provider so a run can be
resumed.

**Standing Delivery Grant** - the maintainer's authorization, adopted in
[operations/development-workflow.md](./operations/development-workflow.md), for a
named set of delivery effects that every run holds without asking.

**Tracking Issue** - the GitHub issue whose body holds a change's plan. The plan
gate is the maintainer's approval of that plan.

**Plan-Gate Delivery Confirmation** - the answer a Claude Code run collects
with every plan-approval question: delivery of the run's draft pull request, the
Claude review request on it, and watching its activity. See
[operations/agent-sessions.md](./operations/agent-sessions.md).

**Amp Review Child Thread** - a fresh Amp thread an Amp run starts for one
independent-review round; it posts its own review on the pull request and is
archived afterwards when the plan-gate archive confirmation allows it. See
[operations/development-workflow.md](./operations/development-workflow.md).

**Plan-Gate Archive Confirmation** - the human's answer, collected at the plan
gate, or before archiving in a run without one or in a resumed session, on
whether an Amp run archives its review child threads. See
[operations/agent-sessions.md](./operations/agent-sessions.md).

**Run State** - the recoverable record of a change loop's progress, which a
fresh session reads to resume the run. See
[operations/development-workflow.md](./operations/development-workflow.md).

**Review Coverage Evidence** - the facts a run keeps separate when it assesses
an independent review: which policy was available, what input the reviewer is
known to have consumed, what it reported, and what it published. See
[operations/development-workflow.md](./operations/development-workflow.md).

**Branch Naming Rule** - the rule that names a working branch after the issue
that is the source of the change, or marks the change as having none, so a
branch name identifies where the change came from rather than which agent host
wrote it. See
[operations/development-workflow.md](./operations/development-workflow.md).

**Agent Marker** - the `<!-- ai-agent -->` comment that marks an issue or comment
as written by an agent.

**Installed Skill** - a directory under `.claude/skills/` copied from
`axross/skills` and pinned in `skills-lock.json`. It is generated and never
edited by hand; see [operations/agent-skills.md](./operations/agent-skills.md).
