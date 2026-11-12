# App

The Windows notification-area app under [`app/`](../../app) and the resident agent
it starts: the one executable and its modes, how the app connects to its agent and
what it does when the agent is gone, the logs, the commands and events between the
Rust side and the web view, the notification-area icon and the app icon, the bundled
font and colour values, the licence notices, how the app is built and which pin its
build tool has, the app's tests and which of them run only on Windows CI, the
install layout the installer will create, and how to go back to the command line.
It is the document other children of the roadmap
([#72](https://github.com/axross/runners/issues/72)) add their own sections to.

[CLI](./cli.md) owns the library the app and the agent are built on: the agent's
state, the token store, protocol version 2 and the snapshot. [Security](../conventions/security.md)
owns the rules the app is reviewed against, including its web view, its capabilities
and its dependencies. [Windows Runner Host](./windows-runner-host.md) owns running
axpc's pools with the command line, which is how axpc runs them today.

## What Exists

The app builds and its tests run, but nothing installs it and nothing releases it.
The installer, the release and the removal of the command line's commands are W1's
installer pull request (pull request 3); W1, [#73](https://github.com/axross/runners/issues/73), is the child that owns the app.
Until it merges, `marshal` is still built and released, axpc still runs its pools
with it, and this document describes an app that is built in CI and run from a
build, not one a host has installed.

| Part                                                 | State                                                                                |
| ---------------------------------------------------- | ------------------------------------------------------------------------------------ |
| The app, its menu and its main window                | Built by the Windows job of `merge-checks.yaml`; its build is kept as a run artifact |
| The agent's resident mode and protocol version 2     | In the `marshal-cli` library, served by the app's executable in its agent mode       |
| The token store                                      | The Windows Credential Manager, entered in the main window                           |
| The installer, the release, the install layout below | Planned for W1's installer pull request (pull request 3)                             |
| The command line's removal                           | Planned for the same pull request                                                    |
| Verification on axpc                                 | Not done; see [Verification Status](#verification-status)                            |

## The Executable and Its Modes

Marshal is one executable, `marshal-app.exe` once it is built for Windows. Its first
argument decides its mode before Tauri starts, in the `marshal-modes` crate:

| Arguments                                  | Mode                                                                                         |
| ------------------------------------------ | -------------------------------------------------------------------------------------------- |
| none, or an argument that is not one below | The app: the notification-area icon, its menu and the main window                            |
| `agent running`, `agent stopped`           | The agent: the library's resident agent, started running the pools or waiting to be told to  |
| `export` and its arguments                 | The diagnostics exporter child, which the agent starts for a diagnostic container's evidence |
| `shim` and its arguments                   | On Unix only, the shim a diagnostic container's `docker run` goes through                    |
| `vm-shim` and its arguments                | On Unix only, the shim that holds one Tart VM's `tart run`, for the axmc runtime             |
| `agent` with anything else                 | Refused with exit status 2                                                                   |

The hidden modes are internal. They have no window, no icon and no console, and no
compatibility promise: their arguments are what the app and the library pass each
other, not an interface for a person or a script. A release build is a Windows
subsystem executable, so the app and the modes it starts have no console of their
own, and the agent is started with `DETACHED_PROCESS` (see Launch), which leaves it
no console. It has no console for its children to inherit, so each `docker` client it
starts and the exporter child get `CREATE_NO_WINDOW`, set by the library's two command
builders and asserted by a unit test at each; the agent's own flags are recorded
and asserted by a unit test too. Their process groups are unchanged. Most clients
have piped or closed standard streams, but in terminal mode `marshal build`'s
`docker build` inherits the CLI's standard output and standard error, and the
exporter child inherits its standard error. What a terminal run does with the flag on
Windows, including whether a terminal `marshal build` still streams its output, is to
be confirmed by the Windows CI tests and the axpc run (see
[CLI](./cli.md#running-a-host)). Whether a
window still appears is shown only on axpc. Only the app mode takes Tauri's single-instance lock, so a second
launch brings the first one's window forward and starts nothing. The agent mode is
kept to one by the machine-wide lock the library's agent takes, as `start` is, so
the app and the command line never run the pools together.

A `fake-docker` binary is built beside `marshal-app` from the CLI's
[`cli/helpers/fake_docker.rs`](../../cli/helpers/fake_docker.rs), so the tests that
start this executable's exporter child can run it against a fake engine. The package
sets `default-run = "marshal-app"`, so `cargo run` still starts the app. The helper
is built by every build of the package and only the app's own executable is uploaded
or, later, released.

### The Debug-Only Test Variables

A debug build reads three variables in the agent mode, and the app mode reads the
first, so a test can point the executable at a scratch host:

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
runner with `cargo test --release`, as a Linux step runs the library's own options in
that profile. `mise run check` runs the crate's tests in the debug profile only.

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
  hangs therefore leaves the app quitting until `marshal-app.exe` is ended in Task
  Manager. A Quit while an attempt to reach the agent is under way, at launch or
  after a drop, may have started the agent, so it waits for the attempt: an agent it
  reaches is quit, and with none the app ends alone.
- **A call that takes too long.** Start all, Stop all, Build and a token call each
  wait a bounded time for the agent's answer (40 s, and 10 s for a token). On a
  connection that is still open a call that overruns says the agent has not
  answered yet and leaves the list as it is, since the agent may still do it; it is
  never reported as lost contact, which is only a connection that ended.
- **The app ending** or being killed leaves the agent and its runners running, and
  the next launch reconnects to them.

A token travels from the window to the agent only in `token_set`. The session trims
it, refuses an empty one or one over 1280 UTF-16 code units before sending, wraps it
in the library's `Secret`, sends it once and drops it; it is in no field, log line,
error text or view.

## Logs

The agent writes its log where `start` does, `%LOCALAPPDATA%\marshal\logs\`, in files
named `marshal-<time>-<pid>.log`, continued in a new file at 10 MiB with the newest
ten kept. The app writes to the same folder under its own name, `app-<time>-<pid>.log`,
with the same limits. Each side prunes only its own files, so neither takes the
other's place, and a test shows it. A second launch of the app writes no file, because
the single-instance plugin ends it before the log opens. Every line passes through the
view crate's `plain`, which also replaces a word shaped like a GitHub token, though
neither the app nor the agent logs one. If the app's own file cannot be made the app
still runs, and a warning goes to the standard error stream.

## Commands and Events

The web view calls the Rust side through eleven commands, declared in
[`build.rs`](../../app/crates/app/build.rs), each with a permission file under
[`permissions/autogenerated/`](../../app/crates/app/permissions/autogenerated) that
`tauri-build` writes and the repository keeps. A window can call a command only when
its capability grants it, and two capabilities, each limited to one window, grant
nothing else:

| Command                                                    | Called by               | Does                                                                                            |
| ---------------------------------------------------------- | ----------------------- | ----------------------------------------------------------------------------------------------- |
| `read_view(forms?)`                                        | The main window         | Returns the current view and its revision; `forms` tells the Rust side which token form is open |
| `start_all`, `stop_all`, `build`, `stop_build`             | The main window         | The action of the same name, shown at once as pending                                           |
| `try_again`                                                | The main window         | Connects again after lost contact                                                               |
| `set_token(repository, token)`, `remove_token(repository)` | The main window         | Sends `token_set` or `token_remove`; the token is trimmed and checked first                     |
| `hide_window`                                              | The main window         | Hides the window; the icon and the agent keep running                                           |
| `read_confirmation`                                        | The confirmation window | Returns the question the menu asked                                                             |
| `answer_confirmation(accepted)`                            | The confirmation window | Runs the action or quits when `accepted`, then closes the window                                |

The capabilities are [`main-window.json`](../../app/crates/app/capabilities/main-window.json),
which also grants `core:event:allow-listen` and `core:event:allow-unlisten`, and
[`confirmation-window.json`](../../app/crates/app/capabilities/confirmation-window.json).
`tests/ipc.rs` runs both windows against Tauri's mock runtime and shows that a call
the capability does not grant is refused and that the granted ones are not. Quit runs
from the menu on the Rust side and has no command. `commands::handler` is the one
list of the eleven commands the app and `tests/ipc.rs` both register.

A failed action that a confirmation started is written to the app's log, since the
window that asked is gone by then.

Three events reach the main window. The view reaches the page by the `marshal:view`
event and by the reply to `read_view`, which nothing orders against each other, so each
carries a `revision` that grows by one with every view the session shows. The page
draws a view only when no newer one has been drawn, and still says what an older one
announces; `tests/frontend/revision.test.mjs` tests the rule, and the session tests
that the revisions only grow.

| Event             | Payload                                            | Meaning                                                                   |
| ----------------- | -------------------------------------------------- | ------------------------------------------------------------------------- |
| `marshal:view`    | `{view, announce[], revision}`                     | The view changed; each `announce` string goes to the live region in order |
| `marshal:confirm` | `{action, question{title, body, confirm, cancel}}` | A menu choice needs a question; `action` is `stop_all` or `stop_build`    |
| `marshal:hidden`  | none                                               | The main window is being hidden; the page clears every open token field   |

The app tells the page `marshal:hidden` just before it hides the main window, from
the one function every hide goes through: the close request, which hides the window
instead of closing it, and the `hide_window` command that Esc calls. The page needs
no permission beyond `core:event:allow-listen` for it. `tests/ipc.rs` shows that both
sites emit it, and `tests/frontend/save-token.test.mjs` that the page clears its
fields on it.

The confirmation window, label `confirmation`, is 440 by 240 px and always on top. It
opens when a menu choice needs a question and the main window is hidden or minimised, and for Quit
whether or not the main window is shown, because the main window's capability has no
command that answers a quit. One question shows at a time: while one is open it is
brought forward and a new one is dropped.

## The Window's Page

The windows load only bundled files from
[`app/crates/app/frontend/`](../../app/crates/app/frontend), with no npm package, as
ES modules, with no inline script, no `style` attribute and no `eval`. The content
security policy is in [`tauri.conf.json`](../../app/crates/app/tauri.conf.json) and
[Security](../conventions/security.md#the-app-surface) states it. What the window and
menu show for a snapshot is computed in the `marshal-view` crate, which has no
operating system dependency; the page only draws the view it is given and sets every
string through `textContent`.

A runner row writes its state out beside a mark: "Container running" with a check,
"Registering" with a ring, "Backing off" and "Held" with the history mark, and
"Stopped" with a square. A backing-off or held runner shows the agent's reason under
its state, and a held one is a runner whose job host a host prerequisite refuses, so
its reason says which. A state the app does not know shows the runner's own reason as
its words, with a dash, or the state's word when there is no reason, as the version
rules in [CLI](./cli.md#version-rules) require.

| File                                                | Holds                                                                                                                               |
| --------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------- |
| `index.html`, `main.js`, `revision.js`              | The page, the first `read_view`, the `marshal:view`, `marshal:confirm` and `marshal:hidden` listeners; the order views are drawn in |
| `render.js`, `pools.js`, `tokens.js`                | The header, tabs and footer; the pool cards, runner rows and diagnostics; the token rows                                            |
| `controls.js`, `tooltip.js`, `dialog.js`, `live.js` | Buttons that stay focusable when unavailable; the balloon; the confirmation; the live region                                        |
| `dom.js`, `icons.js`                                | The element builder, which refuses `style`, `href`, `src`, `action` and `on*` attributes, and the inline icons                      |
| `main.css`, `tokens.css`, `fonts.css`               | The window stylesheet; the colour tokens; the one place that names the font files                                                   |
| `confirmation.html`, `confirmation.js`              | The confirmation window's page, with Cancel focused                                                                                 |

Esc closes the first of these that is open and does nothing more: a tooltip balloon,
then a confirmation dialog, then an open token form or removal question, and only then
hides the window. The page clears a token field as soon as it calls `set_token`,
whatever the outcome, and keeps the token in no variable; `save-token.test.mjs` loads
the page over a stand-in for the document and drives that path with a call that
resolves and one that rejects. It also clears every open token field when the app
sends `marshal:hidden`, which it does at every place it hides the window, and when
the web view reports the page hidden, a second route; the form stays open and empty.

From `app/`, `cargo run -p marshal-view --example dump_view -- <fixture.json>` prints a
recorded snapshot's view as JSON, with options to set the connection, a pending
action, an open token form, a token result and the settled count, which the example's
header comment lists, so the pages can be rendered outside the app.

### Fonts and Colours

IBM Plex Sans is bundled as three WOFF2 files under `frontend/fonts/`, so nothing
is downloaded at run time. [`fonts/README.md`](../../app/crates/app/frontend/fonts/README.md)
records the release, its source, the archive's SHA-256 and each file's, and says how
to check and update them; `.gitattributes` keeps the files byte for byte. The colour
values in `tokens.css` are copied from Radix Colors 3.0.0, and the file's header
records the package, its licence and the roles each theme fills; the package's MIT
notice is kept as `colors-LICENSE.txt` beside it. Both licences ship in the licence
notices below. A font or colour update changes the record in the same commit.

## The Icons

The notification-area icon has five states, all up, some not up, stopped, stopped by
an error and Marshal not in control of the runners, each in white for a dark taskbar
and in near-black for a light one, at 16, 20, 24, 28 and 32 px. The images are
masks in text, [`white.txt`](../../app/crates/tray/glyphs/white.txt) and
[`near-black.txt`](../../app/crates/tray/glyphs/near-black.txt) under
`app/crates/tray/glyphs/`, embedded with `include_str!` and turned into RGBA when
used, so the app links no image decoder. A file holds 25 blocks, one for each state
and size: a line `<state> <size>`, then `size` rows of `size` characters, where a hex
digit is a pixel's alpha in sixteenths and `.` is none. The shapes were laid out for
each size on its own pixel grid, not scaled, and the white set is drawn a little
lighter than the near-black set because white on a dark ground looks heavier.

To change a glyph, edit the text in both files and run `mise run test:app`. The
tests in `app/crates/tray/tests/glyphs.rs` fail on a missing or malformed block, an
edge pixel, a mostly blank image or two states whose white glyphs differ in fewer
than 12 whole pixels at any size, which is how the five states stay apart. The app polls the
taskbar's mode and the primary display's scaling once a second and replaces the
icon's image only when one changes. An unreadable or absent mode counts as dark, and
is logged once. A change is put on the icon from the event loop's thread, which reads
the newest look decided when it runs (`Watch::current`) and not the one that queued
it, so two looks decided on two threads and queued out of order leave the newer one
on the icon.

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

## Licence Notices

The Windows installer will install a notices file, so the notices are checked in.
[`app/notices/`](../../app/notices) holds one file for each tree the app is built
for, `THIRD-PARTY-NOTICES-windows.txt` for `x86_64-pc-windows-msvc` and
`THIRD-PARTY-NOTICES-macos.txt` for `aarch64-apple-darwin`, and what writes them:

| File or folder              | Holds                                                                                                       |
| --------------------------- | ----------------------------------------------------------------------------------------------------------- |
| `generate.sh`               | `write` or `check`: runs `cargo about generate --frozen --fail` for each tree, then appends the files below |
| `about.toml`, `notices.hbs` | cargo-about's accepted licences, which are `deny.toml`'s allowlist and its two exceptions, and its template |
| `licences/`                 | The WebView2 loader's licence and `webview2-com-sys`'s, for the Windows file only                           |

Each file lists every crate the executable links, grouped by licence, with each
MPL-2.0 crate's published source named; then IBM Plex Sans, Radix Colors and, for
Windows, the WebView2 loader. `mise run check:notices` regenerates both files and
fails on any difference from the checked-in ones, so a lockfile change that adds,
drops or bumps a crate fails the Check job until `mise run notices:app` writes them
again and the change commits them. Each file's header names the cargo-about version,
so another version fails the check too.

`cargo-about` is a mise tool pinned in [`mise.toml`](../../mise.toml) and locked in
`mise.lock` like the others, installed from its release as a prebuilt binary, so the
notices need no compile in CI. It runs with `--frozen`, offline, so a crate that
ships no licence file gets the licence's standard text with its template line
`Copyright (c) <year> <copyright holders>`. cargo-about also prints that line for
some crates whose licence file it does not read the copyright line from; `about.toml`
clarifies those, the `windows-rs` crates, `dpi` and `tauri-plugin-single-instance`,
naming each crate's own licence files with their SHA-256. A bump that changes such a
file no longer matches its checksum, the template line comes back in the regenerated
notices, and the clarification is updated to the new file in the same change.
The crate `webview2-com-sys` ships no licence file; its MIT notice is the `LICENSE`
at the git commit its 0.39.1 crate was published from (`edc2caf`, in
`wravery/webview2-rs`, recorded in the crate's `.cargo_vcs_info.json`). The loader's text is Microsoft's `LICENSE.txt` from the NuGet
package `Microsoft.Web.WebView2` 1.0.3800.47, which is the SDK that
`webview2-com-sys` 0.39.1 bundles; it was identified by matching the SHA-256 of
`x64/WebView2LoaderStatic.lib` across NuGet releases. `generate.sh` fails when any
licence file it appends, these two included, is missing or empty, so a notice cannot
ship as a heading over nothing. When `webview2-com-sys` is
bumped, compare the new bundled loader's SHA-256 with the NuGet releases the same
way and, if the SDK changed, replace the loader's licence text and the version in
`generate.sh` in the same commit.

## Building the App

The app's workspace is built with Cargo like the CLI, and the Tauri crate also needs
the Windows web view, so the whole workspace builds only on Windows. The CLI document
lists the gates that run without it under [The App Workspace](./cli.md#the-app-workspace).

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
never reaches `cargo install`. The Windows job installs `tauri-cli` with
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
[Security](../conventions/security.md#pinning-convention) records why the install
is an exception to the checksum rule.

The Windows job's timeout is 45 minutes, twice the 20 min 37 s the whole job took on
[#95](https://github.com/axross/runners/pull/95)'s green run, rounded up to whole 5
minutes.

## Configuration and Tokens

The agent reads the JSON configuration from `%LOCALAPPDATA%\marshal\config.json` on
Windows and `~/Library/Application Support/marshal/config.json` on macOS, which is
also the macOS control socket's folder, so the agent keeps it private (0700). K1
replaces the JSON with TOML at the same place. The agent reads each pool's token
from the token store when it admits the pool and at each registration, never from a
file: it ignores the configuration's `tokenPath`, which the shared validation still
requires until K1's TOML has none. On Windows a token is a Credential Manager entry
for the repository, entered in the main window's Tokens tab; the app never shows a
stored token and never reads the store itself. Before the Mac has a store, the agent
skips every pool there with "no token store".

Today's token files are not imported. A host that moves from the command line to the
app enters each repository's token in the window, once.

## Install Layout

This is planned, and holds until W1's installer pull request (pull request 3) merges the installer;
no host has it installed.

| What                                  | Where                                                                             |
| ------------------------------------- | --------------------------------------------------------------------------------- |
| Marshal (app, agent and hidden modes) | `%LOCALAPPDATA%\Programs\marshal\marshal-app.exe`                                 |
| Licence notices                       | `%LOCALAPPDATA%\Programs\marshal\THIRD-PARTY-NOTICES.txt`, the Windows file above |
| Start menu entry                      | Marshal, starting the app                                                         |
| Configuration (default)               | `%LOCALAPPDATA%\marshal\config.json`                                              |
| Logs                                  | `%LOCALAPPDATA%\marshal\logs\`, the app's files under their own name              |
| Tokens                                | Credential Manager, one generic credential per repository                         |
| Web view data                         | `%LOCALAPPDATA%\app.axross.marshal\`, named by the bundle identifier              |

The bundle identifier is `app.axross.marshal`, the same as the signing identifier
that [CLI](./cli.md#the-signing-contracts) fixes, because the app and the agent are
one code object. The name `marshal-app.exe` never collides with a leftover
`marshal.exe`, and Task Manager tells the two apart.

## Rolling Back

axpc runs its pools with the released `marshal` today, so a rollback has nothing to
undo yet. Once the installer exists, a rollback installs the previous release. The
PowerShell host scripts are gone, so a rollback never restores them; before the
installer, the previous release is the last command-line release. Going back from the
app to the command line takes three things the app does not do for you:

- **Quit Marshal first**, so no agent holds the lock and the command line's `start`
  can take it. If the app does not answer, end `marshal-app.exe` in Task Manager; the
  next start removes the containers it left.
- **Keep the token files** until the app's runners are working. No code path of the
  app reads them, and the command line reads them only while it exists, so a rollback
  finds them as they were.
- **Keep the configuration.** The command line reads its own `--config` path, and the
  app never changes that file, so a rollback needs no configuration change.

## Verification Status

What CI and the checks show, and what only a real axpc session can, so a reader
does not take a build for a host that has run it:

- **Run on every pull request:** the checks of [The App Workspace](./cli.md#the-app-workspace)
  on Linux, the Windows job's clippy, tests and `cargo tauri build --no-bundle`, and
  the mode dispatch on macOS, in the debug profile and then in the release profile.
  The Windows job runs the tests with `--no-fail-fast`, so one test executable that
  fails shows every other executable's result in the same run.
- **Run only by the Windows job:** the test executables loading with the common
  controls manifest of
  [The Build Script and the Test Manifest](#the-build-script-and-the-test-manifest);
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
  build only in debug, for the test variables above.
- **Not shown by any check:** that no console window appears from the agent, the
  exporter or their `docker` clients (the flags are asserted at the command builders,
  and the window is not observable without `unsafe`), that a terminal `marshal build`
  on Windows still streams `docker build`'s output with `CREATE_NO_WINDOW`, which is
  checked on axpc, WebView2 itself, a Windows contrast
  theme, the icon on a real taskbar and its weight at 16 px, the menu opening from the
  keyboard and where its focus goes, the icon following a mode or a scaling change
  within 2 s, a token round trip through the Credential Manager to a registration,
  Narrator, and containers after a sign-out. The Chromium renders of the pages, in
  both themes at 880, 721, 720 and 480 px, ran outside the repository and are not a
  gate. The axpc results are recorded on the tracking issue, not here.

## Sections Other Children Add

Each child below adds its own section to this document, with the contract it owns,
and does not edit another's:

| Child | Adds                                                                                                          |
| ----- | ------------------------------------------------------------------------------------------------------------- |
| W2    | Showing each runner's GitHub status in the window                                                             |
| K1    | The TOML configuration, the file's status in the footer, and running on the last valid configuration          |
| W4    | Automatic updates, the drain, its `ending` reason and its checksum of the `tauri-cli` crate archive           |
| M2    | The Mac app: its disk image, its Keychain store behind the token store's interface and the Mac's stop reasons |

A new message under protocol version 2 follows the version rules in
[CLI](./cli.md#protocol-version-2) and is added there, not here.
