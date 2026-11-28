---
type: Architecture
title: "Resident agent and state transitions"
description: "How does the resident agent serialize requests, maintain configuration and handle failure?"
---

# Resident agent and state transitions

## The Agent

The library also holds the resident agent: the instance in a form that stays up and
is told what to do, instead of being started by a terminal and stopped by it. It is
the same process model as `start`: the agent takes the same single-instance lock,
opens the same control channel, writes a log in the same directory and, on Windows,
runs inside the same kind of Job Object, so `start` and the agent never run on one
machine at once, and a second agent is refused as a second `start` is. It has no
command or flag of its own: `agent::run` is the entry that wires the profile's
backend, GitHub, the token store, the lock, the channel and the log, and the app's
executable runs it in its hidden agent mode, as [App](../marshal/session.md#the-executable-and-its-modes)
describes. On axpc the backend is the container backend and Build builds the
embedded definition with Docker. On axmc it is the Tart backend, which runs the Linux
and macOS pools in VMs as `start` does, with every job VM under the shim. The Linux
runners use `actions-runner:local` when the configuration names no image. Build makes
the golden VMs of the configured pools as `build --profile axmc` does, through the
same function: [The Golden VM Build](./build-images.md#the-golden-vm-build) for a Linux pool, then the
golden macOS VM for a macOS pool. The
CLI's tests start it through `instance-helper agent`. Only a debug
build lets a test replace the agent's token store, channel limits, watched file, log
size and `docker` environment; a release build ignores each
request, as it does the roots and the GitHub address, and `tests/agent_overrides.rs`
shows both halves and runs again in the release profile in CI.

| Module              | Holds                                                                               |
| ------------------- | ----------------------------------------------------------------------------------- |
| `agent::machine`    | The agent's state, and what each request, failure and changed executable does to it |
| `agent::core`       | The pools' slots, with each token read from the store and late pools started        |
| `agent::state`      | Where the agent is, why it is stopped, and what it knows of its configuration       |
| `agent::tokens`     | The pools' token source, read from the store, and the changes that start late pools |
| `agent::view`       | The snapshot, built from the state, the board, the retained record and the tokens   |
| `agent::api`        | The methods a connection can call and the errors they answer with                   |
| `agent::wire`       | The messages of protocol version 2 and the version and size constants               |
| `agent::server`     | One connection: handshake, requests, subscription, heartbeat, `ending` and timers   |
| `agent::channel`    | The pipe or socket the agent listens on, and the public client                      |
| `agent::snapshot`   | The value of the `state` topic and its fit to the message limit                     |
| `agent::log`        | The log file that is continued in a new file at 10 MiB                              |
| `agent::executable` | The check that the file the agent started from is still the same file               |
| `agent::builder`    | The seam the agent builds the runner image through: Docker's, and the golden VM's   |
| `agent::run`        | `run_resident` on its seams, and `run` with the profile's backend, GitHub and store |
| `token_store`       | The token store interface, the in-memory store, the Credential Manager and Keychain |

### Agent State

The agent is `running`, `resuming`, `stopping`, `stopped`, `building`, or
`quitting`, and one task owns the state, so a request that arrives during a
transition is answered at once, never queued behind it. It starts `running` or
`stopped` as the caller asks.

| Method             | Answer                                                                                                                                                                                      |
| ------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `status`           | The snapshot, in any state                                                                                                                                                                  |
| `start_all`        | Stopped: reads the configuration again and checks the image, then answers `resuming`, or `stopped` with the reason it could not. Running: the state it is in                                |
| `stop_all`         | Running: runs the stop sequence (the same grace, budget and exit deadline of 28 s as `stop`) and answers `stopped` when it is done. Stopped: `stopped`, by you when it cancels a resume     |
| `build`            | Stopped: answers `building` with the start time once the build began, and the snapshot tells the rest. Running: `refused`. A configuration that cannot be used: `refused`                   |
| `quit`             | Runs the stop sequence if runners run, cancels a build, answers `quitting`, and ends the agent; the lock is free within a second of the answer                                              |
| `token_set`        | Stores the repository's token in any state                                                                                                                                                  |
| `token_remove`     | Removes it in any state                                                                                                                                                                     |
| `drain_for_update` | See [The Update Drain](./agent-lifecycle.md#the-update-drain)                                                                                                                               |
| `config_read`      | The file's editable values in any state; see [Editing the Configuration](./config-protocol.md#editing-the-configuration)                                                                    |
| `config_edit`      | Running: writes and applies. Stopped: writes, for the next `start_all`. A transition: `busy`. A drain: `refused`                                                                            |
| `config_reload`    | Running: applies the file. Stopped: makes it the configuration in use. A transition: `busy`. A drain: `refused`                                                                             |
| `launch`           | Stopped: as `start_all` when `start_runners_on_launch` is on in effect, or while none is; otherwise stays stopped by you with the text `not started at launch`. Running: the state it is in |
| `lifecycle_set`    | Writes one `[lifecycle]` key through the write path and puts it in effect at once, in any state; see [The Agent's Signals](./agent-lifecycle.md#the-agents-signals)                         |

A request that arrives while the agent is `resuming`, `stopping`, `building` or
`quitting` and is not one of `status`, `config_read`, `token_set`, `token_remove`,
`lifecycle_set`, a stop or a quit, is answered `busy`, except `drain_for_update`,
which is `refused` there. The
agent is `resuming` from the moment it begins to read
the configuration until the runners' pools are admitted, so a `start_all` or a
`build` in that time is `busy`, not answered as if the runners ran. One table in the
code decides all of it, for every phase; this is what it says, and while a drain runs
the table in [The Update Drain](./agent-lifecycle.md#the-update-drain) takes its place:

| State      | `stop_all`                           | `start_all` | `build`                                          | `quit`                     |
| ---------- | ------------------------------------ | ----------- | ------------------------------------------------ | -------------------------- |
| `running`  | Stops, then `stopped` (by you)       | No change   | `refused`                                        | Stops, then the agent ends |
| `stopped`  | No change                            | `resuming`  | `building`, or `refused` without a configuration | The agent ends             |
| `resuming` | Abandons it, then `stopped` (by you) | `busy`      | `busy`                                           | Abandons it, then ends     |
| `building` | Cancels it, then `stopped` (by you)  | `busy`      | `busy`                                           | Cancels it, then ends      |
| `stopping` | No change                            | `busy`      | `busy`                                           | Ends after the stop        |

The agent stays up when something is wrong; it stops its runners and says why in the
snapshot, as the reason of the `stopped` state:

| Reason code             | When                                                                                           |
| ----------------------- | ---------------------------------------------------------------------------------------------- |
| `by_you`                | A `stop_all`, a `quit`, or an agent launched stopped                                           |
| `no_configuration`      | There is no `config.toml` and no configuration is in use                                       |
| `invalid_configuration` | The file, or a `config.json` it migrates, is not valid; problems in snapshot                   |
| `image_unusable`        | The image is missing, or its contract version is not this CLI's                                |
| `no_pool_started`       | No pool could start, such as every pool without a token                                        |
| `restart_limit`         | The runtime core failed too often, under the same restart policy `start` has                   |
| `image_build_failed`    | A `build` failed; the build's message is in the snapshot                                       |
| `slept`                 | The computer slept or woke; see [The Agent's Signals](./agent-lifecycle.md#the-agents-signals) |
| `network_changed`       | The primary network changed on two checks in a row                                             |
| `resumed_too_often`     | A signal stop after too many automatic resumes within an hour                                  |

The answer to a `start_all` follows the configuration read and the image check; it
is not sent first. The check is one `docker image inspect` and, when that fails, one
`docker image ls`, which share a bound of 4 s (the image check budget), so the answer
comes within about that time and the pools are admitted after it. A client that
needs a sooner answer bounds its own call and watches the `state` topic for the
rest.

After a `build` that succeeds the agent checks the image once more, as part of the
build's own outcome: the check refreshes the snapshot's `images` entry with the new
image's labels and warnings, and it does not change the `outcome`, which is the
build's. The check runs while the agent is still `building` and answering: `status`
shows the build, `start_all` and `build` are `busy`, and a `stop_all` or a `quit`
taken during the build or during the check skips or abandons the check, which leaves
the `images` entry as it was; a build that still commits after the stop stays `ready`.
A `build` that succeeds and is not cancelled leaves the agent stopped by you
(`by_you`), whatever it was stopped for before; the build reads no file, so the
snapshot's problems and pools are those of the latest `start_all` or start. Its outcome, `ready`, is only in `agent.build`,
and the refreshed image entry is in `images`: "stopped with the image ready", in
the state table above, is that outcome and not a reason. A build that fails
stops the agent for `image_build_failed`, and a cancelled one is stopped by you.
The app's rule to restart the agent when the last state it heard was stopped by you
is unchanged. A `start_all` answers
`resuming`, which stays true until the pools are admitted: the snapshot's `state`
becomes `running` once they are, or `stopped` with one of the reasons above when
they could not be.

The set is open: a reader keeps a code it does not know as text. The library exports
the codes as the constants of `agent::StopReason`, so a reader compares a reason with
them rather than repeating the words. A pool skipped for
want of a token does not stop the others: it is listed `skipped` with its reason,
and when a token for its repository is set, it starts without a restart. When no
pool is left running, the agent is stopped for `no_pool_started`, and each pool
keeps its reason in the snapshot, listed `stopped`.

The agent ends by itself when the file it started from is moved, renamed, replaced
or deleted, so it never goes on running a program that is no longer the installed
one. It tells the file by its size, its times and its identity on the volume (device
and inode on Unix; volume serial number and file index on Windows, read through a
handle opened for no access with every share mode, so neither the running program nor
an installer is blocked). It looks once a second and ends within 5 s, after the same stop sequence as
`quit`; the `ending` message names the reason `executable_changed`, where a `quit`
names `quit`. The agent does not handle a sign-out or a shutdown: it gets no event
for them, as `start` gets none on Windows, and the next agent removes what they left.

#### The Agent's Configuration

The configuration is `config.toml` in the `marshal` folder of `%LOCALAPPDATA%` on
Windows and in `~/Library/Application Support/marshal/` on macOS, a folder that must
be a directory owned by the user with mode `0700`, the folder the socket is in. A
debug build can move it with a test root. The path is fixed. The agent checks it under
the profile of the host it runs on, axpc on Windows and axmc on macOS; only a debug
build can name another, which the tests use. It reads no token file: each pool's
token comes from the token store, by its repository. `cli/src/host_config/` does the
loading, and the agent keeps what the latest load found.

- **At the start** the agent removes the temporary files an interrupted write left,
  then reads `config.toml`. When it is missing and `config.json` exists, it migrates
  it: `config.json` is checked under today's JSON rules, converted as
  [Converting a JSON Configuration](./toml-configuration.md#converting-a-json-configuration) describes, and
  written as `config.toml` only where no file of that name exists, then read. The log
  says so. `config.json` is left in place and is not read again while `config.toml`
  exists; deleting `config.toml` makes the next start migrate it again.
- **A `config.json` that breaks the rules** is not migrated. The agent stays stopped
  for `invalid_configuration`, with the JSON problems in today's wording.
- **A converted document that fails its check, or a `config.toml` that cannot be
  written,** leaves the agent on `config.json`'s configuration for this run, and the
  snapshot's migration says why. The next start tries again.
- **At each `start_all`** from a stopped state the agent reads `config.toml` again. A
  valid file is used. A file with problems, or none, leaves the configuration the agent
  holds in use, as `last_valid`, and the snapshot lists the file's problems and when
  the configuration in use was read. With none held, the agent stays stopped. A
  `start_all` migrates nothing: a missing `config.toml` beside `config.json` is
  `pending`, and the reason says to quit Marshal and launch it again.
- **`build`** uses the image name of the configuration in use, on axmc
  `actions-runner:local` when it names none, as the library's `build` does, and reads no
  file; with none in use it is `refused`. Start All checks the same image.
- The last valid configuration lives in memory only. A new agent process starts from
  the file. The agent does not watch the file.

The agent's log is a file in the same private directory as `start`'s. It is
continued in a new file when it reaches 10 MiB, so no line is lost to the limit, and
the newest ten files are kept, and a warning in the file being written says when an
old one could not be removed. Every token and JIT configuration is masked in it, and
nothing goes to a terminal, since the agent has none: a failure before the log opens
goes to standard error, and everything after goes to the log. A `docker build` the
agent runs has its output read line by line into the log, each line masked and
marked `docker build:`; a longer line is cut at 8 KiB, or before a masked value the cut would split,
marked with `...`, and the rest of it dropped, so it is logged once and no part of a
masked value is kept, and the last five
lines, masked and each cut at 70 characters, end the message of a failed build, so the 500-character cut
of the message never takes the newest line. The library's `build` passes the build's
output on to its standard error instead.

A log that cannot write or continue its file stops, and the agent goes on without
it. It tries to open a new file again when a line arrives, at most every 30 s, and
writes one line in the new file that says how many lines were not written.

### What the Agent Tests Cover

The agent tests run the agent and the test script on one runtime, behind the real
channel, on the fake backend, clock, registrar and image builder and an in-memory
token store, and talk to it with the library's client or by hand, line by line.
`tests/agent_resident.rs` covers stop, start and quit (also while a subscribed
connection reads nothing), launching stopped, the executable renamed, the resuming
state, build (ready, failed, cancelled by a stop or a quit, `busy`, `refused`, and the
image check after it, which still answers and is skipped or abandoned by a stop or a quit), and
staying up for each reason, also through `start_all`, which answers `resuming`, that
`token_set` and `token_remove` are answered while it resumes, builds and stops, and the
core's restarts: the `restart_due` window with its empty pool list, and the stop for
`restart_limit`. It also covers the configuration: the migration at the first start,
a `config.json` that is not migrated, the last valid configuration at `start_all`
with the file's problems, a pending migration, and a `build` that uses the
configuration in use. The restart waits are the fake clock's, with a real pause of 400 ms
each so the script can look at the window.
`tests/agent_config.rs` covers the configuration methods: `config_read` with a
wrong-typed value and an undefined key, raising `runners` (the new slot starts, the
comments stay, nothing stops), lowering it and removing a pool (only their slots'
containers are stopped), the refusals that write nothing (`config_invalid`,
`config_refused`, `image_unusable`, `config_changed`), an edit on a stopped agent, a
reload of a bad file and then of a good one, a restarted pool, edits of a pool that
waits for its token, and a removed pool with diagnostics on, whose evidence is
exported first, and a stop that runs past its deadline. `tests/agent_drain.rs` shows the two editing methods refused during a
drain.
`tests/agent_protocol.rs` covers the handshake, `welcome`, unknown methods, topics
and fields, subscription (also by a client that arrives while the agent is idle),
coalescing, heartbeats, `ending`, the limits as timers,
a connection that stops reading, the `github` topic and `renew` (the full value on
`subscribe`, the end after 90 s without renewal, `not_subscribed` after it, and
collection only while a subscription is held). `tests/github_status.rs` runs GitHub
status on a paused clock against a fake GitHub: the intervals and levels, the pause,
`retry-after` and the secondary limit, the back-off, pages by `link` with their
`ETag`s and refused next pages, the job lookup and its 20 checks, the hidden-names
retry, and that no token reaches a value or the log; the module's unit tests cover
each answer, the budget window, the next-page rule and the cleaning of names. `tests/agent_tokens.rs` covers what `token_set` and
`token_remove` do to the next registration, a skipped pool starting, refused tokens,
and the store's kind and code in an error, a skipped pool's reason and the log.
`tests/agent_snapshot.rs` covers the snapshot, the diagnostics, the leftovers, the
64 KiB cut, that the agent ends and frees the lock within a second of `quit` with 640
runners and four subscribed connections that never read, the stalled readers' thread
count (measured on Linux only, from the runtime's worker threads) and, in
`tests/agent_quit_stalled.rs`, that the process ends within a second or so of a
`quit` with a stalled subscriber, and that no token or JIT
configuration reaches a connection or the log. `tests/agent_versions.rs` runs `stop`,
`status`, `restart` and `diagnostics` against an agent and shows they exit 1 and
change nothing. `tests/agent_channel.rs` covers the socket's mode, the peer's user
and a directory open to others on Unix. `tests/agent_channel_windows.rs` shows a pipe
of a taken name is refused and a client passes the check of the server's user. `tests/agent_process.rs` runs the agent as a process on
the fake `docker` and a GitHub stub, a second agent refused, and the log continued
with ten files kept. `tests/agent_drain.rs` covers the update drain: idle, busy and
unknown runners, the bound, the reachable rows of its state table, a cancelled drain,
a backing-off slot, a pool waiting for a token, a core failure, and diagnostic
finalisation.

The Windows CI step `tests/ci/control-pipe-security.ps1` runs its checks against
the `serve` mode of the helper and against `serve-agent`, which opens the agent's
channel, so the agent's pipe is read back with the same descriptor. The real
Credential Manager test and the connection limits on Windows and macOS run in CI only.
Not verified here: the agent on Windows and macOS beyond what the hosted runners show,
and the remote-client rejection, as for `start`.
