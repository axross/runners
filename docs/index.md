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
  host, the accepted LAN-egress risk, what is never committed, and why the review
  gate admits one bot identity by login.
- [conventions/documentation.md](./conventions/documentation.md) - how this
  project's own documentation is kept true: correcting what a change
  invalidated, making a new document reachable, standing on its own, and the
  checks a documentation change owes.

## Operations

- [operations/development-workflow.md](./operations/development-workflow.md) -
  the change loop, the post-approval delivery sequence, the standing delivery
  grant and its exclusions, branch naming and merging, run-state storage and
  resume retrieval, what catches a loop stopped mid-flight, and the independent
  review route with its coverage evidence.
- [operations/agent-sessions.md](./operations/agent-sessions.md) - how Claude
  Code and Amp sessions start, the format and check hooks, Claude Code's
  plan-gate delivery confirmation, waiting on CI and review, the subagents and
  how an unavailable role is recorded, telemetry tagging, and recommended
  environment variables.
- [operations/windows-runner-host.md](./operations/windows-runner-host.md) -
  running ephemeral runners for several repositories on a Windows machine with
  Docker Desktop: prerequisites, the per-repository token, the host
  configuration, scheduled tasks, health checks, updating, and recovery.
- [operations/agent-skills.md](./operations/agent-skills.md) - installing and
  refreshing the skills from `axross/skills`, the drift checks, and the register
  of deviations and gaps.

## Glossary

- [glossary.md](./glossary.md) - runner, image, host, agent-host, and change-loop
  vocabulary.

For what this project is, how to start it, and the commands it has, see
[`README.md`](../README.md). [`AGENTS.md`](../AGENTS.md) is the working agreement
for agent sessions.
