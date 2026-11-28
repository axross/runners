---
type: Specification
title: "Marshal availability and verification"
description: "Which parts of Marshal exist, and what has been tested on a host?"
---

# Marshal availability and verification

## What Exists

The app builds, its tests run, and since W1's installer pull request (pull request 3)
each release carries it and its per-user MSI, as [Release artifacts and workflow boundaries](../marshal-cli/release-artifacts.md#releases) describes,
in place of the command line. W1, [#73](https://github.com/axross/runners/issues/73),
is the child that owns the app. No host has installed it yet: axpc still runs its
pools with the last command-line release, identified in
[The Last Command-Line Release](../marshal-cli/release-artifacts.md#the-last-command-line-release),
so this document describes an app that CI builds and tests and the release ships,
not one a host has run. The same pull request removed the command line: no `marshal`
binary is built on any operating system, and the app is the only way to run a host.

| Part                                                                                    | State                                                                                                                                                                                          |
| --------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| The app, its menu and its main window                                                   | Built by the Windows and macOS app jobs of `merge-checks.yaml`; each build is kept as a run artifact                                                                                           |
| The agent's resident mode and protocol version 2                                        | In the `marshal-cli` library, served by the app's executable in its agent mode                                                                                                                 |
| The token store                                                                         | The Credential Manager on Windows and the login keychain on macOS, entered in the main window                                                                                                  |
| The installer, the release, the [install layout](../marshal/rollback.md#install-layout) | Built and tested by the release build on every pull request; see [the installer contract](../marshal-cli/release-artifacts.md#installers). The Mac's disk image comes with M2's pull request 2 |
| Updates                                                                                 | The app's side, with the committed public key; the feed comes with W4's last pull request                                                                                                      |
| The command line's removal                                                              | Done in W1's installer pull request; the last command-line release keeps it                                                                                                                    |
| Verification on axpc                                                                    | Not done; see [Verification Status](#verification-status)                                                                                                                                      |

## Verification Status

What CI and the checks show, and what only a real axpc session can, so a reader
does not take a build for a host that has run it:

- **Run on every pull request:** the checks of [The App Workspace](../marshal-cli/toolchain.md#the-app-workspace)
  on Linux, the Windows job's clippy, tests and `cargo tauri build --no-bundle`, and
  the mode dispatch on macOS, in the debug profile and then in the release profile,
  and the macOS app job's clippy, tests, location rule in the release profile, and
  unsigned `Marshal.app` and disk image.
  The Windows job runs the tests with `--no-fail-fast`, so one test executable that
  fails shows every other executable's result in the same run.
- **Run by the Windows and macOS app jobs:** the app's whole workspace, including the
  process tests and the policy and capability files, with the `fake-engine` feature.
  Of what follows, the Windows-only code and the common controls manifest run only on
  Windows.
- **Run only by the Windows job:** the test executables loading with the common
  controls manifest of
  [The Build Script and the Test Manifest](./building.md#the-build-script-and-the-test-manifest);
  the app starting its own executable in agent mode and connecting to it, and quitting
  it; a second app reusing the first's agent; the exporter child against `fake-docker`; a silent
  channel shown as not answering after 5 s with Quit ending only the app; a channel
  that does not answer version 2 shown as another Marshal, asked again every 5 s; an
  agent at its connection limit retried until it answers; a token reaching the agent's
  store trimmed and appearing in no view and no log line; and the Windows-only code
  (the pipe a test squats on, the registry reads of the taskbar's mode and the time
  zone, the creation flags, the IPC origin `http://tauri.localhost`); and the app's
  policy and capability files and the window's size against System design. The files
  are `app/crates/app/tests/ipc.rs`, `app/crates/app/tests/policy.rs` and
  `app/crates/app/tests/process/`; the process tests
  build only in debug, for the [debug-only test variables](../marshal/session.md#the-debug-only-test-variables).
- **Not shown by any check:** that no console window appears from the agent, the
  exporter or their `docker` clients (the flags are asserted at the command builders,
  and the window is not observable without `unsafe`), WebView2 itself, a Windows contrast
  theme, the icon on a real taskbar and its weight at 16 px, the menu opening from the
  keyboard and where its focus goes, the icon following a mode or a scaling change
  within 2 s, a token round trip through the Credential Manager to a registration,
  Narrator, and containers after a sign-out. W1's earlier Chromium renders of the pages, in
  both themes at 880, 721, 720 and 480 px, ran outside the repository and are not a
  gate. K3 adds the separate rendering gate described under
  [The Configuration Editor](./configuration.md#the-configuration-editor). The axpc results are
  recorded on the tracking issue, not here.
