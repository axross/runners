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

| Surface changed                                                                                   | Document to correct                                                                                                                                                                                  |
| ------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| The runner image (`images/actions-runner/`) or the host scripts (`hosts/windows-docker-desktop/`) | [Windows Runner Host](../operations/windows-runner-host.md), and [Security](./security.md) when the change touches isolation, caches, or tokens                                                      |
| A workflow under `.github/workflows/`                                                             | [Security](./security.md) for pinning, permissions, and the review gate, and [Development Workflow](../operations/development-workflow.md) for what `merge-checks.yaml` and `claude-review.yaml` run |
| A hook, `.claude/settings.json`, a subagent definition, `.amp/`, or `.agents/setup`               | [Agent Sessions](../operations/agent-sessions.md)                                                                                                                                                    |
| An installed skill or `skills-lock.json`                                                          | [Agent Skills](../operations/agent-skills.md)                                                                                                                                                        |
| The change loop, the delivery grant, branch naming, or the review route                           | [Development Workflow](../operations/development-workflow.md)                                                                                                                                        |
| A `mise.toml` task or pinned tool                                                                 | [README](../../README.md), which owns the commands                                                                                                                                                   |

A claim about runner behavior is either verified or described as planned, as the
documentation lens in [REVIEW.md](../../REVIEW.md) requires.

A document that adds a runner image, host, or agent-host surface also adds its
row to [`AGENTS.md`](../../AGENTS.md)'s Routing a Change table. That table is
this project's one routing table; a new topic with no such surface does not grow
a second one.

## Checking a Documentation Change

Run, from the repository root:

```bash
mise run check:links
mise run check:docs
```

`check:links` resolves every relative link to a Markdown file in `docs/`,
`images/`, `hosts/`, `.claude/agents/`, and the root Markdown files
(`README.md`, `AGENTS.md`, `CLAUDE.md`, `REVIEW.md`), which a `docs/`-only sweep
would miss. `check:docs` runs the capability's three validators over `docs/`: a
document `index.md` does not link, a spec with no matching glossary heading, and
a relative link within `docs/` that does not resolve. The
[README](../../README.md) states that `mise run check` runs both along with the
rest of the gate.

Neither check resolves a `#fragment`, which is one reason a link under `docs/`
goes to the file and names the section in prose. What the checks cannot see
stays a reviewer's job: whether a fact sits in the one document that owns it,
whether a document says anything the tree does not already say, and whether a
claim about runner behavior is verified.
