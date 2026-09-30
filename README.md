# runners

Generic GitHub Actions self-hosted runner images and host scripts, and later the
host configuration for Claude Code Remote Control and Amp runner agent hosts on
WSL 2.

## Status

The repository is at its agent foundation. It holds the entry files for agent
sessions, the agent configuration for Claude Code and Amp, the project
documentation, and the pinned lint toolchain. It does not yet hold any runner
material. Runner images, host scripts, and agent-host configuration arrive in
later changes, into this planned layout:

| Planned path               | Holds                                                                                  |
| -------------------------- | -------------------------------------------------------------------------------------- |
| `images/<name>/`           | One runner image per directory, such as a Dockerfile                                   |
| `hosts/<platform>/`        | Scripts that run ephemeral runners on a host, starting with Windows and Docker Desktop |
| `agent-hosts/claude-code/` | WSL 2 service units and templates for Claude Code Remote Control                       |
| `agent-hosts/amp/`         | WSL 2 service units and templates for Amp runners                                      |

This repository is public. Its own CI and review run on GitHub-hosted runners,
never on a self-hosted one; see [Security](./docs/conventions/security.md).

## Getting started

[mise](https://mise.jdx.dev/) pins every tool the checks use, so a local run and
CI use the same versions. Install mise, then from the repository root:

```bash
mise trust
mise install
```

`mise install` installs the tools pinned in [`mise.toml`](./mise.toml): Node,
Prettier, markdownlint-cli2, shellcheck, hadolint, actionlint, and PowerShell.
PSScriptAnalyzer is a PowerShell module rather than a mise tool, so
`mise run lint:powershell` installs its pinned version from the PowerShell
Gallery the first time a PowerShell script exists to lint.

## Commands

Run from the repository root.

| Command                    | What it does                                                                            |
| -------------------------- | --------------------------------------------------------------------------------------- |
| `mise install`             | Install the pinned tools                                                                |
| `mise run format`          | Rewrite Markdown, JSON, YAML, and TypeScript files with Prettier                        |
| `mise run format:check`    | Fail when Prettier would change a file                                                  |
| `mise run lint`            | Run every linter below                                                                  |
| `mise run lint:markdown`   | markdownlint-cli2 over the Markdown files                                               |
| `mise run lint:shell`      | shellcheck over `*.sh` files and `.agents/setup`                                        |
| `mise run lint:docker`     | hadolint over Dockerfiles; passes when there are none                                   |
| `mise run lint:actions`    | actionlint over `.github/workflows/`                                                    |
| `mise run lint:powershell` | PSScriptAnalyzer over `*.ps1`, `*.psm1`, and `*.psd1` files; passes when there are none |
| `mise run check:links`     | Resolve relative links in the Markdown files                                            |
| `mise run check:docs`      | Run the `docs/` structural validators                                                   |
| `mise run check:amp`       | Run the Amp quality-hooks plugin's smoke test (`node --test`, no install needed)        |
| `mise run check:skills`    | Check that `skills-lock.json` and `.claude/skills/` list the same skills                |
| `mise run check`           | Run every gate above except the writing `format`; this is what CI runs                  |

The installed copies under `.claude/skills/` are generated and are excluded from
formatting and linting. [Agent Skills](./docs/operations/agent-skills.md) owns
refreshing them and the upstream comparison that `check:skills` does not do.

## Development workflow

Every change goes through the change loop in
[Development Workflow](./docs/operations/development-workflow.md): a tracking
issue with the plan in its body, the maintainer's approval, work on a
`claude/`-prefixed branch, a draft pull request, and an independent review.
Merging is the maintainer's decision. The same gates apply to a change made
without an agent.

The agent configuration is described in
[Agent Sessions](./docs/operations/agent-sessions.md), which states the command
each format and check hook runs; they use the pinned tools above.

## Related links

- [docs/index.md](./docs/index.md) - project documentation
- [AGENTS.md](./AGENTS.md) - the working agreement for agent sessions
- [REVIEW.md](./REVIEW.md) - the review policy
