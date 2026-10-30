# Documentation

This project's own documentation, alongside its README. Which body answers which
question: **what must a change satisfy?** -> `conventions/`. **How is something
run or operated?** -> `operations/`. **What does a project word mean?** ->
[glossary.md](./glossary.md). This project keeps no decision log: a settled
constraint's rationale, including a rejected alternative that would otherwise
leave no trace, is stated inline in whichever document governs its subject. It
has no `specs/` directory yet; one is added when a behavior needs describing
rather than instructing, and each spec then earns a matching glossary heading.

Documents under `conventions/` and `operations/` use MUST, MUST NOT, SHOULD,
SHOULD NOT, and MAY as [RFC 2119](https://www.rfc-editor.org/rfc/rfc2119.html)
describes.

The agent-host configuration this repository is also for is planned and is not
documented here until it exists. A change that adds a runner image, host, or
agent-host surface adds the operations document that owns its procedure, an
entry below, and a row in [AGENTS.md](../AGENTS.md)'s routing table.

## Conventions

- [conventions/security.md](./conventions/security.md) - how actions and
  downloads are pinned, least-privilege workflow permissions, why this public
  repository runs its own jobs on GitHub-hosted runners only, the cache-poisoning
  threat model for shared runner storage, per-repository isolation on a runner
  host, the release record for the CLI's dispatch-only release workflow, the accepted
  LAN-egress risk, what is never committed, and why the review
  gate admits one bot identity by login.
- [conventions/documentation.md](./conventions/documentation.md) - which
  document a change to each runner surface invalidates, and what the
  documentation checks cover.

## Operations

- [operations/development-workflow.md](./operations/development-workflow.md) -
  the change loop, the delivery grant, branch naming, bumping the runner, cutting
  a release, and the independent review route.
- [operations/agent-sessions.md](./operations/agent-sessions.md) - how Claude
  Code and Amp sessions start, their hooks, the plan-gate delivery confirmation,
  subagents, and telemetry.
- [operations/windows-runner-host.md](./operations/windows-runner-host.md) -
  running ephemeral runners for several repositories on a Windows machine with
  Docker Desktop with the CLI: prerequisites, installing `marshal`, the
  per-repository token and its file's access control, the host configuration,
  building the image, running and stopping the pools, diagnostics and their
  recovery, health checks, rebuilding and updating, recovery, and rolling back to
  the PowerShell supervisor.
- [operations/cli.md](./operations/cli.md) - the CLI under `cli/`: what `validate`
  accepts and prints for the axpc and axmc host profiles, how it stays equal to the
  PowerShell validator, the shared fixtures and the parity test, the runtime layer
  (container backend, JIT registration, CPU placement, and the slot loop), the
  instance layer (instance state, single-instance lock, and control channel),
  the image contract (its labels, the image check, and freshness), building the
  runner image with `build`, running a host with `start`, `stop`, `restart`,
  `status`, and `diagnostics` (the log, containment, the stop sequence, and the
  diagnostic containers and their export), the toolchain and Rust
  gates, the release build and its dry run, installing, updating, and rolling
  back from a release, the allowed dependencies, and why the CLI's name is fixed.
- [operations/agent-skills.md](./operations/agent-skills.md) - installing and
  refreshing the skills from `axross/skills`, the drift checks, and the register
  of deviations and gaps.

## Glossary

- [glossary.md](./glossary.md) - runner, image, host, CLI, agent-host, and
  change-loop vocabulary.

For what this project is, how to start it, and the commands it has, see
[`README.md`](../README.md). [`AGENTS.md`](../AGENTS.md) is the working agreement
for agent sessions.
