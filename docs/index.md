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

The runner images, runner host scripts, and agent-host configuration this
repository is for are planned and are not documented here until they exist. A
change that adds one adds the operations document that owns its procedure, an
entry below, and a row in [AGENTS.md](../AGENTS.md)'s routing table.

## Conventions

- [conventions/security.md](./conventions/security.md) - how actions and
  downloads are pinned, least-privilege workflow permissions, why this public
  repository runs its own jobs on GitHub-hosted runners only, the cache-poisoning
  threat model for shared runner storage, what is never committed, and why the
  review gate admits one bot identity by login.

## Operations

- [operations/development-workflow.md](./operations/development-workflow.md) -
  the change loop, the standing delivery grant and its exclusions, branches and
  merging, the configured subagents, and the independent `@claude review` route.
- [operations/agent-sessions.md](./operations/agent-sessions.md) - how Claude
  Code and Amp sessions start, the format and check hooks, the subagents, and
  telemetry tagging.
- [operations/agent-skills.md](./operations/agent-skills.md) - installing and
  refreshing the skills from `axross/skills`, the drift checks, and the register
  of deviations and gaps.

## Glossary

- [glossary.md](./glossary.md) - runner, image, host, and agent-host vocabulary.
