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
containers, together with the scripts under `hosts/<platform>/`. The first
platform is Windows with Docker Desktop, under `hosts/windows-docker-desktop/`.

**Host Configuration** - the machine-local JSON file, kept outside the repository,
that names a runner host's image and lists its **Execution Pools** for
**Target Repositories**. The
**Supervisor** and the host's other scripts read it. The **CLI** reads it too,
checked against a **Host Profile**.

**Target Repository** - a GitHub repository a runner host serves. Its
**Execution Pools** share one repository-scoped token file.

**Execution Pool** - one entry of the **Host Configuration**, with its own
**Slots**, registration labels, and
container, runner, and cache-volume namespace.

**Routing Label** - the label a workflow uses in `runs-on` to select an
**Execution Pool**. Missing or empty host `labels` default to the host's own
label, `axpc` or `axmc`; a non-empty list supplies that pool's routing labels
instead.

**Custom Label** - an optional runner label an entry of the **Host Configuration**
lists beside `self-hosted`, `linux`, and `x64`. A workflow
can use a custom label to select that pool for its **Target Repository**.

**Slot** - one concurrent runner position on a runner host. A host with two slots
can run two jobs at once; each slot starts a fresh ephemeral runner when its
previous one exits.

**Supervisor** - the host-side process that keeps each slot of every **Execution
Pool** filled: it requests a **JIT Config** with the target repository's token,
starts a runner container, waits for it to exit, and repeats. On axpc it is still
the PowerShell script; the **CLI**'s `start` is the supervisor that replaces it.

**CLI** - this project's Rust command line tool, under `cli/`. `validate` checks a
**Host Configuration**; `build` builds its **Runner Image** from the definition
the binary embeds; `start` runs a host's pools in the foreground as the
**Supervisor**, and `stop`, `restart`, `status`, and `diagnostics` talk to it.

**Instance State** - where the one running `start` on a machine stands: running,
stopping, stopped with a reason, or resuming; or `building`, while a `build` holds
the instance lock instead. `status` reports it, and it decides which requests the
instance accepts. See
[operations/cli.md](./operations/cli.md#instance-state).

**Runtime Core** - the part of `start` that prepares the **Execution Pools** and
runs every **Slot**'s loop until a stop. The instance restarts it after a failure;
see [operations/cli.md](./operations/cli.md#the-instance-layer).

**Diagnostic Container** - a job container of an **Execution Pool** with
`diagnostics` on. It runs the runner under the image's collector, carries the
`runners.diagnostic-lifecycle=1` label, and has no `--rm`, so it stays after its job.
The **Supervisor** finalises it: it exports the collector's evidence into a bundle
in the pool's private sink, then removes it. Until then it is retained with the
settings it ran with. See
[operations/cli.md](./operations/cli.md#diagnostics).

**Host Profile** - the host a **Host Configuration** is checked for: `axpc`, the
Windows machine with Docker Desktop, or `axmc`, the macOS machine. A profile fixes
the fields, the platform labels, the **Routing Label**, and the path form. See
[operations/cli.md](./operations/cli.md).

**Shared Fixtures** - the configuration files under `tests/host-configuration/`
that the PowerShell validator's test, the **CLI**'s tests, and the **Parity Test**
read, with one `expectations.json` recording what each must give.

**Parity Test** - the check that runs the PowerShell validator and the **CLI** over
the **Shared Fixtures** and fails when their outcomes, ordered errors, or summaries
differ. It ends when the PowerShell validator is removed.

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
