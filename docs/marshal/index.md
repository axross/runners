# Marshal

The desktop app starts a resident agent and presents its pools, configuration and updates. The [library](../marshal-cli/index.md) owns agent behavior. For running a host, use the [Windows host guide](../runner-hosts/index.md#windows-with-docker-desktop) or the [macOS host guide](../runner-hosts/index.md#macos-with-tart).

## Run Marshal

- [Which parts of Marshal exist, and what has been tested on a host?](./availability.md)
- [Which process owns the agent, how does the app connect, and where can I find logs?](./session.md)
- [How do token entry, file diagnostics and applying or discarding a pool edit work?](./configuration.md)
- [Which controls can the operator change and when do they take effect?](./settings.md)
- [When does the window subscribe to GitHub and what can its indicators establish?](./github-status.md)
- [What does an on-demand pool's card show about its scale set, listener state and assigned jobs?](./github-status.md#on-demand-pools-in-the-windows-window)
- [When does the app install an update, wait for a job, or prompt again?](./updates.md)

## Install and recover

- [Which Windows installer is available, and how do I verify an app MSI?](../marshal-cli/install-on-windows.md)
- [What does the MSI install, and when can a host safely return to a previous release?](./rollback.md)
- [Where must Marshal be installed, and how is it upgraded or removed?](./mac-installation.md)
- [Which bundle, Keychain and process states does the Mac app expose?](./mac-app.md)

## Build and maintain the app

- [Which capabilities may the web view use, and what authorizes an update?](../security/app-trust.md)
- [Which commands and events can the bundled web view use?](./webview-commands.md)
- [What do the window, menu icons, fonts and colours show?](./window.md)
- [Which build, manifest and tool pin produce the app on each platform?](./building.md)
- [How are the platform notices produced and verified before packaging?](./notices.md)
- [Which roadmap child owns a change to a Marshal capability?](./roadmap.md)
