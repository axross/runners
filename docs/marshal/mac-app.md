---
type: Specification
title: "Marshal on macOS"
description: "Which bundle, Keychain and process states does the Mac app expose?"
---

# Marshal on macOS

## The Mac App

M2 ([#84](https://github.com/axross/runners/issues/84)) builds the same app for an
Apple silicon Mac running macOS 26 or later: a menu bar app whose agent runs axmc's
Linux and macOS pools in Tart VMs, cloned from the golden VMs, as
[macOS Runner Host](../runner-hosts/index.md#macos-with-tart) describes. This page describes the bundle, disk image and Mac states. For installation and removal, see [Installing the Mac app](../marshal/mac-installation.md). [The Token Store](../marshal-cli/token-store.md#the-token-store)
owns the Keychain store.

### The Bundle and the Disk Image

| Item                 | Value                                                                                                                                                                                                                                                                                                                                            |
| -------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Disk image           | `marshal-<version>-macos-arm64.dmg`: read-only and zlib-compressed, with the volume name `Marshal`, holding `Marshal.app` and a link named `Applications` to `/Applications`                                                                                                                                                                     |
| Disk image signature | Planned for M2's pull request 2: Developer ID Application, secure timestamp, identifier `app.axross.marshal.dmg`; notarized and stapled                                                                                                                                                                                                          |
| Bundle               | `Marshal.app`, installed at `/Applications/Marshal.app`, with the bundle identifier `app.axross.marshal`, which is also the executable's signing identifier                                                                                                                                                                                      |
| Executable           | `Marshal.app/Contents/MacOS/` holds the one executable and nothing else                                                                                                                                                                                                                                                                          |
| Version              | `CFBundleShortVersionString` and `CFBundleVersion` are the release's `X.Y.Z`, or `0.0.0` for a dry run, since `0.0.0-dryrun.<n>` is not a valid bundle version; the release's app job, planned for M2's pull request 2, passes it to Tauri's build; the macOS app job of `merge-checks.yaml` builds at `0.0.0`. File names keep the full version |
| Minimum system       | `LSMinimumSystemVersion` is `26.0`; the executable is arm64 only                                                                                                                                                                                                                                                                                 |
| Menu bar app         | `LSUIElement` is `true`, so Marshal starts with no Dock icon; it becomes a regular app while its main window is open, and an accessory again when it closes                                                                                                                                                                                      |
| Resources            | `icon.icns`, and the macOS licence notices as `THIRD-PARTY-NOTICES.txt`                                                                                                                                                                                                                                                                          |
| Not in the bundle    | a command line, a helper, a launch agent, a framework, a plug-in, an installer package; the login item is the app itself, which [the session](./session.md#the-agent-and-the-apps-session) registers with `SMAppService`                                                                                                                         |
| Signature            | Planned for M2's pull request 2: Developer ID Application, hardened runtime, secure timestamp, no entitlements, signed inside-out without `--deep`; the app has no stapled ticket of its own, since notarizing the disk image notarizes it                                                                                                       |

While its main window is open, Marshal shows its own menus in place of Tauri's
default: Marshal (About Marshal, with the name and version only; Hide Marshal, Hide
Others, Show All; Quit Marshal with Command-Q, which asks W1's question and quits as
the icon menu's Quit does), Edit (Undo, Redo, Cut, Copy, Paste, Select All, whose key equivalents are the web view's editing shortcuts, such as Command-V in the token field), Runners (Start All Runners, Stop
All Runners, Build Image, under the icon menu's rules) and Window (Minimize, Close with
Command-W, which hides the window). The icon menu's items are in title case on the Mac.
Opening Marshal again from the Finder or Spotlight while it runs shows its window.

`tauri.macos.conf.json` holds the Mac's settings, which Tauri merges over
`tauri.conf.json` on macOS: the bundle with `app` as its only target, the icon and
the notices, and the main window again, since a merge replaces the whole window list,
with the overlay title bar that puts the window buttons in the header band. The Mac
app is bundled with `cargo tauri build --bundles app` in `app/crates/app`; the disk
image is made from it with `hdiutil`, not by Tauri's bundler. The app starts its agent with
the fixed `PATH` `/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin`, because an app
opened from the Finder gets launchd's short `PATH`; the agent finds `tart`, and Tart
finds `softnet`, on it, as
[macOS Runner Host](../runner-hosts/mac-setup.md#installing-tart-and-softnet) says. Until M2's pull request 2
adds the release's app job and restores the signing job, nothing signs or notarizes
either: the `app-macos` job of `merge-checks.yaml` keeps the unsigned app, as a
`ditto` archive, and an unsigned disk image as a workflow artifact for testing. The bundler copies every binary of
`marshal-app` whose features are on, so `fake-docker`, the process tests' stand-in for
the docker client, and those tests need the `fake-engine` feature, which no shipped
build turns on: run the workspace's tests with
`cargo test --locked --workspace --features marshal-app/fake-engine`.

The Mac's app icon is W1's design, the tile and its three lines, drawn on Apple's icon
grid: the tile is 824 of 1024 units with a corner of 22.5% of its side. From `app/`,
`cargo run -p marshal-tray --example icons -- mac-icon crates/app/icons` writes
`icon.icns` at 16 to 1024 px, and `tests/app_icon.rs` fails if the checked-in file
differs from what the code draws.

### Tokens and a Locked Keychain

A token is entered in the Tokens tab and kept in the login keychain; only the agent
reads it, and it never shows a macOS prompt. While the login keychain is locked, a
pool whose token the agent cannot read is skipped, a registration that cannot read
it backs off, and a save is refused, each naming a locked keychain or an item another
build of Marshal stored, since macOS may give both one code. Unlock the login keychain
in Keychain Access, which asks for its password; never put the password in a script.
Then choose Stop All and Start All to start the skipped pools. Keychain Access's
"Lock after" and "Lock when sleeping" settings, under Change Settings for Keychain
"login", say when it locks by itself; its password must be the login password for it
to unlock at sign-in. The VMs need the unlocked keychain too, as
[The Signed-In Session](../runner-hosts/mac-setup.md#the-signed-in-session) says.

For an item another build stored, enter the token again: Marshal deletes the old item
and adds one this build may read. If the delete is refused, the window gives the one
command it shows, `security delete-generic-password -s app.axross.marshal -a
github.com/<owner>/<name>`, or delete the item labelled `Marshal: <owner>/<name>` in
Keychain Access, then enter the token again. An organisation's item has the account
`github.com/<org>` and the label `Marshal: <org> (organisation)`.

### The Mac's Words and States

The window draws W1's states in the Mac's words: the login keychain for the store,
the menu bar, Activity Monitor, the golden VM and VMs for the image and the
containers, and "Waiting for Tart" where W1 waits for Docker. `marshal_view::HOST`
picks the set at compile time; the view takes it from its input, so either set is
drawn and tested on any operating system.

For macOS runners (M3, [#80](https://github.com/axross/runners/issues/80)), the
window shows a slot waiting for the macOS VM limit or for a VM's deletion, the golden
macOS VM after the Linux one with its runner-age and base-age warnings, and a leftover
VM of a pool no longer configured, which Marshal never deletes, pointing to
[Removing the VMs of a Pool No Longer Configured](../runner-hosts/mac-pools.md#removing-the-vms-of-a-pool-no-longer-configured).
The window shows no command but the Keychain's fallback delete. The agent sends these
items, and the view's tests draw each of them from a fake snapshot in the same codes.
