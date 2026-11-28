---
type: Procedure
title: "Windows app installation layout and rollback"
description: "What does the MSI install, and when can a host safely return to a previous release?"
---

# Windows app installation layout and rollback

## Install Layout

W1's MSI creates this layout, as [the installer contract](../marshal-cli/release-artifacts.md#installers) describes, and the build
leg's installer test measures it on every run; no host has it installed yet.

| What                                  | Where                                                                                                               |
| ------------------------------------- | ------------------------------------------------------------------------------------------------------------------- |
| Marshal (app, agent and hidden modes) | `%LOCALAPPDATA%\Programs\marshal\marshal-app.exe`                                                                   |
| Licence notices                       | `%LOCALAPPDATA%\Programs\marshal\THIRD-PARTY-NOTICES.txt`, the [Windows notices file](./notices.md#licence-notices) |
| Start menu entry                      | Marshal, starting the app                                                                                           |
| Configuration (default)               | `%LOCALAPPDATA%\marshal\config.toml`                                                                                |
| Logs                                  | `%LOCALAPPDATA%\marshal\logs\`, the app's files under their own name                                                |
| Tokens                                | Credential Manager, one generic credential per repository or organisation                                           |
| Web view data                         | `%LOCALAPPDATA%\app.axross.marshal\`, named by the bundle identifier                                                |

The bundle identifier is `app.axross.marshal`, the same as the signing identifier
that [CLI](../marshal-cli/signing.md#the-signing-contracts) fixes, because the app and the agent are
one code object. The name `marshal-app.exe` never collides with a leftover
`marshal.exe`, and Task Manager tells the two apart.

## Rolling Back

Complete [step 1 of the Windows host rollback procedure](../runner-hosts/windows-recovery.md#rolling-back) before uninstalling Marshal. Keep the installed app if you cannot verify a compatible target. If the target might handle retained diagnostic containers differently, recover them with the installed version before continuing. For an app target, follow [Updating, Repairing, Uninstalling and Rolling Back](../marshal-cli/install-on-windows.md#updating-repairing-uninstalling-and-rolling-back).

Before the first app release, the previous release is the last command-line
release. For that target, open the guide at that release's tag before uninstalling,
as
[The Last Command-Line Release](../marshal-cli/release-artifacts.md#the-last-command-line-release)
says. Use that tag's guide for the command line's installation and the program tree
it identifies for the runtime procedures.

The PowerShell host scripts are gone, so a rollback never restores
them. Going back from the
app to the command line takes three things the app does not do for you:

- **Quit Marshal first**, so no agent holds the lock and the command line's `start`
  can take it. If the app does not answer, end `marshal-app.exe` in Task Manager; the
  next start removes the containers it left.
- **Keep the token files** until the app's runners are working. No code path of the
  app reads them, and the command line reads them only while it exists, so a rollback
  finds them as they were.
- **Keep the configuration.** The command line reads its own `--config` path, and the
  app never changes that file, so a rollback needs no configuration change.
