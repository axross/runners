---
type: Architecture
title: "Marshal roadmap responsibilities"
description: "Which roadmap child owns a change to a Marshal capability?"
---

# Marshal roadmap responsibilities

## Capability owners

The [Marshal roadmap](https://github.com/axross/runners/issues/72) assigns these capabilities to separate children. Use the owning page when a change affects one of these contracts.

| Child | Capability                                                                                                                                                                                  |
| ----- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| W2    | [Showing each runner's GitHub status](./github-status.md#github-status-in-the-window)                                                                                                       |
| K1    | [Configuration status and running with the last valid configuration](./configuration.md#the-configuration-file)                                                                             |
| W4    | [Automatic updates](./updates.md#updates), [drain and `ending`](../marshal-cli/agent-lifecycle.md#the-update-drain), and the [build tool archive checksum](./building.md#the-tauri-cli-pin) |
| M2    | [Mac app distribution, Keychain access and stop reasons](./mac-app.md#the-mac-app)                                                                                                          |
| W3    | [The Settings tab](./settings.md#the-settings-tab)                                                                                                                                          |
| X1    | [The on-demand pool's scale-set name, listener state and assigned count on its existing card](./github-status.md#on-demand-pools-in-the-windows-window)                                     |
| M4    | [The Settings tab on the Mac](./settings.md#the-settings-tab-on-the-mac)                                                                                                                    |

Document a new agent message with the [protocol version rules](../marshal-cli/agent-protocol.md#version-rules).
