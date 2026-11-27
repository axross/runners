---
type: Procedure
title: "Development setup"
description: "Prepare the toolchain, documentation validators and rendering gate for a local checkout."
---

# Development Setup

How to prepare a local checkout so the project's checks run with the same tool
versions as CI. [Testing](../conventions/testing.md) lists the commands those
tools run; [Agent Sessions](../agent-work/session-setup.md) describes how an agent session
provisions the same tools for itself.

## Pinned Tools

[mise](https://mise.jdx.dev/) installs the pinned toolchain used by local checks and CI. Install mise, then from the repository root:

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

## Documentation checks

The first `mise run check:docs` needs Node and npm, Python 3 with `venv` and
pip, and network access. It installs locked JavaScript parsers, PyYAML 6.0.3
and the pinned OKF v0.3 validator under `node_modules/.cache/runners-docs/`.
The [dependency pins](../security/dependency-pins.md#pinning-convention) identify
the parser lockfile and approved PyYAML wheel hashes. Subsequent runs can use the cache
offline. See [Checking a Documentation Change](../conventions/documentation.md#checking-a-documentation-change)
for checkout and standalone bundle checks.

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
[Rust Provisioning](../agent-work/session-setup.md#rust-provisioning) in Agent Sessions
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

The format and check hooks use the pinned tools above. See [Agent quality hooks](../agent-work/quality-hooks.md) for the commands Amp and Claude Code run after edits and at turn end.
