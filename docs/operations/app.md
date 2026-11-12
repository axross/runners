# App

The Windows app's Cargo workspace under [`app/`](../../app): the one executable and
its modes, the debug-only test variables, how the app connects to its agent and what
it does when the agent is gone, the logs, what the window, menu and icon show for a
snapshot, the notification-area icon and the app icon, and the app's tests and what
they do and do not show.

[CLI](./cli.md) owns the library the app and the agent are built on: the agent's
state, the token store, protocol version 2 and the snapshot. [Security](../conventions/security.md)
owns the rules the app is reviewed against, including its processes and its
dependencies. [Windows Runner Host](./windows-runner-host.md) owns running axpc's
pools with the command line, which is how axpc runs them today.

## What Exists

The workspace holds the mode dispatch, the view, the session, the icons and their tests, but nothing builds the app's
executable, installs it or releases it. `marshal` is still built and released, and
axpc still runs its pools with it.

| Part                                                                      | State                                                                                          |
| ------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| The mode dispatch, the `marshal-modes` crate                              | Tested by `mise run check` on Linux, the `app-modes` job on macOS and the `app` job on Windows |
| What the window, menu and icon show, the `marshal-view` crate             | Tested by `mise run check` on Linux and the `app` job on Windows                               |
| The app's session with its agent and its log, the `marshal-session` crate | Tested by `mise run check` on Linux and the `app` job on Windows                               |
| The notification-area glyphs and the app icon, the `marshal-tray` crate   | Tested by `mise run check` on Linux and the `app` job on Windows                               |
| The agent's resident mode and protocol version 2                          | In the `marshal-cli` library, which the mode dispatch runs in its agent mode                   |
| Verification on axpc                                                      | Not done; see [Verification Status](#verification-status)                                      |

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
other, not an interface for a person or a script. The agent is started with
`DETACHED_PROCESS` (see Launch), which leaves it no console, and its flags are recorded
and asserted by a unit test. It has no console for its children to inherit, so each
`docker` client it starts and the exporter child get `CREATE_NO_WINDOW`;
[CLI](./cli.md#running-a-host) says where that is set and tested, which children
inherit the CLI's standard streams, and what only the axpc run shows, including whether
a window still appears. The agent mode is kept to one
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

## The Agent and the App's Session

The app's Rust side is a client of protocol version 2 and never the agent itself:
it does not hold the lock, read the configuration, touch the token store or run
Docker. One task in the `marshal-session` crate owns the connection, the pending
action and the view, and every seam it uses (the connection, the launcher, the clock,
the log and the presentation) is a trait, so its tests run with a paused clock and no
web view.

- **Launch.** The app connects to this user's agent. When none answers it starts its
  own executable, by absolute path, as a plain detached child with no console window
  (the flags `DETACHED_PROCESS`, which leaves it no console, and
  `CREATE_NEW_PROCESS_GROUP`; `CREATE_NO_WINDOW` is left out because Windows ignores
  it beside `DETACHED_PROCESS`), not through Tauri's shell plugin, so the agent outlives the app. A
  fresh launch starts the agent running its pools, because the launch is the user's
  own action. The restart after a dropped connection starts it stopped when the last
  state the app heard was "stopped by you", and running otherwise. It never starts
  an agent from a file that is missing, that it could not read when it started, or
  that is no longer the file the app started from.
- **A dropped connection** that is not a quit keeps the last list, marked with its
  time, under a lost-contact banner. The app starts the agent again once and then
  offers Try Again.
- **Not answering.** A channel that exists but does not complete the handshake within
  5 s shows "the agent is not answering". Quit then ends only the app, and the next
  start removes the ended agent's containers.
- **Busy.** An agent that answers `busy` is retried every 5 s.
- **Another process holds the lock** when the lock is held and this user's channel
  does not answer protocol version 2, for example a stray copy of the command line's
  `start`. The window says so, offers no action and checks again every 5 s.
- **The agent ends** in one of two ways the app tells apart by the `ending` message's
  reason. `quit` and `executable_changed` end the app as well: the first because the
  user asked for it, the second because the file the agent started from was moved,
  renamed, replaced or deleted. An `ending` with any other reason is logged and
  ignored, because a later agent may add reasons under the version rules in
  [CLI](./cli.md#protocol-version-2).
- **Quit** stops every runner as Stop all does, cancelling a build first, then ends
  the agent and the app. After a successful quit the session can end as the agent's
  `quit` ending or as its own quit, depending on which arrives first; both end the
  app. A quit the agent does not answer in 45 s while the connection is open does
  not end the app: the window keeps saying it is quitting, the log notes it, and the
  app ends when the agent's `ending` arrives or the connection closes. An agent that
  hangs therefore leaves the app quitting until its process is ended in Task
  Manager. A Quit while an attempt to reach the agent is under way, at launch or
  after a drop, may have started the agent, so it waits for the attempt: an agent it
  reaches is quit, and with none the app ends alone.
- **A call that takes too long.** Start all, Stop all, Build and a token call each
  wait a bounded time for the agent's answer (40 s, and 10 s for a token). On a
  connection that is still open a call that overruns says the agent has not
  answered yet and leaves the list as it is, since the agent may still do it; it is
  never reported as lost contact, which is only a connection that ended. A token
  save or removal that overruns is neither saved nor refused: its row says the
  agent has not answered yet, its form or question closes, and the row's state
  then follows the snapshot.
- **The app ending** or being killed leaves the agent and its runners running, and
  the next launch reconnects to them.

A token travels to the agent only in `token_set`. The session trims
it, refuses an empty one or one over 1280 UTF-16 code units before sending, wraps it
in the library's `Secret`, sends it once and drops it; it is in no field, log line,
error text or view. A removal that the agent refuses, or that is asked for with no
connection, shows as not removed, with the token still stored.

## Logs

The agent writes its log where `start` does, `%LOCALAPPDATA%\marshal\logs\`, in files
named `marshal-<time>-<pid>.log`, continued in a new file at 10 MiB with the newest
ten kept. The app writes to the same folder under its own name, `app-<time>-<pid>.log`,
with the same limits. A log that cannot write stops and drops its lines for 30 s
before it tries a new file, as the agent's does, so a full disk does not replace the
kept files with empty ones; the first line of the next file says how many lines were
not written. Each side prunes only its own files, so neither takes the
other's place, and a test shows it. Every line passes through the
view crate's `plain`, which also replaces a word shaped like a GitHub token, though
neither the app nor the agent logs one.

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

The agent gives a skipped pool's reason after its place in the configuration, as
`repositories[0]: no token.`, and the view reads the cause after that place. A pool
skipped for want of a token says "No token for owner/app. Start All skipped this
pool." with Set Token. A pool skipped because the token store failed, or because there
is none, keeps the agent's reason on its card, and its repository's row shows the
store's words, such as "The token store failed: access was refused (kind
access_refused, code 5).", instead of "No token", with no "No repository has a token
yet" banner; Set Token stays, since a save goes through the same store and says if it
fails too. A token save or removal the agent has not answered says so, and the row
then follows the snapshot, since the agent may still do it.

From `app/`, `cargo run -p marshal-view --example dump_view -- <fixture.json>` prints a
recorded snapshot's view as JSON, with options to set the connection, a pending
action, an open token form, a token result and the settled count, which the example's
header comment lists.

## The Icons

The notification-area icon has five states, all up, some not up, stopped, stopped by
an error and Marshal not in control of the runners, each in white for a dark taskbar
and in near-black for a light one, at 16, 20, 24, 28 and 32 px. The images are
masks in text, [`white.txt`](../../app/crates/tray/glyphs/white.txt) and
[`near-black.txt`](../../app/crates/tray/glyphs/near-black.txt) under
`app/crates/tray/glyphs/`, embedded with `include_str!` and turned into RGBA when
used, so the app links no image decoder. A file holds 25 blocks, one for each state
and size: a line `<state> <size>`, then `size` rows of `size` characters, where a hex
digit is a pixel's alpha in fifteenths (`f` is opaque) and `.` is none. The shapes were laid out for
each size on its own pixel grid, not scaled, and the white set is drawn a little
lighter than the near-black set because white on a dark ground looks heavier.

To change a glyph, edit the text in both files and run `mise run test:app`. The
tests in `app/crates/tray/tests/glyphs.rs` fail on a missing or malformed block, an
edge pixel, a mostly blank image or two states whose glyphs of one colour differ in
fewer than 12 whole pixels at any size, which is how the five states stay apart. The tray crate's
`Watch` replaces the icon's image only when the icon's state, the taskbar's mode or
the primary display's scaling changes. An unreadable or absent mode counts as dark,
and is logged once.

Two choices go beyond the design record and are the maintainer's to confirm. The
record draws stopped by an error as a red disc with an exclamation mark; the glyph
is an exclamation mark inside the outline frame in the glyph's own colour, because
the images have two colours only and one mask format. The app icon is the record's
Iris tile with three lines, with 20 and 40 px sizes added, not an outline glyph.

The app icon's source is
[`app-icon.txt`](../../app/crates/tray/glyphs/app-icon.txt). From `app/`,
`cargo run -p marshal-tray --example icons -- app-icon crates/app/icons` writes
`icon.ico` and `icon.png`, at 16, 20, 24, 32, 40 and 48 px, and
`cargo run -p marshal-tray --example icons -- sheet <file.png>` writes the icon
sheet: every state at 16 to 32 px in both colours on a light and a dark taskbar, two
mismatched pairings, and the app icon. The files are written without compression so
the output is the same every time, and `tests/app_icon.rs` fails if the checked-in
icon files differ from what the source produces. The maintainer approves the sheet
before the app's pull request is ready; the approval is recorded on the tracking
issue, not here.

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
- **Not shown by any check:** that no console window appears from the agent, the
  exporter or their `docker` clients (the flags are asserted at the command builders, and the
  window is not observable without `unsafe`), and that a terminal `marshal build` on
  Windows still streams `docker build`'s output with `CREATE_NO_WINDOW`, which is
  checked on axpc. The axpc results are recorded on the
  tracking issue, not here.
