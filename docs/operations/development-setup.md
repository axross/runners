# Development Setup

How to prepare a local checkout so the project's checks run with the same tool
versions as CI. [Testing](../conventions/testing.md) lists the commands those
tools run; [Agent Sessions](./agent-sessions.md) describes how an agent session
provisions the same tools for itself.

## Pinned Tools

[mise](https://mise.jdx.dev/) pins every tool the checks use, so a local run and
CI use the same versions. Install mise, then from the repository root:

```bash
mise trust
mise install
```

`mise install` installs the tools pinned in [`mise.toml`](../../mise.toml): Node,
Prettier, markdownlint-cli2, shellcheck, hadolint, actionlint, PowerShell, and jq.
PSScriptAnalyzer is a PowerShell module rather than a mise tool, so
`mise run lint:powershell` downloads its pinned version from the PowerShell
Gallery the first time it runs, and refuses it unless its SHA-256 matches the pin
in `mise.toml`. `cargo-deny` and `cargo-about` are mise tools too.

## Rust Toolchain

The CLI needs the Rust toolchain that [`cli/rust-toolchain.toml`](../../cli/rust-toolchain.toml)
pins, and the app's [`app/rust-toolchain.toml`](../../app/rust-toolchain.toml) names the same
channel. Rust is not a mise tool. On Linux x86-64, run the provisioning script, then
put `~/.cargo/bin` on `PATH`:

```bash
.agents/install-rust
```

The agent session setup and the Claude Code session-start hook run it for you. On
another platform, install `rustup` yourself.
[Rust Provisioning](./agent-sessions.md#rust-provisioning) in Agent Sessions
describes what the script installs, when it does nothing, and how a Rust task
fails without the toolchain.

## Rendering Gate

On Linux x64, `mise run prepare:app-render` installs the locked `playwright-core`
and the checksum-verified Chromium that `mise run test:app-render` uses. Both
versions are pinned under `app/crates/app/tests/render/`. It needs Python 3, which
extracts the verified archive under `app/target/render/browser-<version>/`, and it
does not install Chromium's OS libraries.
`mise run check` does not run the gate, so no other host needs this step.
[Testing](../conventions/testing.md) says where the gate runs.

## Hooks

The agent configuration is described in
[Agent Sessions](./agent-sessions.md), which states the command each format and
check hook runs; they use the pinned tools above.
