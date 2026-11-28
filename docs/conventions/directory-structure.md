---
type: Convention
title: "Repository layout and technologies"
description: "Choose the repository path for a runner image, host profile, library, app or release change."
---

# Directory Structure

Where this repository's runner material lives, and the technologies it is built
with. Each document named below owns the detail of its own area; this one only
says where things go.

## Placement Rule

Each path listed in [The Layout](#the-layout) holds what its row says. A change
that adds, removes, or repurposes a runner image, host configuration, library,
app, release, or agent-host path MUST update that table in the same change;
[Documentation](./documentation.md)'s invalidation table names every other
document such a change corrects. Repository tooling, such as `docs/`, `tests/`,
`.github/`, and the agent directories, is outside the table.

## Technologies

| Area                        | Technology                                                                                                        | Owner                                                                                      |
| --------------------------- | ----------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------ |
| Library and app             | Rust, with the toolchain pinned in `cli/rust-toolchain.toml`                                                      | [CLI](../marshal-cli/index.md) and [Development Setup](../operations/development-setup.md) |
| App window and bundle       | Tauri                                                                                                             | [App](../marshal/index.md)                                                                 |
| Runner image                | Docker, on GitHub's runner image                                                                                  | [Windows Runner Host](../runner-hosts/index.md#windows-with-docker-desktop)                |
| axmc VMs                    | Tart and Softnet                                                                                                  | [macOS Runner Host](../runner-hosts/index.md#macos-with-tart)                              |
| Releases                    | semantic-release                                                                                                  | [Release publication](../releases/publish.md)                                              |
| Editor rendering gate       | `playwright-core` and a checksum-pinned Chromium, on Linux x64, under `app/crates/app/tests/render/`              | [Testing](./testing.md) and [App](../marshal/index.md)                                     |
| Toolchain, format, and lint | mise, Prettier, markdownlint, shellcheck, hadolint, actionlint, PSScriptAnalyzer, rustfmt, clippy, and cargo-deny | [Development Setup](../operations/development-setup.md) and [Testing](./testing.md)        |

## The Layout

| Path                            | Holds                                                                                                                                                                       | State   |
| ------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------- |
| `images/actions-runner/`        | The runner image: GitHub's runner image plus build dependencies, pinned by digest                                                                                           | Present |
| `hosts/windows-docker-desktop/` | An example configuration for the axpc host profile, and the reference for its fields                                                                                        | Present |
| `cli/`                          | The `marshal-cli` library: reading a host configuration, building the runner image, running a host's pools, and the resident agent                                          | Present |
| `app/`                          | The Marshal desktop app and its resident agent, including the Windows notification-area icon and the Mac bundle; [Marshal](../marshal/index.md) owns it                     | Present |
| `release/`                      | The pinned semantic-release tooling that `release.yaml` runs to tag and publish the app's releases                                                                          | Present |
| `hosts/macos/`                  | An example configuration for the macOS host profile, `axmc`, whose Linux and macOS pools run in Tart VMs; see [macOS Runner Host](../runner-hosts/index.md#macos-with-tart) | Present |
| `agent-hosts/claude-code/`      | WSL 2 service units and templates for Claude Code Remote Control                                                                                                            | Planned |
| `agent-hosts/amp/`              | WSL 2 service units and templates for Amp runners                                                                                                                           | Planned |
