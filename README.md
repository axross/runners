# runners

Generic GitHub Actions self-hosted runner images and Marshal, the Windows app that
runs them, and later the host configuration for Claude Code Remote Control and Amp
runner agent hosts on WSL 2.

## Status

The repository holds the entry files for agent sessions, the agent configuration
for Claude Code and Amp, the project documentation, the pinned lint toolchain, and
the first runner material: one runner image, the Rust library `marshal-cli` that runs
it for several repositories, and Marshal, a Windows notification-area app whose
resident agent runs that library from a Windows machine with Docker Desktop, which
each release ships with a per-user MSI. There is no command line: the `marshal`
command line ended with the
[last command-line release](./docs/operations/cli.md#the-last-command-line-release).
[Windows Runner Host](./docs/operations/windows-runner-host.md) owns the operator
procedure and states what has and has not been verified on a real host. The
library checks a host configuration, builds the runner image, runs a host's pools,
and exports what a diagnostics-enabled pool left. On the axmc host, an Apple silicon
Mac, the library runs each Linux pool's jobs in a fresh Tart Ubuntu VM that is cloned
from a golden VM, on Softnet, with no Docker Desktop, and builds the golden VM; axmc
has no supported way to run it until M2's Mac app.
[macOS Runner Host](./docs/operations/macos-runner-host.md) owns that runtime and
states what has and has not been run on the Mac. The agent-host configuration is not
here yet. Layout:

| Path                            | Holds                                                                                                                                                           | State   |
| ------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------- |
| `images/actions-runner/`        | The runner image: GitHub's runner image plus build dependencies, pinned by digest                                                                               | Present |
| `hosts/windows-docker-desktop/` | An example configuration for the axpc host profile, and the reference for its fields                                                                            | Present |
| `cli/`                          | The `marshal-cli` library: reading a host configuration, building the runner image, running a host's pools, and the resident agent                              | Present |
| `app/`                          | The Windows app: a notification-area icon and main window for the resident agent, which its executable also runs; [App](./docs/operations/app.md) owns it       | Present |
| `release/`                      | The pinned semantic-release tooling that `release.yaml` runs to tag and publish the app's releases                                                              | Present |
| `hosts/macos/`                  | An example configuration for the macOS host profile, `axmc`, whose Linux pools run in Tart VMs; see [macOS Runner Host](./docs/operations/macos-runner-host.md) | Present |
| `agent-hosts/claude-code/`      | WSL 2 service units and templates for Claude Code Remote Control                                                                                                | Planned |
| `agent-hosts/amp/`              | WSL 2 service units and templates for Amp runners                                                                                                               | Planned |

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
Prettier, markdownlint-cli2, shellcheck, hadolint, actionlint, PowerShell, and jq.
PSScriptAnalyzer is a PowerShell module rather than a mise tool, so
`mise run lint:powershell` downloads its pinned version from the PowerShell
Gallery the first time it runs, and refuses it unless its SHA-256 matches the pin
in `mise.toml`. `cargo-deny` and `cargo-about` are mise tools too.

The CLI needs the Rust toolchain that [`cli/rust-toolchain.toml`](./cli/rust-toolchain.toml)
pins, and the app's [`app/rust-toolchain.toml`](./app/rust-toolchain.toml) names the same
channel. Rust is not a mise tool. On Linux x86-64, run the provisioning script, then
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

| Command                             | What it does                                                                                                                                                                                                                                                                                                                                                                                                                           |
| ----------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `mise install`                      | Install the pinned tools                                                                                                                                                                                                                                                                                                                                                                                                               |
| `mise run format`                   | Rewrite Markdown, JSON, YAML, and TypeScript files with Prettier, and Rust files with rustfmt                                                                                                                                                                                                                                                                                                                                          |
| `mise run format:check`             | Fail when Prettier or rustfmt would change a file                                                                                                                                                                                                                                                                                                                                                                                      |
| `mise run lint`                     | Run every linter below                                                                                                                                                                                                                                                                                                                                                                                                                 |
| `mise run lint:markdown`            | markdownlint-cli2 over the Markdown files                                                                                                                                                                                                                                                                                                                                                                                              |
| `mise run lint:shell`               | shellcheck over `*.sh` files, `.agents/setup`, and `.agents/install-rust`                                                                                                                                                                                                                                                                                                                                                              |
| `mise run lint:docker`              | hadolint over Dockerfiles; passes when there are none                                                                                                                                                                                                                                                                                                                                                                                  |
| `mise run lint:actions`             | actionlint over `.github/workflows/`                                                                                                                                                                                                                                                                                                                                                                                                   |
| `mise run lint:powershell`          | PSScriptAnalyzer over `*.ps1`, `*.psm1`, and `*.psd1` files, flagging syntax Windows PowerShell 5.1 rejects, after checking that its settings still flag PowerShell 7-only syntax; passes when there are none                                                                                                                                                                                                                          |
| `mise run check:links`              | Resolve relative links in `docs/`, `images/`, `hosts/`, `.claude/agents/`, and the root Markdown files                                                                                                                                                                                                                                                                                                                                 |
| `mise run check:docs`               | Run the `docs/` structural validators                                                                                                                                                                                                                                                                                                                                                                                                  |
| `mise run check:amp`                | Run the Amp quality-hooks plugin's smoke test (`node --test`, no install needed)                                                                                                                                                                                                                                                                                                                                                       |
| `mise run check:skills`             | Check that `skills-lock.json` and `.claude/skills/` list the same skills                                                                                                                                                                                                                                                                                                                                                               |
| `mise run lint:rust`                | clippy over the CLI, all targets, warnings as errors                                                                                                                                                                                                                                                                                                                                                                                   |
| `mise run test:set-release-version` | Run the version write script, `.github/scripts/set-release-version.sh`, against temporary copies of the CLI's and the app's `Cargo.toml` and `Cargo.lock`: a release and a prerelease version, invalid versions, a missing placeholder or package, a duplicate entry, and CRLF files                                                                                                                                                   |
| `mise run test:verify-release`      | Run the release gate, `.github/scripts/verify-release.sh`, against temporary release files: the plan phase's output, the exact files, a missing, extra, or misnamed executable or installer, a directory in place of a file, a checksum mismatch, an unset or unknown phase, and the asset globs in `release/.releaserc.json` against the exact files                                                                                  |
| `mise run test:assemble-release`    | Run `.github/scripts/assemble-release.sh`, which writes `SHA256SUMS` for the release's app executable and MSI, against temporary directories: the two lines in their fixed order, and a refusal for another file set, a changed file, or a hash that is not a SHA-256. It needs GNU `sha256sum` and GNU `find -printf`, so it runs on Linux (CI), not on macOS                                                                         |
| `mise run check:release-shape`      | Read the `.yaml` and `.yml` workflows with `.github/scripts/check-release-shape.mjs` and fail unless the release jobs keep the shape the release record describes: no job declares an Environment or reads a signing secret, and each release job keeps its place in the chain. It catches mistakes, not a hostile edit; the rules are listed in [What the Release Tests Cover](./docs/operations/cli.md#what-the-release-tests-cover) |
| `mise run test:release-shape`       | Run that check against the unchanged workflows, then against copies altered one change at a time, and expect each alteration to fail                                                                                                                                                                                                                                                                                                   |
| `mise run test:installer-version`   | Run `.github/scripts/installer-version.sh`: a release `X.Y.Z` is kept, `0.0.0-dryrun.<n>` maps to `0.0.0`, and a version beyond the MSI limits or in another prerelease form is refused                                                                                                                                                                                                                                                |
| `mise run test:fetch-wix`           | Run `.github/scripts/fetch-wix.sh` with a stand-in `curl` and `dotnet`: a package whose SHA-256 differs from the pin is refused before `dotnet` runs, and a package with the pinned hash is installed from a NuGet.Config that lists only its folder                                                                                                                                                                                   |
| `mise run test:rust`                | Run the CLI's unit and integration tests, which read the shared fixtures                                                                                                                                                                                                                                                                                                                                                               |
| `mise run deny:rust`                | `cargo deny` bans, licenses, and sources over the CLI's crates; the advisory check runs in its own workflow                                                                                                                                                                                                                                                                                                                            |
| `mise run lint:app`                 | clippy over the app's platform-independent crates (the mode dispatch, the view, the session, the icon, and the update feed), all targets, warnings as errors; the Tauri crate needs the web view and is linted by the Windows CI job                                                                                                                                                                                                   |
| `mise run test:app`                 | Run the tests of those five app crates                                                                                                                                                                                                                                                                                                                                                                                                 |
| `mise run test:app-frontend`        | Run the main window page's tests with Node: the order it draws views in, and that it clears a typed token                                                                                                                                                                                                                                                                                                                              |
| `mise run deny:app`                 | `cargo deny` bans, licenses, and sources over the crates the app's executable links, for the Windows and macOS targets; the advisory check runs in its own workflow                                                                                                                                                                                                                                                                    |
| `mise run check:notices`            | Fail when the checked-in licence notices in `app/notices/` no longer match `app/Cargo.lock`; `mise run notices:app` writes them again                                                                                                                                                                                                                                                                                                  |
| `mise run check:app-pins`           | Fail unless the CLI's and the app's toolchain files name one channel and `app/tauri-cli.version` holds one exact version; `mise run test:tauri-cli-version` tests the script that reads it                                                                                                                                                                                                                                             |
| `mise run check:app-lock`           | Fail when the app's lockfile resolves a crate `marshal-cli` builds with, for Windows, macOS or Linux, to another version than the CLI's                                                                                                                                                                                                                                                                                                |
| `mise run check`                    | Run every gate above except the writing `format`; this is what CI runs                                                                                                                                                                                                                                                                                                                                                                 |

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
