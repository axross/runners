# runners

Generic GitHub Actions self-hosted runner images and host scripts, and later the
host configuration for Claude Code Remote Control and Amp runner agent hosts on
WSL 2.

## Status

The repository holds the entry files for agent sessions, the agent configuration
for Claude Code and Amp, the project documentation, the pinned lint toolchain, and
the first runner material: one runner image and the host scripts that run it for
several repositories from a Windows machine with Docker Desktop.
[Windows Runner Host](./docs/operations/windows-runner-host.md) owns the operator
procedure and states what has and has not been verified on a real host. A Rust
command line tool, the CLI, has started to replace those scripts; so far it only
checks a host configuration, and nothing that runs jobs uses it yet. The
agent-host configuration is not here yet. Layout:

| Path                            | Holds                                                                                   | State   |
| ------------------------------- | --------------------------------------------------------------------------------------- | ------- |
| `images/actions-runner/`        | The runner image: GitHub's runner image plus build dependencies, pinned by digest       | Present |
| `hosts/windows-docker-desktop/` | The supervisor, image rebuild, and scheduled-task scripts, and an example configuration | Present |
| `cli/`                          | The CLI: `validate` checks a host configuration                                         | Present |
| `hosts/macos/`                  | An example configuration for the macOS host profile, `axmc`                             | Present |
| `agent-hosts/claude-code/`      | WSL 2 service units and templates for Claude Code Remote Control                        | Planned |
| `agent-hosts/amp/`              | WSL 2 service units and templates for Amp runners                                       | Planned |

This repository is public. Its GitHub Actions CI and review jobs run on
GitHub-hosted runners, never on a self-hosted one; see [Security](./docs/conventions/security.md).

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
`mise run lint:powershell` downloads its pinned version from the PowerShell
Gallery the first time it runs, and refuses it unless its SHA-256 matches the pin
in `mise.toml`. `cargo-deny` is a mise tool too.

The CLI needs the Rust toolchain that [`cli/rust-toolchain.toml`](./cli/rust-toolchain.toml)
pins. Rust is not a mise tool. On Linux x86-64, run the provisioning script, then
put `~/.cargo/bin` on `PATH`:

```bash
.agents/install-rust
```

It installs `rustup` from a pinned release when that is absent, then the pinned
toolchain, and does nothing when both are there. The agent session setup and the
Claude Code session-start hook run it for you; see
[Agent Sessions](./docs/operations/agent-sessions.md). On another platform, install
`rustup` yourself. Without the toolchain, every Rust task fails at once with one
line that names the script.

## Commands

Run from the repository root.

| Command                    | What it does                                                                                                                                                                                                  |
| -------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `mise install`             | Install the pinned tools                                                                                                                                                                                      |
| `mise run format`          | Rewrite Markdown, JSON, YAML, and TypeScript files with Prettier, and Rust files with rustfmt                                                                                                                 |
| `mise run format:check`    | Fail when Prettier or rustfmt would change a file                                                                                                                                                             |
| `mise run lint`            | Run every linter below                                                                                                                                                                                        |
| `mise run lint:markdown`   | markdownlint-cli2 over the Markdown files                                                                                                                                                                     |
| `mise run lint:shell`      | shellcheck over `*.sh` files, `.agents/setup`, and `.agents/install-rust`                                                                                                                                     |
| `mise run lint:docker`     | hadolint over Dockerfiles; passes when there are none                                                                                                                                                         |
| `mise run lint:actions`    | actionlint over `.github/workflows/`                                                                                                                                                                          |
| `mise run lint:powershell` | PSScriptAnalyzer over `*.ps1`, `*.psm1`, and `*.psd1` files, flagging syntax Windows PowerShell 5.1 rejects, after checking that its settings still flag PowerShell 7-only syntax; passes when there are none |
| `mise run check:links`     | Resolve relative links in `docs/`, `images/`, `hosts/`, `.claude/agents/`, and the root Markdown files                                                                                                        |
| `mise run check:docs`      | Run the `docs/` structural validators                                                                                                                                                                         |
| `mise run check:amp`       | Run the Amp quality-hooks plugin's smoke test (`node --test`, no install needed)                                                                                                                              |
| `mise run check:skills`    | Check that `skills-lock.json` and `.claude/skills/` list the same skills                                                                                                                                      |
| `mise run build:rust`      | Build the CLI's debug binary at `cli/target/debug/runners`                                                                                                                                                    |
| `mise run lint:rust`       | clippy over the CLI, all targets, warnings as errors                                                                                                                                                          |
| `mise run test:host`       | Check the Windows host configuration validation against the shared fixtures, the example, and its other checks                                                                                                |
| `mise run test:rust`       | Run the CLI's unit and integration tests, which read the shared fixtures                                                                                                                                      |
| `mise run deny:rust`       | `cargo deny` bans, licenses, and sources over the CLI's crates; the advisory check runs in its own workflow                                                                                                   |
| `mise run test:parity`     | Build the CLI and check it against the PowerShell validator on the shared fixtures, after the harness's self-test                                                                                             |
| `mise run check`           | Run every gate above except the writing `format`; this is what CI runs                                                                                                                                        |

The installed copies under `.claude/skills/` are generated and are excluded from
formatting and linting. [Agent Skills](./docs/operations/agent-skills.md) owns
refreshing them and the upstream comparison that `check:skills` does not do.

## Development workflow

Every change goes through the change loop in
[Development Workflow](./docs/operations/development-workflow.md): a tracking
issue with the plan in its body, the maintainer's approval, work on a branch
named for its issue (`issue-<n>-<description>`, or `noissue-<description>`), a
draft pull request, and an independent review.
Merging is the maintainer's decision. The same gates apply to a change made
without an agent.

The agent configuration is described in
[Agent Sessions](./docs/operations/agent-sessions.md), which states the command
each format and check hook runs; they use the pinned tools above.

## Related links

- [docs/index.md](./docs/index.md) - project documentation
- [AGENTS.md](./AGENTS.md) - the working agreement for agent sessions
- [REVIEW.md](./REVIEW.md) - the review policy
