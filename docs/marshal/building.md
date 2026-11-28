---
type: Procedure
title: "Building Marshal"
description: "Which build, manifest and tool pin produce the app on each platform?"
---

# Building Marshal

## Building the App

The app's workspace is built with Cargo like the CLI, and the Tauri crate also needs
a platform web view, so the whole workspace builds only on Windows and, for
[The Mac App](./mac-app.md#the-mac-app), on macOS. The CLI document
lists the gates that run without it under [The App Workspace](../marshal-cli/toolchain.md#the-app-workspace).

The release executable is built with `cargo tauri build --no-bundle` in
`app/crates/app`: Tauri's bundler is not used, because it writes a per-machine
installer and fetches tools this project does not pin, and the installer will be built
by the release workflows. The manifest already carries the feature lists the tool would
write, and the generated permission files are checked in, so a build changes no tracked
file; the Windows job fails if it does, or if it leaves a file that is neither tracked
nor ignored. Tauri links the Visual C++ runtime statically and uses the Universal C
Runtime that Windows 11 includes.

### The Build Script and the Test Manifest

[`build.rs`](../../app/crates/app/build.rs) declares the commands and has
`tauri-build` write the app's Windows resources, among them a manifest that asks for
version 6 of the common controls. The `common-controls-v6` feature needs it: the menu
and the web view runtime then import `TaskDialogIndirect`, which only that version
exports, and a process gets that version only when its manifest asks. `tauri-build`
links its resource into the binaries alone, so a test executable that links the Tauri
runtime, as `tests/ipc.rs` does, stopped at load with `STATUS_ENTRYPOINT_NOT_FOUND`
(`0xc0000139`) before it ran a test.

When the target is Windows with the MSVC environment, which the script reads from the
target's `CARGO_CFG_TARGET_*` variables and not from the host, `build.rs` also compiles
a manifest holding only that dependency with `embed-resource` and links it with
`cargo:rustc-link-arg-tests`, which Cargo gives to the integration tests, `ipc.rs`,
`policy.rs` and `process/`. `marshal-app.exe`, the unit test of its `main.rs` and `fake-docker` keep
the resource `tauri-build` makes, with its manifest, icon and version, and gain
nothing. Cargo gives the library's own unit tests neither argument, so a unit test in
`src/` that linked the Tauri runtime would stop the same way: the tests that link it
live under `tests/`, and the library has none of its own.

The feature stays on, and no test is skipped to avoid the manifest. `tauri-build` 2.7
has no option to link its resource into every target; when a release has one, it
replaces this second manifest and the `embed-resource` build dependency.

### The `tauri-cli` Pin

[`app/tauri-cli.version`](../../app/tauri-cli.version) is the one place that sets the
version of `tauri-cli`: one line holding one exact `X.Y.Z`.
[`tauri-cli-version.sh`](../../.github/scripts/tauri-cli-version.sh) reads it and
refuses a file that is a range, a prefix or more than one line, so a loose value
never reaches `cargo install`. The Windows `app` job of
`merge-checks.yaml`, and the Windows leg and the `app-macos` job of `release-build.yaml`, install `tauri-cli` with
`cargo install tauri-cli --version "=<pin>" --locked` under `$RUNNER_TEMP`, with no
cache, so the tool compiles on every run. The release build that W1's installer pull
request (pull request 3) adds reads the same file the same way, and so does W4's tool job once it
lands, which commits a checksum of the crate archive that moves with the pin. Do not
write the version anywhere else. To move it:

1. Look up the latest `tauri-cli` release, never recall it.
2. Change the one line of `app/tauri-cli.version`, and run `mise run check:app-pins`
   and `mise run test:tauri-cli-version`.
3. Check the `tauri` crate's version in `app/Cargo.toml` is one the new tool supports,
   and let the Windows job run `cargo tauri build --no-bundle` against it.

Dependabot does not cover the pin, because it is not a crate of the workspace.
[Security](../security/dependency-pins.md#pinning-convention) records why the install
is an exception to the checksum rule.

The Windows job's timeout is 45 minutes, twice the 20 min 37 s the whole job took on
[#95](https://github.com/axross/runners/pull/95)'s green run, rounded up to whole 5
minutes.
