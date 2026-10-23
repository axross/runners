# Documentation

Which of this project's documents a change to a runner surface invalidates, and
what the documentation checks here do and do not cover.

The general practice is not restated here. The installed
[`living-project-documentation`](../../.claude/skills/living-project-documentation/SKILL.md)
capability owns it: the shape of `docs/`, the format each document takes,
correcting a document in the same change that made it wrong, listing a document
in the index, writing a settled or rejected constraint into the document that
governs it, and the three validators. Load it before writing or restructuring
anything under `docs/`. What follows is only this project's own answer, which
nothing outside this repository could know.

## Which Surface Invalidates Which Document

A change to one of these surfaces MUST correct the document that describes it in
the same change:

| Surface changed                                                                                             | Document to correct                                                                                                                                                                                                |
| ----------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| The runner image (`images/actions-runner/`) or the host scripts (`hosts/windows-docker-desktop/`)           | [Windows Runner Host](../operations/windows-runner-host.md), and [Security](./security.md) when the change touches isolation, caches, or tokens                                                                    |
| The CLI (`cli/`), the shared fixtures (`tests/host-configuration/`), or the example host configurations     | [CLI](../operations/cli.md), and [Security](./security.md) when the change touches labels, paths, or the dependency surface; [Windows Runner Host](../operations/windows-runner-host.md) when an axpc rule changes |
| A workflow under `.github/workflows/`                                                                       | [Security](./security.md) for pinning, permissions, and the review gate, and [Development Workflow](../operations/development-workflow.md) for what `merge-checks.yaml` and `claude-review.yaml` run               |
| A hook, `.claude/settings.json`, a subagent definition, `.amp/`, `.agents/setup`, or `.agents/install-rust` | [Agent Sessions](../operations/agent-sessions.md)                                                                                                                                                                  |
| An installed skill or `skills-lock.json`                                                                    | [Agent Skills](../operations/agent-skills.md)                                                                                                                                                                      |
| The change loop, the delivery grant, branch naming, or the review route                                     | [Development Workflow](../operations/development-workflow.md)                                                                                                                                                      |
| A `mise.toml` task or pinned tool                                                                           | [README](../../README.md), which owns the commands                                                                                                                                                                 |

## Checking a Documentation Change

The [README](../../README.md) lists `check:links` and `check:docs` and what each
covers. Neither check resolves a `#fragment`, so a link that carries one is verified by
hand; prefer linking the file and naming the section in prose. What the checks cannot see
stays a reviewer's job: whether a fact sits in the one document that owns it,
whether a document says anything the tree does not already say, and whether a
claim about runner behavior is verified.
