# runners

Generic GitHub Actions self-hosted runner images and Marshal, the Windows app that
runs them, and later the host configuration for Claude Code Remote Control and Amp
runner agent hosts on WSL 2.

The repository holds one runner image, the Rust library `marshal-cli` that runs it
for several repositories, and Marshal, a Windows notification-area app whose
resident agent runs that library from a Windows machine with Docker Desktop, which
each release ships with a per-user MSI. There is no command line: the `marshal`
command line ended with the
[last command-line release](./docs/operations/cli.md#the-last-command-line-release),
which the axpc host still runs until it moves to the app.
[Windows Runner Host](./docs/operations/windows-runner-host.md) owns the operator
procedure, including how a host moves to the app, and states what has and has
not been verified on a real host. The
library checks a host configuration, builds the runner image, runs a host's pools,
and exports what a diagnostics-enabled pool left. On the axmc host, an Apple silicon
Mac, the library runs each Linux and macOS pool's jobs in a fresh Tart VM of its OS
that is cloned from a golden VM, on Softnet, with no Docker Desktop, and builds the
golden VMs; axmc has no supported way to run it until M2's Mac app.
[macOS Runner Host](./docs/operations/macos-runner-host.md) owns that runtime and
states what has and has not been run on the Mac. The agent-host configuration is
not here yet.

This repository is public. Its GitHub Actions CI and review jobs run on
GitHub-hosted runners, never on a self-hosted one; see
[Security](./docs/conventions/security.md).

## Development

Prepare these tools to work on the repository:

- **mise**, which pins every tool the checks use, as
  [`mise.toml`](./mise.toml) declares.
- **The Rust toolchain** that [`cli/rust-toolchain.toml`](./cli/rust-toolchain.toml)
  pins, for the library and the app.

Read these guides before making a change:

- [Development setup](./docs/operations/development-setup.md) — install the
  pinned tools and the Rust toolchain.
- [Contributor agreement](./AGENTS.md) — working rules for agent sessions.
- [Development workflow](./docs/operations/development-workflow.md) — plan,
  verify, review, and deliver a change; the same gates apply without an agent.
- [Testing](./docs/conventions/testing.md) — the check commands and what CI runs.
- [Directory structure and technologies](./docs/conventions/directory-structure.md)
  — where runner material lives and what it is built with.
- [Documentation index](./docs/index.md) — conventions, operational guides, and
  the glossary.

## Related links

- [REVIEW.md](./REVIEW.md) - the review policy
- [Windows Runner Host](./docs/operations/windows-runner-host.md) - running the
  axpc host
- [macOS Runner Host](./docs/operations/macos-runner-host.md) - running the axmc
  host
