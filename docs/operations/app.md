# App

The Windows app's Cargo workspace under [`app/`](../../app): the one executable and
its modes, the debug-only test variables, what the window, menu and icon show for a
snapshot, and the app's tests and what they do and do not show.

[CLI](./cli.md) owns the library the app and the agent are built on: the agent's
state, the token store, protocol version 2 and the snapshot. [Security](../conventions/security.md)
owns the rules the app is reviewed against, including its processes and its
dependencies. [Windows Runner Host](./windows-runner-host.md) owns running axpc's
pools with the command line, which is how axpc runs them today.

## What Exists

The workspace holds the mode dispatch, the view and their tests, but nothing builds the app's
executable, installs it or releases it. `marshal` is still built and released, and
axpc still runs its pools with it.

| Part                                                          | State                                                                                          |
| ------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| The mode dispatch, the `marshal-modes` crate                  | Tested by `mise run check` on Linux, the `app-modes` job on macOS and the `app` job on Windows |
| What the window, menu and icon show, the `marshal-view` crate | Tested by `mise run check` on Linux and the `app` job on Windows                               |
| The agent's resident mode and protocol version 2              | In the `marshal-cli` library, which the mode dispatch runs in its agent mode                   |
| Verification on axpc                                          | Not done; see [Verification Status](#verification-status)                                      |

## The Executable and Its Modes

Marshal is one executable. Its first argument decides its mode before the app
starts, in the `marshal-modes` crate:

| Arguments                                  | Mode                                                                                         |
| ------------------------------------------ | -------------------------------------------------------------------------------------------- |
| none, or an argument that is not one below | The app, which the crate leaves to its caller                                                |
| `agent running`, `agent stopped`           | The agent: the library's resident agent, started running the pools or waiting to be told to  |
| `export` and its arguments                 | The diagnostics exporter child, which the agent starts for a diagnostic container's evidence |
| `shim` and its arguments                   | On Unix only, the shim a diagnostic container's `docker run` goes through                    |
| `vm-shim` and its arguments                | On Unix only, the shim that holds one Tart VM's `tart run`, for the axmc runtime             |
| `agent` with anything else                 | Refused with exit status 2                                                                   |

The hidden modes are internal. They have no window, no icon and no console, and no
compatibility promise: their arguments are what the app and the library pass each
other, not an interface for a person or a script. Each `docker` client the agent
starts and the exporter child get `CREATE_NO_WINDOW` on Windows; [CLI](./cli.md#running-a-host)
says where it is set and tested, which children inherit the CLI's standard streams,
and what only the axpc run shows, including whether a window still appears. The agent mode is kept to one
by the machine-wide lock the library's agent takes, as `start` is, so the app and the
command line never run the pools together.

### The Debug-Only Test Variables

A debug build reads three variables in the agent mode, so a test can point the
executable at a scratch host:

| Variable                  | Moves                                                                |
| ------------------------- | -------------------------------------------------------------------- |
| `MARSHAL_TEST_ROOT`       | The agent's lock, channel, log and configuration locations           |
| `MARSHAL_TEST_DOCKER`     | The `docker` program the agent runs, so a test can use `fake-docker` |
| `MARSHAL_TEST_GITHUB_API` | The address the agent sends registration requests to                 |

They are the names the command line's `start` reads. A release build ignores all of
them: the `marshal-modes` crate reads them only when `debug_assertions` is on, so no
environment moves a release build's lock, channel, files, `docker` or GitHub address.
[Security](../conventions/security.md#the-app-surface) records why. The crate has a
test of that in the release profile, which the `app-modes` job runs on a macOS hosted
runner with `cargo test --release` and all three variables set, so the test fails if a
release build reads them, as a Linux step runs the library's own options in that
profile. `mise run check` runs the crate's tests in the debug profile only.

## The View

What the window and menu show for a snapshot is computed in the `marshal-view` crate,
which has no operating system dependency. Its tests build views from the recorded
snapshots in `app/crates/view/tests/fixtures/`.

A runner row writes its state out beside a mark: "Container running" with a check,
"Saving diagnostics" with a check while the runner's diagnostic container is
finalising, "Registering" with a ring, "Backing off" and "Held" with the history mark, and
"Stopped" with the stop mark. A backing-off or held runner shows the agent's reason under
its state, and a held one is a runner whose job host a host prerequisite refuses, so
its reason says which. A state the app does not know shows the runner's own reason as
its words, with the unknown mark, or the state's word when there is no reason, as the version
rules in [CLI](./cli.md#version-rules) require.

From `app/`, `cargo run -p marshal-view --example dump_view -- <fixture.json>` prints a
recorded snapshot's view as JSON, with options to set the connection, a pending
action, an open token form, a token result and the settled count, which the example's
header comment lists.

## Verification Status

What CI and the checks show, and what only a real axpc session can, so a reader
does not take a test for a host that has run it:

- **Run on every pull request:** the checks of [The App Workspace](./cli.md#the-app-workspace)
  on Linux, the Windows job's clippy and tests, and the mode dispatch on macOS, in the
  debug profile and then in the release profile. The Windows job runs the tests with
  `--no-fail-fast`, so one test executable that fails shows every other executable's
  result in the same run. Its timeout is 15 minutes, twice the 5 min 12 s that
  checkout, the toolchain, clippy and the tests took on
  [#95](https://github.com/axross/runners/pull/95)'s green run, rounded up to whole 5
  minutes.
- **Not shown by any check:** that no console window appears from the exporter or the
  agent's `docker` clients (the flags are asserted at the command builders, and the
  window is not observable without `unsafe`), and that a terminal `marshal build` on
  Windows still streams `docker build`'s output with `CREATE_NO_WINDOW`, which is
  checked on axpc. The axpc results are recorded on the
  tracking issue, not here.
