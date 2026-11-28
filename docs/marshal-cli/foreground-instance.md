---
type: Architecture
title: "Foreground instance ownership"
description: "How do the lock, private files, control channel and state machine enforce one foreground instance?"
---

# Foreground instance ownership

## The Instance Layer

The library also holds what makes `start` one instance per machine: the
instance's state and the restarts of its runtime core, the single-instance lock,
the control channel that other terminals use, the checks for private files and
directories, the runtime core itself, and the log file. `start` and its clients
use it, and the tests also drive it through its seams, the runtime layer's fake
backend and clock, and a helper process.

| Module                                    | Holds                                                                                       |
| ----------------------------------------- | ------------------------------------------------------------------------------------------- |
| `instance::lifecycle`                     | The instance's state, the answer to each request, and the runtime core's restarts           |
| `instance::restart`                       | When a failed runtime core starts again, and when the instance gives up                     |
| `instance::source`                        | The instance's own configuration file, read again at a restart or a resume                  |
| `instance::lock`, `instance::roots`       | The single-instance lock, and where the lock and the channel live                           |
| `instance::channel`, `instance::protocol` | The control channel's server and client, and its text protocol                              |
| `instance::windows`                       | The current user's security identifier, the only-user descriptor, and the pipe's options    |
| `instance::steps`                         | The container steps of a stop, shared by the foreground instance and the agent              |
| `private`                                 | The Unix checks for a private directory and a private file, and the current user ID         |
| `instance::runtime_core`                  | The runtime core `start` runs: pool preparation and every slot's loop                       |
| `instance::log_file`                      | The log `start` writes through, its file, the file's cap, and the files kept                |
| `instance::fake`                          | A scripted runtime core and configuration source, used by tests; a debug or test build only |

### Instance State

