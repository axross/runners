# App

The Windows notification-area app under [`app/`](../../app) and the resident agent
it starts: the one executable and its modes, how the app connects to its agent and
what it does when the agent is gone, the logs, the commands and events between the
Rust side and the web view, the notification-area icon and the app icon, the bundled
font and colour values, the licence notices, how the app is built and which pin its
build tool has, the app's tests and which of them run only on Windows CI, the
install layout the installer creates, and how to go back to the command line.
It is the document other children of the roadmap
([#72](https://github.com/axross/runners/issues/72)) add their own sections to.

[CLI](./cli.md) owns the library the app and the agent are built on: the agent's
state, the token store, protocol version 2 and the snapshot. [Security](../conventions/security.md)
owns the rules the app is reviewed against, including its web view, its capabilities
and its dependencies. [Windows Runner Host](./windows-runner-host.md) owns running
axpc's pools with the command line, which is how axpc runs them today.

## What Exists

The app builds, its tests run, and since W1's installer pull request (pull request 3)
each release carries it and its per-user MSI, as [CLI](./cli.md#releases) describes,
in place of the command line. W1, [#73](https://github.com/axross/runners/issues/73),
is the child that owns the app. No host has installed it yet: axpc still runs its
pools with the [last command-line release](./cli.md#the-last-command-line-release),
so this document describes an app that CI builds and tests and the release ships,
not one a host has run. The same pull request removed the command line: no `marshal`
binary is built on any operating system, and the app is the only way to run a host.

| Part                                                 | State                                                                                       |
| ---------------------------------------------------- | ------------------------------------------------------------------------------------------- |
| The app, its menu and its main window                | Built by the Windows job of `merge-checks.yaml`; its build is kept as a run artifact        |
| The agent's resident mode and protocol version 2     | In the `marshal-cli` library, served by the app's executable in its agent mode              |
| The token store                                      | The Windows Credential Manager, entered in the main window                                  |
| The installer, the release, the install layout below | Built and tested by the release build on every pull request; see [CLI](./cli.md#installers) |
| Updates                                              | The app's side, with the committed public key; the feed comes with W4's last pull request   |
| The command line's removal                           | Done in W1's installer pull request; the last command-line release keeps it                 |
| Verification on axpc                                 | Not done; see [Verification Status](#verification-status)                                   |

## The Executable and Its Modes

Marshal is one executable, `marshal-app.exe` once it is built for Windows. Its first
argument decides its mode before Tauri starts, in the `marshal-modes` crate:

| Arguments                                  | Mode                                                                                         |
| ------------------------------------------ | -------------------------------------------------------------------------------------------- |
| none, or an argument that is not one below | The app: the notification-area icon, its menu and the main window                            |
| `--sign-in`                                | The app with its icon only, as the sign-in item starts it; a second launch shows the window  |
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
no console; its flags are recorded and asserted by a unit test. It has no console for
its children to inherit, so each `docker` client it starts and the exporter child get
`CREATE_NO_WINDOW`; [CLI](./cli.md#running-a-host) says where that is set and tested,
which children inherit the CLI's standard streams, and what only the axpc run shows,
including whether a window still appears. Only the app mode takes Tauri's
single-instance lock, so a second launch brings the first one's window forward and
starts nothing. The agent mode is kept to one by the machine-wide lock the library's
agent takes, as the library's `start` is, so the app and the last command-line
release never run the pools together.

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
  fresh launch starts the agent stopped and sends it `launch` once connected, so the
  agent starts the runners as `start_runners_on_launch` says; an agent that does not
  list `launch` is sent `start_all`. A fresh launch that reaches an agent already
  running sends nothing. The window starts hidden and is shown unless the app was
  started with `--sign-in`. The restart after a dropped connection starts the agent
  stopped when the last state the app heard was stopped by you, by a sleep, by a
  network change or by too many resumes, and running otherwise. It never starts
  an agent from a file that is missing, that it could not read when it started, or
  that is no longer the file the app started from.
- **Sign-in.** While `start_at_login` is on in `config.toml`'s `[lifecycle]`, the
  agent keeps the current user's Run value `Marshal`, which starts the app with
  `--sign-in` (see [CLI](./cli.md#the-agents-signals)). The app is not yet installed
  or released, so this is unverified on axpc until W3's out-of-tree check. The
  app's installer does not remove the value yet: before uninstalling, turn the
  setting off, or delete `Marshal` under
  `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` afterwards.
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
  never reported as lost contact, which is only a connection that ended. A token
  save or removal that overruns is neither saved nor refused: its row says the
  agent has not answered yet, its form or question closes, and the row's state
  then follows the snapshot.
- **The app ending** or being killed leaves the agent and its runners running, and
  the next launch reconnects to them.

A token travels from the window to the agent only in `token_set`. The session trims
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
other's place, and a test shows it. A second launch of the app writes no file, because
the single-instance plugin ends it before the log opens. Every line passes through the
view crate's `plain`, which also replaces a word shaped like a GitHub token, though
neither the app nor the agent logs one. If the app's own file cannot be made the app
still runs, and a warning goes to the standard error stream.

## Commands and Events

The web view calls the Rust side through twelve commands, declared in
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
| `open_job(pool, runner)`                                   | The main window         | Opens the job that runner runs on github.com; see [GitHub Status](#github-status-in-the-window) |
| `read_confirmation`                                        | The confirmation window | Returns the question the menu asked                                                             |
| `answer_confirmation(accepted)`                            | The confirmation window | Runs the action or quits when `accepted`, then closes the window                                |

The capabilities are [`main-window.json`](../../app/crates/app/capabilities/main-window.json),
which also grants `core:event:allow-listen` and `core:event:allow-unlisten`, and
[`confirmation-window.json`](../../app/crates/app/capabilities/confirmation-window.json).
`tests/ipc.rs` runs both windows against Tauri's mock runtime and shows that a call
the capability does not grant is refused and that the granted ones are not. Quit runs
from the menu on the Rust side and has no command. `commands::handler` is the one
list of the twelve commands the app and `tests/ipc.rs` both register.

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
"Saving diagnostics" with a check while the runner's diagnostic container is
finalising, "Registering" with a ring, "Backing off" and "Held" with the history mark,
and "Stopped" with a square. A backing-off or held runner shows the agent's reason under
its state, and a held one is a runner whose job host a host prerequisite refuses, so
its reason says which. A state the app does not know shows the runner's own reason as
its words, with a dash, or the state's word when there is no reason, as the version
rules in [CLI](./cli.md#version-rules) require.

The agent gives a skipped pool's reason after its place in the configuration, as
`pool[0]: no token.` (`repositories[0]` from an agent that read `config.json`), and the view
reads the cause after that place. A pool
skipped for want of a token says "No token for owner/app. Start All skipped this
pool." with Set Token. A pool skipped because the token store failed, or because there
is none, keeps the agent's reason on its card, and its repository's row shows the
store's words, such as "The token store failed: access was refused (kind
access_refused, code 5).", instead of "No token", with no "No repository has a token
yet" banner; Set Token stays, since a save goes through the same store and says if it
fails too. A token save or removal the agent has not answered says so, and the row
then follows the snapshot, since the agent may still do it.

| File                                                | Holds                                                                                                                                  |
| --------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------- |
| `index.html`, `main.js`, `revision.js`              | The page, the first `read_view`, the `marshal:view`, `marshal:confirm` and `marshal:hidden` listeners; the order views are drawn in    |
| `render.js`, `pools.js`, `tokens.js`                | The header, tabs and footer; the pool cards, runner rows and diagnostics; the token rows                                               |
| `controls.js`, `tooltip.js`, `dialog.js`, `live.js` | Buttons that stay focusable when unavailable; the balloon; the confirmation; the live region                                           |
| `dom.js`, `icons.js`                                | The element builder, which refuses `style`, `href`, `src`, `srcdoc`, `action`, `formaction` and `on*` attributes, and the inline icons |
| `main.css`, `tokens.css`, `fonts.css`               | The window stylesheet; the colour tokens; the one place that names the font files                                                      |
| `confirmation.html`, `confirmation.js`              | The confirmation window's page, with Cancel focused                                                                                    |

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
action, an open token form, a token result, the settled count, and a recorded `github`
value or an agent without one, which the example's header comment lists, so the pages
can be rendered outside the app.

### Fonts and Colours

IBM Plex Sans is bundled as three WOFF2 files under `frontend/fonts/`, so nothing
is downloaded at run time. [`fonts/README.md`](../../app/crates/app/frontend/fonts/README.md)
records the release, its source, the archive's SHA-256 and each file's, and says how
to check and update them; `.gitattributes` keeps the files byte for byte. The colour
values in `tokens.css` are copied from Radix Colors 3.0.0, and the file's header
records the package, its licence and the roles each theme fills; the package's MIT
notice is kept as `colors-LICENSE.txt` beside it. Both licences ship in the licence
notices below. A font or colour update changes the record in the same commit.

A token field has a 1 px border in `--border-neutral-unselected-control`, which
turns `--text-destructive-low` when the field is invalid; a Windows contrast theme
keeps its own system colours. No check measures contrast, so the ratios, by the WCAG
formula, against the card (`--list-card-fill`) and the field's fill
(`--component-neutral-rest`) are recorded here, all above the 3:1 a control's
boundary needs; a change to these tokens measures them again:

| Theme | State   | Border    | Against the card | Against the fill |
| ----- | ------- | --------- | ---------------- | ---------------- |
| Light | Rest    | `#80838d` | 3.78:1           | 3.33:1           |
| Light | Invalid | `#ca244d` | 5.43:1           | 4.77:1           |
| Dark  | Rest    | `#696e77` | 3.43:1           | 3.10:1           |
| Dark  | Invalid | `#ff949d` | 8.36:1           | 7.56:1           |

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
fewer than 12 whole pixels at any size, which is how the five states stay apart. The app polls the
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

| File or folder              | Holds                                                                                                                                                                                                |
| --------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `generate.sh`               | `write` or `check`: runs `cargo about generate --frozen --fail` for each tree, checks its clarifications applied, fills in the copyright lines below, adds ring's part, then appends the files below |
| `about.toml`, `notices.hbs` | cargo-about's accepted licences, which are `deny.toml`'s allowlist and its two exceptions, and its template                                                                                          |
| `licences/`                 | The WebView2 loader's licence and the `webview2-rs` crates' licence, for the Windows file only                                                                                                       |

Each file lists every crate the executable links, and every procedural macro run
while it compiles, grouped by licence, with each MPL-2.0 crate's published source
named; then ring's part below, IBM Plex Sans, Radix Colors and, for Windows, the
WebView2 loader. `mise run check:notices` regenerates both files and
fails on any difference from the checked-in ones, so a lockfile change that adds,
drops or bumps a crate fails the Check job until `mise run notices:app` writes them
again and the change commits them. Each file's header names the cargo-about version,
so another version fails the check too.

`cargo-about` is a mise tool pinned in [`mise.toml`](../../mise.toml) and locked in
`mise.lock` like the others, installed from its release as a prebuilt binary, so the
notices need no compile in CI. It runs with `--frozen`, offline, so a crate that
ships no licence file gets the licence's standard text with its template line
`Copyright (c) <year> <copyright holders>`, and the notices never hold that line:
`generate.sh` refuses output with either placeholder, so `check:notices` and
`notices:app` fail on it. cargo-about also prints the line for some crates whose
licence file it does not read the copyright line from; `about.toml` clarifies those,
the `windows-rs` crates, `dpi`, `tauri-plugin-single-instance`,
`tauri-plugin-opener`, `tauri-plugin-updater` and `minisign-verify`, naming each
crate's own licence files with their SHA-256. It clarifies `ring` the same way, to its
`LICENSE-other-bits` (ISC) and `LICENSE-BoringSSL` (Apache-2.0), since cargo-about
would otherwise list each of its source files' licence headers as a text of its own.
ring's `LICENSE` says its new code is under the ISC text, that code from BoringSSL is
under the Apache-2.0 text or under ISC with the licence at the top of each file, and
that code from `once_cell` is under that polyfill's own `LICENSE-APACHE` and
`LICENSE-MIT`. So `generate.sh` adds a part for ring, from the source Cargo fetched:
its `LICENSE` in full, every distinct copyright line in its published source outside
its `tests` folder, `Portions Copyright` lines included, sorted, and the polyfill's MIT text.

cargo-about drops a clarification whose checksum no longer matches with only a
warning, so a bump that changes a clarified file would go unnoticed. `generate.sh`
looks for each clarified crate's known copyright line, held in the script as the
SHA-256 of the line without its surrounding blanks, among the lines of the licence
texts that name it (the line of each crate's clarified licence file: one for the
`windows-rs` crates, and one each for `dpi`, `tauri-plugin-single-instance`,
`tauri-plugin-opener`, `tauri-plugin-updater`, `minisign-verify` and `ring`), and
checks the crate appears under no more
texts than its clarification names files. It fails when either does not hold, or
when `about.toml` clarifies a crate it has no line for, until the clarification
names the new file.

Two groups ship no licence file, and `generate.sh` fills in their line itself:

- `webview2-com`, `webview2-com-macros` and `webview2-com-sys` come from one
  repository, `wravery/webview2-rs`. `licences/webview2-com-sys-LICENSE.txt` is its
  `LICENSE` at the git commit the 0.39.1 crate was published from (`edc2caf`,
  recorded in the crate's `.cargo_vcs_info.json`), and the three crates' notice takes
  its copyright line from that copy. `generate.sh` does so only while the copy's
  words, without that line, are the licence text cargo-about printed for them, and
  fails otherwise.
- The `objc2` crates, from `madsmtm/objc2`. The versions in `app/Cargo.lock` ship no
  licence file, and the repository's `LICENSE.md` at the commits they were published
  from (in each crate's `.cargo_vcs_info.json`) names no copyright holder; a later
  licence there, from commit `ee9a7ad`, names one, and applies once a release carries
  it. Their notice is the licence text without the template line, after one sentence
  saying so. `generate.sh` lists the eleven name and version pairs checked, so any
  other version of these crates keeps the template line and fails the run until its
  licence is checked again.

A crate outside these groups that gets the template line fails the run.

The Windows file's part for the WebView2 loader repeats `webview2-com-sys`'s notice
from the same copy. The loader's text is Microsoft's `LICENSE.txt` from the NuGet
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

The agent reads the TOML configuration from `%LOCALAPPDATA%\marshal\config.toml` on
Windows and `~/Library/Application Support/marshal/config.toml` on macOS, which is
also the macOS control socket's folder, so the agent keeps it private (0700).
[The Configuration File](#the-configuration-file) says what the window shows of it,
and [The Agent's Configuration](./cli.md#the-agents-configuration) how the agent
loads it. The agent reads each pool's token
from the token store when it admits the pool and at each registration, never from a
file: it ignores the configuration's `tokenPath`, which the shared validation still
requires of a JSON configuration and the TOML format does not have. On Windows a token is a Credential Manager entry
for the repository, entered in the main window's Tokens tab; the app never shows a
stored token and never reads the store itself. Before the Mac has a store, the agent
skips every pool there with "no token store".

Today's token files are not imported. A host that moves from the command line to the
app enters each repository's token in the window, once.

## The Configuration File

K1 ([#77](https://github.com/axross/runners/issues/77)) owns this section. The agent
runs `config.toml`, migrates a `config.json` it finds alone at its start, and keeps the
last valid configuration in use when a Start All finds the file broken or gone. The
window takes all of it from the snapshot's `configuration`: `path`, `problems`,
`status`, `in_use` and `migration`, as [The Snapshot](./cli.md#the-snapshot) lists
them. It adds no state, control or banner kind; the wording lives in the
`marshal-view` crate.

- **The footer** names `config.toml` and, after a migration, says when this agent
  migrated `config.json` and that `config.json` is no longer read.
- **A footer warning** stands beside the image warnings while the agent holds a
  configuration that is not the file's: the last valid one, naming the file's problems
  (or that it is missing) and when the configuration in use was read; or `config.json`'s,
  after a migration that could not write `config.toml`, with why. A migration that
  waits for the next start says to quit Marshal and launch it again.
- **The stopped banners** keep their roles. With no configuration the banner names
  `config.toml`, and says to launch Marshal again when `config.json` waits to be
  migrated. When `config.json` broke the rules at its migration, the invalid
  configuration banner names `config.json`, its problems in the JSON wording, and that
  a fix takes effect when Marshal starts again.

Build needs a configuration in use: it uses that configuration's image name and reads
no file. `app/crates/view/tests/view/configuration.rs` covers each wording, and the
recorded snapshots name `config.toml`.

**Rolling back.** A release without K1 reads `config.json` as it was when it was
migrated; changes made in `config.toml` since then are not carried back. After rolling
forward again, `config.toml` is read as it was, and edits made to `config.json` in the
meantime are ignored while `config.toml` exists. Delete `config.toml` to migrate
`config.json` again at the next start.

## Install Layout

W1's MSI creates this layout, as [CLI](./cli.md#installers) describes, and the build
leg's installer test measures it on every run; no host has it installed yet.

| What                                  | Where                                                                             |
| ------------------------------------- | --------------------------------------------------------------------------------- |
| Marshal (app, agent and hidden modes) | `%LOCALAPPDATA%\Programs\marshal\marshal-app.exe`                                 |
| Licence notices                       | `%LOCALAPPDATA%\Programs\marshal\THIRD-PARTY-NOTICES.txt`, the Windows file above |
| Start menu entry                      | Marshal, starting the app                                                         |
| Configuration (default)               | `%LOCALAPPDATA%\marshal\config.toml`                                              |
| Logs                                  | `%LOCALAPPDATA%\marshal\logs\`, the app's files under their own name              |
| Tokens                                | Credential Manager, one generic credential per repository                         |
| Web view data                         | `%LOCALAPPDATA%\app.axross.marshal\`, named by the bundle identifier              |

The bundle identifier is `app.axross.marshal`, the same as the signing identifier
that [CLI](./cli.md#the-signing-contracts) fixes, because the app and the agent are
one code object. The name `marshal-app.exe` never collides with a leftover
`marshal.exe`, and Task Manager tells the two apart.

## Rolling Back

A rollback uninstalls Marshal and installs the previous release, as
[CLI](./cli.md#updating-repairing-uninstalling-and-rolling-back) describes; before the
first app release, the previous release is the
[last command-line release](./cli.md#the-last-command-line-release), whose procedures
are at its tag. The PowerShell host scripts are gone, so a rollback never restores
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
  and the window is not observable without `unsafe`), WebView2 itself, a Windows contrast
  theme, the icon on a real taskbar and its weight at 16 px, the menu opening from the
  keyboard and where its focus goes, the icon following a mode or a scaling change
  within 2 s, a token round trip through the Credential Manager to a registration,
  Narrator, and containers after a sign-out. The Chromium renders of the pages, in
  both themes at 880, 721, 720 and 480 px, ran outside the repository and are not a
  gate. The axpc results are recorded on the tracking issue, not here.

## Updates

W4 ([#78](https://github.com/axross/runners/issues/78)) owns this section. An
installed app updates itself from a signed feed: it asks its agent to drain, waits for
the jobs already running to finish, and installs the new release.
[Security](../conventions/security.md#updates-come-only-from-the-signed-feed) holds
the rules the update path is reviewed against, and
[CLI](./cli.md#the-update-drain) holds the agent's drain.

The maintainer's update-feed public key, minisign key ID `6F39E3131E21E1E4`, is
committed in `tauri.conf.json`. Until the release run publishes the feed, which W4's
last pull request adds, a check finds no feed and changes nothing; an app with no
committed key, or a development build at `0.0.0-dev`, never reads the feed.

**Checking.** The app reads
`https://github.com/axross/runners/releases/latest/download/latest.json` when it
starts and every hour after, through Tauri's updater plugin, which only the app mode
registers. A check gives up after 30 s; a failed one is logged and changes nothing
else. For a release newer than its own, the app checks the download address (`https`,
`github.com`, under `/axross/runners/releases/download/`), downloads the MSI within 10
minutes, and the plugin verifies its signature against the committed key and that the
signature names the feed's version. Only then does it hold the update, and it does not
check again while it holds one.

**Draining.** Holding a verified update, the app asks its agent for
`drain_for_update` once the agent is running or stopped, no drain is under way and no
action of the window waits for an answer. While the agent resumes, builds or stops,
or another client's drain is under way, it waits; a refused drain is asked again a
minute later. While the agent drains, the host summary, the menu's first line and the
icon's tooltip read `Updating to <version>: waiting for <N> runners to finish`, or
`Updating to <version>` when it waits for none, and the live region says so; Start
All and Build stay unavailable. Stop all stops every runner and the agent ends for the
update with the runners stopped by you; Quit installs nothing.

**Installing.** After the agent's `ending` for this app's drain, the app waits up to
60 s for the agent's process to exit, from the process ID the drain's answer or the
snapshot gave. It then writes the record below, saying whether `ending` said the
runners were running, and starts `msiexec.exe /i <msi> /passive /promptrestart
AUTOLAUNCHAPP=True LAUNCHAPPARGS="<its own arguments>"`; the plugin then ends the app.
The installer shows Windows Installer's progress window. Starting the new app, which
starts its agent, depends on the MSI honouring `AUTOLAUNCHAPP` and `LAUNCHAPPARGS`;
that is a criterion of W1's installer pull request, and until it merges there is no
app MSI on main. If the process has not exited in 60 s, or the installer does not
start, the app drops the update and the record, handles the agent's end as any other,
and checks again an hour later. An end for an update another client asked for is
handled as any other end.

**A lost connection during the drain.** If the agent's process still runs, the app
reaches it again without starting another. If the agent's first snapshot still shows
this app's drain, for the same version and process, the drain was past its last steps
and goes on, and its `ending` installs as above; otherwise the app asks again. If no
agent answers while the process still runs, the app waits for it to exit as after
`ending`. If it has
exited, the agent ended unexpectedly: the app installs as above, and the record says
the runners stay stopped if the last state it heard keeps them stopped, as the
restart after a drop reads it. When nothing installs, the agent is started again
stopped or running as `ending`, or that last state, said.

**The record.** `update-stopped-by-you.json` in `%LOCALAPPDATA%\marshal\` names the
update's version, when it was written, and `running`, whether the runners were
running; a record without `running` is one written before it was added, and keeps
them stopped. A launch of that version within 30 minutes of it starts the agent
running when `running` is true and stopped otherwise, without consulting
`start_runners_on_launch`, so an update never undoes a stop by you or by a signal;
every launch removes it, so it applies once. Without a record that applies, a launch
is fresh.

**An older agent.** An app that finds a running agent older than the agent it starts,
for example after an installer was run by hand, asks it to drain for that version and
then starts its own agent, stopped unless `ending` said the runners were running.
The comparison is with the version of the `marshal-cli` library the app links, which
the agent gives in `welcome`, not with the app's own package version, so a release
that stamps the two differently does not replace the app's own agent again and again.
An older agent that does not list `drain_for_update` is used as it is, logged once,
and nothing installs while it runs.

**After a failed install.** Windows Installer rolls back to the installed version,
but Marshal stays off until it is launched from the Start menu, which starts the
runners and checks again. Tauri's updater leaves each downloaded MSI in a
`Marshal-<version>-updater-*` folder under the temporary folder; Windows' storage
cleanup removes it. The unsigned executable may lose its place in the notification
area after an update.

**The feed check.** The `marshal-feed` crate holds the address rule and the same
signature rules the plugin applies, and builds `marshal-feed-check`, which the release
run uses on the feed it writes before anything is published:

```text
marshal-feed-check <latest.json> <version> <SHA256SUMS> <platform>=<file>...
```

It checks the feed's version, that it has exactly the given platforms, each entry's
address under the release's tag, each file's SHA-256 against `SHA256SUMS`, and each
signature against the key committed in `tauri.conf.json`, and lists every problem. Its
tests use throwaway keys in `app/crates/feed/tests/fixtures/`.

The key's generation, its secrets, rotation and recovery come with the feed's pull
request.

## GitHub Status in the Window

While the main window is shown and not minimized, the session opens a second
connection to the agent and holds the `github` subscription on it, renewing it every
30 s and subscribing again when the agent answers `not_subscribed`. Hiding or
minimizing the window closes that connection, which ends the subscription, so the
agent asks GitHub only while someone can see the answer; [CLI](./cli.md#github-status)
owns the topic and what the agent does with it. The app tells the session when the
window is shown, hidden, minimized or restored, from the window's events and from
every place it shows or hides it.

Each runner with a registration gets a GitHub column beside its local state, in words
that begin "GitHub:": checking…, offline, idle, running a job, not listed, or not shown
when the agent's value left the runner out for size. A runner reads as checking until
its repository was checked after both the window opened and the runner entered its
current state, so a list read before then never makes it not listed. While checks are
paused, or GitHub cannot answer for the repository, the last values read since the
runner entered its state stay, marked with their time, even when they were read before
the window opened. A busy runner
whose job is known shows the workflow and job names, cleaned again and cut to 120 code
points, and an Open run link. Open run is an element with the link role that takes Tab
in list order; its accessible name names the job and the runner. It passes the pool
and the runner's name to `open_job`; the session builds the address from the pool's
configured repository and the job's two numbers, and when the runner or its job is
gone nothing opens and the pool says "That job has ended." for a minute. If a focused
Open run goes away with its runner, focus moves to the pool's heading. The agent's
value does not say which runners it left out for size, so while it left any out,
every registered runner it does not list reads "not shown" rather than "not listed".
A word of the topic's open sets this reader does not know reads as checking, or says
nothing on the card.

A pool's card says when job names are hidden or could not be read, and when GitHub
cannot answer for its repository, with the last check's time and when Marshal tries
again; a token cause W1's card already gives is not said twice, and a repository with
no stored token shows nothing from GitHub. The footer's first line is the request
budget, "not known yet" before any answer, and says when checks are slowed; a pause is
a banner. The summary line counts running jobs; the menu, the icon and its tooltip do
not. An agent without the topic, or one with no room for the subscription, is said in
the footer, and the latter is asked again every 60 s.

The live region says the running-jobs count with the summary, at most once per 5 s,
and each one-time GitHub announcement once, queued in order and written at least 5 s
after the last write: checks slowing or back to every 15 s, hidden or unreadable job
names, a repository GitHub cannot answer for, GitHub status unavailable, and a job that
has ended. A pause is said at once by its banner.

`app/crates/view/tests/view/github.rs` covers the states from a recorded value in
`tests/fixtures/github/`, `app/crates/session/tests/session/github.rs` the
subscription's lifecycle and Open run's address, `tests/frontend/github.test.mjs` the
column, the link's Tab stop, name and focus move, and `tests/ipc.rs` that showing and
hiding the window opens and ends the subscription and that the opener plugin's own
commands are refused.

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
