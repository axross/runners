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
and the tools jobs need.

**Runner Host** - the machine that builds runner images and starts runner
containers, together with the scripts under `hosts/<platform>/`. The first
platform is Windows with Docker Desktop, under `hosts/windows-docker-desktop/`.

**Host Configuration** - the machine-local JSON file, kept outside the repository,
that names a runner host's image and lists its **Target Repositories**. The
**Supervisor** and the host's other scripts read it.

**Target Repository** - a GitHub repository a runner host serves: one entry of the
**Host Configuration**, with its own **Slots**, token, **Custom Labels**, and
cache volumes.

**Custom Label** - an optional runner label an entry of the **Host Configuration**
adds to the default `self-hosted`, `linux`, `x64`, and `axpc`. A workflow lists
`axpc` or a custom label in `runs-on` to choose that host for its **Target
Repository**.

**Slot** - one concurrent runner position on a runner host. A host with two slots
can run two jobs at once; each slot starts a fresh ephemeral runner when its
previous one exits.

**Supervisor** - the host-side script that keeps each slot of every **Target
Repository** filled: it requests a **JIT Config** with that repository's token,
starts a runner container, waits for it to exit, and repeats.

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

**Authoring Host** - the agent product a change is written with, Claude Code or
Amp, or "manual" when no agent is involved. It selects nothing about the review
provider here, since one provider serves every host; it is recorded so a run can
be resumed.

**Standing Delivery Grant** - the maintainer's authorization, adopted in
[operations/development-workflow.md](./operations/development-workflow.md), for a
named set of delivery effects that every run holds without asking.

**Tracking Issue** - the GitHub issue whose body holds a change's plan. The plan
gate is the maintainer's approval of that plan.

**Agent Marker** - the `<!-- ai-agent -->` comment that marks an issue or comment
as written by an agent.

**Installed Skill** - a directory under `.claude/skills/` copied from
`axross/skills` and pinned in `skills-lock.json`. It is generated and never
edited by hand; see [operations/agent-skills.md](./operations/agent-skills.md).