An instance is running or stopped, with stopping and resuming between them, and it
starts running. `build` holds the lock as an instance of its own, in the state
`building`, or `stopping` once it is asked to stop; [Build](./build-images.md#build) describes it.
One task owns the state and answers every request, so a request
that arrives during a transition is answered at once: `status` reports the state,
`stop` is taken, and `restart`, `diagnostics`, and `resume` are refused, naming the state. A
running instance refuses a resume too.

- `stop` ends the instance from every state. From running it first runs the
  container steps; from stopping it lets the stop finish; from resuming it ends
  without waiting for the configuration; from stopped it touches no container.
- `restart` runs the container steps, then resumes. On a stopped instance it
  resumes directly.
- A resume names a profile and a path. A stopped instance refuses one whose profile
  or path is not the one it was started with, and reads nothing. The path is the
  canonical one `start` will send; the instance only compares it.
- Resuming clears the runtime core's failures, reads the instance's own
  configuration file again, and validates it. A valid file runs the core with the
  new plan; an invalid one leaves the instance stopped, with the errors as its
  reason. The file is fixed when the instance is built, and no request names
  another, so nothing a client sends makes the instance read a different file.
  `start` fixes it as the path it was given, made absolute but not resolved, so
  each read finds the checkout above it as the first validation did; the
  canonical path is kept only to compare with a resume's.
- A resume and a `restart` check the host prerequisites, on axmc, and then the
  image, after the file validates and before the instance answers. A prerequisite
  that refuses leaves the instance stopped with the refusal's message as its reason;
  the instance also checks them once before its first run, and a refusal there ends
  the instance, so `start` exits 1.
- On axmc, a sleep and its wake, or a change of the primary network on two checks
  in a row, stops every runner as `stop` does, runs the container steps and the
  release of every job host, and leaves the instance stopped with the signal's
  text as its reason: `the Mac slept, so every runner was stopped` or
  `the primary network changed (<old interface> to <new interface>), so every runner was stopped`.
  It names interfaces only. Only `start` or `restart` resumes it, and a resume
  begins the signals afresh;
  [Wiring the Signals into the Instance](./mac-signals.md#wiring-the-signals-into-the-instance) has
  the rest.
- On axmc the instance holds off idle sleep while it is running or resuming and
  while it is stopping, and releases the hold on entering stopped.
- An image that cannot run jobs, which on axmc includes a missing or mismatched
  golden VM, leaves the instance stopped on a resume or a `restart`, with a reason
  that names the image and says to run the command-line release's `stop`, then
  `build --config <path>`, then `start --config <path>`, because
  `build` is refused while the instance exists. When the engine does not answer the
  check, Docker or Tart, the instance answers as before and runs the core, which
  checks again once it answers and moves the instance to stopped with that reason.

The container steps stop every ordinary container the backend has started and not
seen end, in one `docker stop` call with a grace of 15 s, ended at 20 s, and then
force-remove them in one call ended 6 s later, so they take 26 s at the most.
Alongside, they finalise the diagnostic containers the backend retains, on their
own clock, by 24 s; [Diagnostics](./diagnostics.md#diagnostics) has the timeline. A forced removal,
which a second Ctrl+C or SIGINT asks for, abandons the stop or skips it and removes
at once. At a stop, the container steps and the core's wind-down share an exit
deadline of 28 s from the request; whatever still runs then is dropped, and when the
removal had not finished, how the instance ended records a failed removal. The
instance reads the containers before it tells the core to stop, because dropping a
worker forgets its container. How the instance ended records whether every removal
in its life succeeded, at a stop, a restart, or a failure of the core, so a failed
removal is still reported when a later stop has nothing to remove.

**Releasing job hosts.** After the container steps, the instance and the agent
release every job host the backend still counts as prepared, all at once, each
within the backend's release budget: a worker releases its own host when its run
ends, so what remains is the host of a worker that is still busy or that a panic
took. The container backend has no hosts and a release budget of zero, so the stop
sequence above is unchanged. A backend that reserves a release budget `R` shortens
the container stop to fit the same 28 s exit deadline: the stop call is ended at
28 - 6 - `R` seconds, and its grace is 5 s shorter, never longer than 15 s. With a
`R` of 10 s, the grace is 7 s and the stop call is ended at 12 s, then the removal
at 6 s later, then the release at 10 s later, so the three take 28 s at the most
and `stop` still ends within 30 s. That leaves no slack: a `tart` call that hangs
to its bound in a double fault leaves its VM to the VM shim or the next `start`, as
[The release budget split](./tart-backend.md#tart-backend) describes. The diagnostic column ends
early by the same reserve, as [Diagnostics](./diagnostics.md#diagnostics) describes, so a
diagnostics pool's release keeps its whole budget too. The release itself splits
its budget, a graceful stop of the host first and a forced stop and deletion in its
last seconds; the backend chooses that split.

The runtime core is the part that runs the pools. It fails when it ends, with an
error, a panic, or neither, while no stop was asked for. The instance polls it
inside `catch_unwind`, so a panic becomes a failure carrying the panic's message.
Unwinding drops the core's workers, so the container backend keeps the name of
each container whose run a panic unwound through, and forgets it when a removal
succeeds; the instance stops and removes those containers before the restart. When
no pool can start at the instance's first run of the core, the instance ends and
`start` exits 1; at a later run, after a restart or a resume, that is a failure
like any other. A slot's registration or
container failure stays in that slot's backoff and is not a core failure. After a
failure the instance runs the container steps for the containers the core reports,
then starts the core again after 10 s for the first failure in a sliding 10-minute
window, 30 s for the second, and 60 s for the third. A fourth failure in the window
stops the instance, with the restart limit as its reason. A core restart that
finds an image that cannot run jobs moves the instance straight to stopped, with
the reason a resume gives, and counts no failure. A failure exactly 10
minutes old still counts and an older one does not. A clock that moves back
forgets nothing.

### Single-Instance Lock

The lock is machine-wide, across every OS user and configuration file. The
operating system releases it when the process ends, however it ends, and no child
process inherits it.

- **Windows.** The lock is the first instance of the named pipe
  `\\.\pipe\marshal-instance`, created with an instance limit of one, remote
  clients rejected, a handle no child inherits, and the descriptor below that
  admits only the creating user. The process keeps it open and it carries no
  message. Creating the name again fails for every user, with access denied or pipe
  busy, and that is "held".
- **Unix.** The lock is an exclusive advisory lock (`flock`) on a regular file: on
  macOS `/Users/Shared/.marshal-instance.lock`, and on Linux only a debug build's
  test root, because Linux is not a supported host. The creator sets the file's
  mode to `0644` after creating it, whatever the umask, so another user can open
  it. The file is opened for writing and, when the user may not write it,
  read-only; the lock then refuses that user rather than the open. Both opens pass
  `O_NOFOLLOW` and `O_NONBLOCK`, and what they open must be a regular file. So a
  link at the path is refused without its target being opened, a FIFO is refused
  at once instead of waiting for a reader, and anything that is not a regular file
  is "cannot be opened", never "held". After locking, the path must still name
  the locked file, not a replacement, or the lock is "lost".
- **Unix residual case.** That last check runs once, when the lock is taken. The
  lock file's owner, the user who created it first, can remove it while an
  instance holds it and create a new one, and a second instance can then lock the
  new file while the first still holds the old one. So can anyone else who may
  remove the file from its directory. The owner can also hold the lock and keep
  every other user's `start` from running.

The three refusals read differently: the lock is held by another process; the lock
cannot be opened, with the reason; or the lock file was replaced or removed after
it was locked.

### Control Channel

The channel is per user, and only the instance's own user can reach it. The
holder of the lock opens it, so a leftover Unix socket proves that no instance owns
it.

- **Windows.** The named pipe `\\.\pipe\marshal-control-<user SID>` is created as
  the first instance, with the descriptor `D:P(A;;GA;;;<user SID>)`, which grants
  all access to that user, to no one else, and inherits nothing, and with remote
  clients rejected. The pipe crate's blocking calls run on threads of their own: one
  accepts, and each connection is served on its own thread, which hands the
  request to the instance and waits for the answer. Up to 16 connections are served
  at once; a further one is closed unanswered. A connection that sends nothing
  holds its thread until it disconnects, because the blocking calls have no
  timeout. Closing the channel wakes the accepting thread with a connection of its
  own, trying twice, and waits for that thread only when a wake connects; otherwise
  the thread stays blocked until the next client, which it closes unanswered, or
  until the process ends. Serving threads are not joined, and a connection that
  sends nothing keeps its thread's handle on the instance's request queue, so the
  instance cannot rely on that queue closing to learn that the channel has closed.
  Another user who creates this user's pipe name first stops the channel
  from opening, and a client of this user would then reach that user's pipe; that
  is the accepted cost of squatting a name on a single-user host.
- **Unix.** The socket `control.sock` is in a directory that must be a directory,
  not a link, owned by the current user, with mode `0700`: on macOS
  `~/Library/Application Support/marshal/`, and on Linux only a debug build's test
  root. The server creates the directory with that mode when it is missing and
  refuses one that is not private, without changing it. It removes a leftover
  socket of the current user and refuses anything else at the path. It sets the
  socket to `0600` and checks it. It accepts a peer only when the peer's user ID
  is its own, and closes any other unanswered. Up to 16 connections are served at
  once, and a further one waits its turn. A request must arrive within 5 s.
  A socket path longer than the operating system allows is a startup failure.
  The client checks the directory before it connects, so it does not talk to a
  socket another user placed.
- **Windows, the agent's client.** The library's client of protocol version 2 reads
  the process ID of the pipe's server right after it connects and before it sends
  anything, opens that process, and compares its user's SID with the current
  user's. A server of another account, a process that cannot be opened or read, or
  any failed call is a refusal, `ServerNotThisUser`: the client drops the
  connection without sending a byte, so a `token_set` never reaches a squatter of
  another account. The check names no command and no token. A squatter of the same
  user is not told apart, and a process ID reused between the connection and the
  check is a short window; both are accepted on a single-user host.
  The commands of `start`, which speak version 1, are unchanged and still reach
  whatever holds the pipe, as the cost above says.

On both systems an ending instance first drops its request queue, so a request
still waiting is answered that the instance is ending, then stops accepting and
waits up to 0.5 s for every connection it accepted to end, so an answer it has
given, such as `stop`'s, is written before the process exits. A connection still
sending its request does not hold the exit longer than that.

The agent opens this same channel with the same names, descriptor, modes and
peer checks, but serves protocol version 2 on its own runtime without a thread per
connection, so its connection limits are timers on every operating system; see
[The Agent](./resident-agent.md#the-agent). A version 1 request reaches an agent as a first line that
is not `hello` and is refused as `unsupported_protocol`.

The protocol is UTF-8 lines. A request is one line of at most 16 KiB: `status`,
`stop`, `restart`, `diagnostics`, or `resume <profile> <path>`, where the path is
the rest of the line and may hold spaces. A path with a line break, or one that is not UTF-8, is
refused before the client connects. A response is a line `ok` or `refused` with an
optional message, then a line per field, a name, a space and its value, and then a
line `end`; a line break in a value becomes a space, so it cannot end the response
early. A response carries states, reasons, paths, and names, never a token or a JIT
configuration. Any other line is answered with `refused`, or closed unanswered
when it is too long, and never reaches the instance.

A client gives up connecting after 2 s and treats that as no instance, as it does a
missing socket or pipe, so the moment before a new instance opens its channel and
after an ending one closes it reads as no instance. It then waits for the answer as
long as its caller allows, and an answer that does not arrive in time is "did not
answer in time". The client reads at most 64 KiB.

### Roots and Private Files

The lock file, the socket directory, the log directory and the pipe names are fixed
for a host. Only a debug build can move them, to a test root, which suffixes the
Windows pipe names with a hash of the root; a release build ignores the request, as
it ignores a GitHub base address. `tests/instance_roots.rs` shows both halves and
each host's fixed roots, and CI runs it again in the release profile. `std` has no
call for the process's own user ID and `unsafe` is forbidden, so the CLI reads its
user ID as the owner of a file it creates in the temporary directory. The file's
name has a random part, and the CLI only ever creates it new: a name that already
exists is skipped, never opened, and after eight taken names reading the user ID
fails.

On macOS a token file must also be a regular file owned by the current user with
mode `0600`, checked on the file that is opened, or reading it fails and the pool is
skipped with a warning; at a registration the failure goes into the slot's backoff.
On Windows the token file's access control list is not checked, because no safe call
reads it.

### What the Instance Tests Cover

`instance_state.rs`, `core_restart.rs`, `instance_lock.rs`, `control_channel.rs`,
and `instance_roots.rs` drive the instance layer through its seams as one OS user,
and each test's name says what it shows. Two CI steps cover what one account cannot.
On the Linux and macOS hosted runners, `tests/ci/other-user-lock.sh` copies the
helper to a directory under `/tmp`, holds the lock in a debug-only root there, and
runs the copy as `nobody` through `sudo`; it fails unless `nobody` is refused with
the "held" exit code. On the Windows hosted runner,
`tests/ci/control-pipe-security.ps1` has the helper open the control pipe, once as
the instance's channel (`serve`) and once as the agent's (`serve-agent`), connects
to it, reads the live pipe's descriptor through .NET, and fails unless the access
control list is protected and holds one entry, allowing the current user full
access. Neither crate the CLI uses reads a live pipe's descriptor without `unsafe`,
so that check is in PowerShell. A hosted runner cannot show the Windows lock
against a second OS user or the rejection of remote clients, and both are not yet
verified on axpc
([#56](https://github.com/axross/runners/issues/56#issuecomment-6511420845)):
neither check was run there, by the maintainer's decision.
Not verified: macOS and Windows behaviour beyond what the hosted runners show.
