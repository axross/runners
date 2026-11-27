# runners

Generic GitHub Actions self-hosted runner images and Marshal, the Windows app that
runs them, and later the host configuration for Claude Code Remote Control and Amp
runner agent hosts on WSL 2.

The repository holds one runner image, the Rust library `marshal-cli` that runs it
for several repositories, and Marshal, a Windows notification-area app whose
resident agent runs that library from a Windows machine with Docker Desktop. The
app release baseline ships a per-user MSI, with no command line. W1 removed the
`marshal` command line from main; [The last command-line release](./docs/marshal-cli/release-artifacts.md#the-last-command-line-release) explains the missed pre-installer release and its
recovery. The axpc host still runs the command line until it moves to the app.
During the bounded F0 recovery, the release workflow temporarily builds the fixed
historical CLI instead of the app; [CLI Recovery](./docs/runner-recovery/index.md)
owns that procedure and the actual-version legacy CLI MSI/pkg installs. No recovery
release version or native installer evidence is recorded yet. App source and normal
builds remain; the recovery is not a supported parallel CLI product. App publication resumes
only after successful public F0 evidence and a separate reviewed restoration.
[Windows Runner Host](./docs/runner-hosts/index.md#windows-with-docker-desktop) owns the operator
procedure for the app, including how a host moves to it, and states what has and has
not been verified on a real host. The
library checks a host configuration, builds the runner image, runs a host's pools,
and exports what a diagnostics-enabled pool left. On the axmc host, an Apple silicon
Mac, the library runs each Linux and macOS pool's jobs in a fresh Tart VM of its OS
that is cloned from a golden VM, on Softnet, with no Docker Desktop, and builds the
golden VMs; outside the bounded CLI recovery, axmc has no supported release route
until M2's Mac app.
[macOS Runner Host](./docs/runner-hosts/index.md#macos-with-tart) owns that runtime and
states what has and has not been run on the Mac. The agent-host configuration is
not here yet.

This repository is public. Its GitHub Actions CI and review jobs run on
GitHub-hosted runners, never on a self-hosted one; see
[the CI runner policy](./docs/security/workflow-permissions.md#self-hosted-runners-stay-out-of-this-repositorys-own-jobs).

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
- [Development workflow](./docs/agent-work/change-delivery.md) — plan,
  verify, review, and deliver a change; the same gates apply without an agent.
- [Testing](./docs/conventions/testing.md) — the check commands and what CI runs.
- [Directory structure and technologies](./docs/conventions/directory-structure.md)
  — where runner material lives and what it is built with.
- [Engineering documentation](./docs/index.md) — find the subject that owns a runner, app, or release decision.
- [Where to document a change](./docs/conventions/documentation.md#choose-the-owning-subject) — choose the owner for a new or changed claim.

## Related links

- [REVIEW.md](./REVIEW.md) - the review policy
- [Windows Runner Host](./docs/runner-hosts/index.md#windows-with-docker-desktop) - running the
  axpc host
- [macOS Runner Host](./docs/runner-hosts/index.md#macos-with-tart) - running the axmc
  host
