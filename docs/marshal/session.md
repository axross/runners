---
type: Architecture
title: "Marshal executable and agent session"
description: "Which process owns the agent, how does the app connect, and where can I find logs?"
---

# Marshal executable and agent session

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
`CREATE_NO_WINDOW`; [CLI](../marshal-cli/foreground-commands.md#running-a-host) says where that is set and tested,
which children inherit the CLI's standard streams, and what only the axpc run shows,
including whether a window still appears. Only the app mode takes Tauri's
single-instance lock, so a second launch brings the first one's window forward and
starts nothing. The agent mode is kept to one by the machine-wide lock the library's
agent takes, as the library's `start` is, so the app and the last command-line
release never run the pools together.

A `fake-docker` binary is built beside `marshal-app` from the CLI's
[`cli/helpers/fake_docker.rs`](../../cli/helpers/fake_docker.rs), so the tests that
start this executable's exporter child can run it against a fake engine. The package
sets `default-run = "marshal-app"`, so `cargo run` still starts the app. The helper,
and the process tests that use it, are built only with the `fake-engine` feature,
which no shipped build turns on, so run the workspace's tests with
`cargo test --locked --workspace --features marshal-app/fake-engine`; without the
feature, Cargo skips those tests without saying so. Only the app's own executable is
uploaded or released.

### The Debug-Only Test Variables

A debug build reads three variables in the agent mode, and the app mode reads the
first, so a test can point the executable at a scratch host:

| Variable                  | Moves                                                                |
| ------------------------- | -------------------------------------------------------------------- |
| `MARSHAL_TEST_ROOT`       | The agent's lock, channel, log and configuration locations           |
| `MARSHAL_TEST_DOCKER`     | The `docker` program the agent runs, so a test can use `fake-docker` |
| `MARSHAL_TEST_GITHUB_API` | The address the agent sends registration requests to                 |

They are the names the command line's `start` reads. On axmc the agent also reads the
CLI's other debug-only variables, such as `MARSHAL_TEST_TART` and the sleep and
network signal variables, as the [host signal source](../marshal-cli/mac-signals.md#signal-source) describes. On the Mac, a debug build's
app mode also reads `MARSHAL_TEST_ANY_LOCATION`, which lets a copy outside
Applications control an agent, so a test can run the app from its build folder.
A release build ignores all of
them: the `marshal-modes` crate reads its three only when `debug_assertions` is on,
the CLI library does the same for its own, and the location rule reads its variable
only in a debug build, so no environment moves a release build's lock, channel,
files, `docker`, `tart` or GitHub address, or lifts the Applications rule. The
`app-macos` job runs the location rule's tests in the release profile with the
variable set, so they fail if a release build reads it.
[Security](../security/app-trust.md#the-app-surface) records why. The crate has a
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
  `--sign-in` (see [CLI](../marshal-cli/agent-lifecycle.md#the-agents-signals)). Signing in with
  it on is unverified on axpc until W3's out-of-tree check. The installer's uninstall
  removes the value, and an absent value is not an error.
- **Login item on the Mac.** M4 ([#90](https://github.com/axross/runners/issues/90))
  keeps the app's own bundle registered as its login item with `SMAppService` while
  `start_at_login` is on in effect; the agent leaves the snapshot's sign-in item unset
  on macOS. The app reconciles at launch, when a snapshot's settings in effect change,
  and once after each Start all, whether or not that left the settings unchanged: it
  registers when the setting is on and macOS reports Marshal not registered, or not
  found, which is what macOS 27 reports for a Marshal that has never registered, and
  unregisters when it is off and Marshal is registered or waiting for approval. Only a
  Marshal in `/Applications` or `~/Applications` does either. It never registers over
  "requires approval", and it keeps the empty file `login-item-registered-by-marshal`
  in the configuration folder after registering, so a later "not registered" counts as
  the maintainer turning Marshal off in Login Items; turning the setting off clears
  it. Showing the window reads macOS's status again, so a removal or an approval made
  in Login Items while Marshal runs shows then; it registers and unregisters nothing,
  and it keeps a failed registration's code until macOS reports another state. A failed
  call is logged with its code and tried again at the next launch, Start all or change
  of a setting. This is unverified on axmc until M4's Mac run. Before removing Marshal,
  turn the setting off, or remove Marshal in Login Items.
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
  [CLI](../marshal-cli/agent-protocol.md#protocol-version-2).
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
  wait a bounded time for the agent's answer (40 s, and 10 s for a token or a
  setting). On a
  connection that is still open a call that overruns says the agent has not
  answered yet and leaves the list as it is, since the agent may still do it; it is
  never reported as lost contact, which is only a connection that ended. A token
  save or removal that overruns is neither saved nor refused: its row says the
  agent has not answered yet, its form or question closes, and the row's state
  then follows the snapshot.
- **The app ending** or being killed leaves the agent and its runners running, and
  the next launch reconnects to them.

A token travels from the window to the agent only in `token_set` or
`organization_token_set`. The session trims
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
