---
type: Procedure
title: "Installing the Mac app"
description: "Where must Marshal be installed, and how is it upgraded or removed?"
---

# Installing the Mac app

These steps apply to the [Marshal app bundle](../marshal/mac-app.md#the-bundle-and-the-disk-image). While F0 recovery publishes the historical CLI and package, use [CLI Recovery](../runner-recovery/index.md).

## Installing, Upgrading and Uninstalling

Open the disk image and drag Marshal to Applications, then open it from there.
Marshal controls an agent only from a bundle directly in `/Applications` or
`~/Applications`: a copy opened from
the mounted disk image, Downloads or anywhere else starts no agent, says "Move Marshal
to Applications", and offers only Open Marshal and Quit Marshal; quitting it ends
only that copy. A Marshal that is moved, renamed or replaced while it runs is a
changed executable, so its agent stops every runner and ends, and the next copy
opened from Applications says why it ended: "The last agent ended because Marshal was
moved". The ending copy leaves an empty marker, `agent-ended-moved`, in
`~/Library/Application Support/marshal`, and that next copy removes it.

To upgrade, quit Marshal, which stops the runners, replace it in Applications with
the new version, and open it. The configuration, the logs, the golden VMs and the
Keychain items are kept. Until M4, Marshal has no login item and no updates of its
own.

With the command line still running, the app shows that another Marshal holds the
runner lock. Stop it with `marshal stop` before handing the pools to the app. For a
pkg install, use the uninstall procedure in the recovery release tag's
[CLI Recovery](../runner-recovery/index.md) guide. For a manual copy at `~/.local/bin/marshal`,
use that guide's macOS block under Migrating From a Manual Install instead. Then
use the app to run the pools.

To uninstall, quit Marshal and move it to the Trash. What it leaves, and how to remove
each:

| Left                                    | Remove it with                                                                                                                                                                                                                     |
| --------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| The configuration and the agent's state | `rm -r ~/Library/Application\ Support/marshal`                                                                                                                                                                                     |
| The logs                                | `rm -r ~/Library/Logs/marshal`                                                                                                                                                                                                     |
| The web view's data                     | `rm -r ~/Library/WebKit/app.axross.marshal ~/Library/Caches/app.axross.marshal`                                                                                                                                                    |
| The golden VMs                          | `tart delete <name>` for each `marshal-golden-linux-...` and, with a macOS pool, `marshal-golden-macos-...` that `tart list --source local` shows                                                                                  |
| The bases in Tart's cache               | `tart delete <source>` for each `ghcr.io/cirruslabs/ubuntu` and `ghcr.io/cirruslabs/macos-tahoe-base` entry that `tart list --source oci` shows; the macOS base alone is a download of about 26 GB, and more once unpacked on disk |
| The instance lock                       | `rm /Users/Shared/.marshal-instance.lock`, once no Marshal, the app or the command line, runs on the Mac                                                                                                                           |
| Each token                              | Remove in the Tokens tab before quitting, or `security delete-generic-password -s app.axross.marshal -a github.com/<owner>/<name>`                                                                                                 |

A dry-run disk image from the release's dry run installs the same way; its bundle
version is `0.0.0`, so the next real release replaces it.
