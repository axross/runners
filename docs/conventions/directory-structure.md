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

| Area                        | Technology                                                                                                        | Owner                                                                                   |
| --------------------------- | ----------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------- |
| Library and app             | Rust, with the toolchain pinned in `cli/rust-toolchain.toml`                                                      | [CLI](../operations/cli.md) and [Development Setup](../operations/development-setup.md) |
| App window and bundle       | Tauri                                                                                                             | [App](../operations/app.md)                                                             |
| Runner image                | Docker, on GitHub's runner image                                                                                  | [Windows Runner Host](../operations/windows-runner-host.md)                             |
| axmc VMs                    | Tart and Softnet                                                                                                  | [macOS Runner Host](../operations/macos-runner-host.md)                                 |
| Releases                    | semantic-release                                                                                                  | [Development Workflow](../operations/development-workflow.md)                           |
| Editor rendering gate       | `playwright-core` and a checksum-pinned Chromium, on Linux x64, under `app/crates/app/tests/render/`              | [Testing](./testing.md) and [App](../operations/app.md)                                 |
| Toolchain, format, and lint | mise, Prettier, markdownlint, shellcheck, hadolint, actionlint, PSScriptAnalyzer, rustfmt, clippy, and cargo-deny | [Development Setup](../operations/development-setup.md) and [Testing](./testing.md)     |

## The Layout

| Path                                          | Holds                                                                                                                                                                 | State   |
| --------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------- |
| `images/actions-runner/`                      | The runner image: GitHub's runner image plus build dependencies, pinned by digest                                                                                     | Present |
| `hosts/windows-docker-desktop/`               | An example configuration for the axpc host profile, and the reference for its fields                                                                                  | Present |
| `cli/`                                        | The `marshal-cli` library: reading a host configuration, building the runner image, running a host's pools, and the resident agent                                    | Present |
| `app/`                                        | The Windows app: a notification-area icon and main window for the resident agent, which its executable also runs; [App](../operations/app.md) owns it                 | Present |
| `release/`                                    | The pinned semantic-release tooling; publication temporarily selects the fixed historical CLI while normal app builds remain                                          | Present |
| `.github/scripts/cli-recovery/`               | Temporary historical-source checks, CLI assembly/publication gate and tests; removed by the reviewed app-path restoration                                             | Present |
| `.github/scripts/cli-recovery-install-tests/` | Temporary suites extracting the recovery tag's CLI procedures, separately from retained app install tests                                                             | Present |
| `hosts/macos/`                                | An example configuration for the macOS host profile, `axmc`, whose Linux and macOS pools run in Tart VMs; see [macOS Runner Host](../operations/macos-runner-host.md) | Present |
| `agent-hosts/claude-code/`                    | WSL 2 service units and templates for Claude Code Remote Control                                                                                                      | Planned |
| `agent-hosts/amp/`                            | WSL 2 service units and templates for Amp runners                                                                                                                     | Planned |
