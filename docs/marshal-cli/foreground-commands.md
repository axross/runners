---
type: Specification
title: "Foreground host entry points"
description: "What do the test-only foreground commands do on startup, stop, signal or forced close?"
---

# Foreground host entry points

## Running a Host

This section describes the library's foreground instance, its `build` and its version
1 clients. Since W1's installer pull request no released executable built from main's source runs them: the
app's agent runs a host instead, as [the resident agent](./resident-agent.md#the-agent) describes, with the
runtime layer, the image check, the log and containment that this section shares
with it. The tests run them through `commands-helper`, and the last command-line
release's tag documents them as commands.

`start` runs the host's pools in the foreground through the instance layer and the
runtime layer, and `stop`, `restart`, `status`, and `diagnostics` talk to it from another terminal
over the control channel. `build` builds the runner image, and the same clients
talk to it. On axmc `build` makes the golden VM, which M2's Mac app is to build.

| Module                                   | Holds                                                                                                                                                                                                                                                                                                                 |
| ---------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `start`                                  | `start`: the lock, validation, the log file, containment, the channel, the instance, and its signals                                                                                                                                                                                                                  |
| `build`, `build::golden`                 | `build`: the lock, validation, the channel, its states, and its `docker` steps and their aborts; the golden VM build                                                                                                                                                                                                  |
| `build::golden_macos`, `build::build_vm` | The golden macOS VM build, and what a build of either OS shares: the Tart and Softnet check, the deletion of build VMs an earlier build left, the build VM's boot, guest calls under the stop signal, shutdown, commit, and deletion, and the stop and failure handling of retiring the old golden VMs after a commit |
| `control`                                | The clients `stop`, `restart`, `status`, and `diagnostics`                                                                                                                                                                                                                                                            |
| `shim`                                   | The Unix shim between `start` and one job container's `docker run` client                                                                                                                                                                                                                                             |
| `diagnostics`                            | The exporter child, listed under The Runtime Layer                                                                                                                                                                                                                                                                    |
| `instance::runtime_core`                 | The runtime core, listed under The Instance Layer                                                                                                                                                                                                                                                                     |
| `instance::log_file`                     | The log, listed under The Instance Layer                                                                                                                                                                                                                                                                              |

### Start

A fresh `start` does these in order:

1. It takes the instance lock. When the lock is held it asks this user's instance
   to resume instead, as below.
2. It validates the configuration as `validate` does. An invalid one prints every
   error and exits 1, and nothing is written.
3. It opens its log file and logs the plan summary.
4. It contains its children, builds the HTTPS client, and opens the control
   channel. A failure of any of them, including a TLS setup failure, exits 1.
5. It runs the instance on the backend of the profile: Docker on axpc, Tart on
   axmc. On axmc, a host prerequisite that refuses ends the instance first, as
   [Host Prerequisites](./tart-backend.md#host-prerequisites) says. The runtime core waits for the
   engine every 5 s (Docker, or Tart), checks the image as
   [The Image Contract](./runner-images.md#the-image-contract) describes, prepares the pools
   as [Names and the Slot Loop](./pool-runtime.md#names-and-the-slot-loop) describes, and then runs
   every slot's loop until a stop. When the image cannot run jobs, the instance
   ends, and `start` exits 1, saying to run the command-line release's
   `build --config <path>`, then `start --config <path>`. When no pool can start, `start` exits 1. On axmc
   the instance also holds off idle sleep and watches the Mac's sleep and primary
   network, as [Signal Source](./mac-signals.md#signal-source) describes, and a slot clones no VM
   until a primary network is recorded.

On Windows, `start --profile axmc` exits 1 before step 1 with
`error: the axmc profile runs only on macOS`, because Tart exists only on macOS.
On axmc the containment of a job VM is the [VM shim](./tart-backend.md#the-vm-shim): `start` gives
the Tart backend the shim as its launcher on Unix, as it gives the Docker backend
the container shim.

On Linux a release build exits 1 at the first step, because Linux is not a
supported host, and `tests/instance_roots.rs` shows it in the release profile. A
debug build runs on a test root, for the tests. On macOS, `start --profile axpc`
runs the container backend with Unix containment, which serves the tests, not an
operated host. `MARSHAL_TEST_ROOT` moves the lock, the channel, and the log
directory, `MARSHAL_TEST_GITHUB_API` moves the GitHub base address,
`MARSHAL_TEST_DOCKER` names the `docker` client that `start` and `build` run, the
Unix shim's removal in `start` included, `MARSHAL_TEST_TART` names the `tart`
client that `start` runs on axmc, `MARSHAL_TEST_BUILD_DEFINITION` names
a test definition that `build` builds instead of the embedded one, and
`MARSHAL_TEST_RUNNER` declares another runner version and its osx-arm64 SHA-256
in place of the runner record's, as `<version>=<sha256>`; [The Runner
Record](./macos-pools.md#the-runner-record) says how it is read. On axmc the
tests also set `PATH` to find the fake `softnet`, as Tart does, and
`MARSHAL_TEST_SOFTNET_OWNER_MODE` to judge its file, and `MARSHAL_TEST_SIGNALS` to
script the sleep, the primary network and the keep-awake hold. Only a debug
build honours any of them: in the release profile `tests/base_address.rs` shows
that the client stays `docker` and that the declared runner stays the record's,
`tests/build_definition.rs` that `build` keeps
the embedded definition, and `tests/release_overrides.rs` that the client stays
`tart`, the `route`, `arp` and `caffeinate` programs stay the system's, the
signals are not read from a directory, and Softnet's file is judged as it is.
The tests name the fake client rather than put it first on `PATH`, because Windows
searches the program's directory and the system directories before `PATH`, so a
real `docker.exe` in `System32` would win.

A `start` that finds the lock held changes nothing. It canonicalises its
configuration path, sends `resume <profile> <path>`, and waits up to 10 s:

- A stopped instance started with the same path and profile reads its own file
  again. A valid file runs it and `start` exits 0; an invalid one leaves it stopped
  with the new reason and `start` exits 1.
- A running, stopping, resuming, or building instance refuses, naming its state,
  and so does a stopped one started with another path or profile. `start` exits 1.
- A stopped instance whose image cannot run jobs stays stopped with that reason,
  and `start` exits 1.
- A stopped axmc instance whose host prerequisite refuses stays stopped with the
  refusal as its reason, and `start` exits 1.
- A stopped axmc instance that a sleep or a network change stopped resumes like any
  other: it reads its file, checks the prerequisites and the image, records the
  primary network afresh, and `start` exits 0.
- When nothing answers on this user's channel, `start` exits 1, saying that the
  lock is held by another OS user or by an instance of this user that is still
  starting or ending. The lock does not say which, and `start` never connects to
  another user's channel.

### The Clients

Each client gives up connecting after 2 s. With no instance it says so and exits
1, and an answer that does not arrive in time exits 1, saying the instance did not
answer in time.

- `status` waits up to 5 s and prints one `name: value` line per field: `state`;
  `reason`, when stopped; `config`; `profile`; one `pool` line per pool, which is
  `waiting for Docker` (`waiting for Tart` on axmc), `preparing`, `running`, or
  `skipped: <reason>`; one `slot`
  line per slot, which is `registering` (the preparation of its job host and the CPU
  probe included), `backing off`, `held: <reason>`, `running container <name>`, or,
  for a macOS slot, `running VM <name>`, `waiting for the macOS VM limit` or
  `waiting for VM <name> to be deleted`; on axmc, one `leftover-vm` line per
  counted job VM of an unconfigured pool; the image lines under
  [The Image Contract](./runner-images.md#the-image-contract), and with a macOS pool the golden macOS
  VM's lines under [macOS Status](./macos-pools.md#macos-status); then, on axmc, a
  `network: waiting for a network (no IPv4 default route)` line while no primary
  network is recorded, a `warning: keep-awake is not held (...)` line while the hold
  is wanted and not held, and one `warning: <message>` line per
  warning of the latest passing host prerequisite check. Pools and slots
  are listed while the runtime core runs; the image and warning lines are listed on
  every call, as of the latest check. A `build` answers `state`, `image`, and `elapsed: <s> s`.
- `restart` waits up to 35 s for the instance to run its container steps (28 s at the
  most), read its file again, check the host prerequisites (axmc only) and the image
  (6 s at the most on axmc, 4 s on axpc), and answer. An invalid file or an image that cannot
  run jobs leaves the instance stopped, and `restart` exits 1. It is refused while
  the instance is stopping, resuming, or building.
- `stop` asks the instance to stop, then checks about every 200 ms whether the
  instance lock is free, by taking it and letting it go at once, until 35 s after
  its request. It exits 0 once the lock is free, so a `start` right after it finds
  the lock free, and 1 when the instance still holds it.
- `diagnostics` waits up to 30 s for the instance to finalise the diagnostic
  containers it retains, and prints the answer; [Diagnostics](./diagnostics.md#diagnostics)
  describes its lines and exit codes. It is refused while the instance is
  stopping, resuming, or building, and while another `diagnostics` is under way.

### Stopping

`stop`, SIGINT, SIGTERM, and SIGHUP on Unix, and Ctrl+C and Ctrl+Break on Windows
end the instance from any state with the stop sequence, which takes 30 s at the
most from the request. SIGHUP is how macOS reports a closed terminal. A signal
`start` cannot listen for is a warning at startup, and that signal keeps its
default action; the others still stop the instance.

1. The slots stop registering at once.
2. The container steps under [Instance State](./foreground-instance.md#instance-state) run: one
   `docker stop --time 15`, ended at 20 s, and then one `docker rm -f`, ended 6 s
   later; alongside, each diagnostic container is exported and removed by 24 s, a
   VM's by deleting the VM.
3. At 28 s whatever still runs is dropped. A `docker` client is killed; on Unix a
   job container's shim sees its liveness pipe close, removes its container, or
   ends a diagnostic one and keeps it, within 5 s, and ends. A VM backend reserves
   10 s of the 28 s to release its job VMs, which ends the container steps earlier,
   as the [Tart Backend](./tart-backend.md#tart-backend) describes, and a VM its release could not
   delete is deleted by its VM shim within 15 s more.
4. `start` gives the answers it has already sent, such as the one to `stop`, up to
   0.5 s to reach their clients, closes the channel, and exits, which releases the
   lock.

A second SIGINT, Ctrl+C, or Ctrl+Break skips the grace period: a `docker stop` under
way is abandoned and the ordinary containers are removed at once, while each
diagnostic container is ended with `docker kill` and kept without its export.
`start` then exits 1, and its last line names each kept container. A VM backend cannot
keep a container once its VM is gone, so it deletes each diagnostic VM without an
export and reports the lost evidence, as [Diagnostics](./diagnostics.md#diagnostics) describes; `start`
still exits 1 and names the containers. Raising Ctrl+C in a
Windows test needs `unsafe`, so the Windows handling is checked by review. On axpc
one Ctrl+C stopped the instance with exit 0, and a second Ctrl+C force-stopped it
with exit 1 (maintainer's report,
[#56](https://github.com/axross/runners/issues/56#issuecomment-6511420845)). That
the second kept the diagnostic container is not yet verified on axpc
([#56](https://github.com/axross/runners/issues/56#issuecomment-6511420845)): the
report did not say. Ctrl+Break is not yet verified on axpc
([#56](https://github.com/axross/runners/issues/56#issuecomment-6511420845)): it
could not be tested.
For Ctrl+Break, a client's own process group keeps Ctrl+C from it, but whether it
also keeps Ctrl+Break from it is unverified.

| Exit | Meaning                                                                                                                                                                                                                                      |
| ---- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 0    | `start` stopped and every removal of containers in its life succeeded, or a client's request succeeded                                                                                                                                       |
| 1    | An invalid configuration, a refused or failed `start`, a failed resume, no pool that can start, a client with no instance, a refused request or no answer in time, a failed removal at a stop, or a diagnostic container kept or not removed |
| 2    | A usage error                                                                                                                                                                                                                                |

### The Log

Each `start` writes one file, `marshal-<yyyyMMddHHmmssfff>-<pid>.log` in UTC, in a
private per-user directory:

- **Windows.** `%LOCALAPPDATA%\marshal\logs\`, created when missing. Its access
  control list is not checked, because no safe call reads it.
- **macOS.** `~/Library/Logs/marshal/`, created `0700` when missing and refused
  when it is not a directory owned by the user with that mode. A file is created
  new, set to `0600` whatever the umask, and checked.

The file is opened after validation succeeds, so a refused `start` writes none.
Every line goes to the terminal and to the file, which adds a time stamp. A write
to a terminal that is gone is ignored, and the file goes on. The newest 10 log
files by name are kept, the new one included, and only regular files named as a
log file count. An old file that cannot be removed is a warning in the new log,
and `start` goes on. A file stops at 10 MiB: a line that would leave no room for the
last line is not written, and the last line says that the file reached 10 MiB. A
failed write stops the file, with a warning on the terminal.

Every token read, from its first read in pool preparation on, and every JIT
configuration, from its registration until its container's outcome is reported,
is replaced by `<masked>` in every line, including the job container's output the
backend forwards, a secret that runs across two pieces of a long line included.
The default panic hook would write a panic's message to the terminal unmasked, so
`start` replaces it with one that names only where the panic happened. A panic of
the runtime core reaches the log as a failure that carries its message, with every
token read so far masked. A JIT configuration in that message is not: its mask
ends as the panic unwinds the slot that held it, before the instance logs the
failure.

### Containment

- **Windows.** At startup `start` puts itself in a Job Object with kill-on-close,
  through `win32job`, and never closes its handle. The operating system closes it
  when the process ends, however it ends and after its exit code is set, and every
  child in the job dies then. Docker Desktop keeps a container running after its
  client is killed, so the next `start`'s stale cleanup removes it. The Job Object
  against Docker Desktop is not yet verified on axpc
  ([#56](https://github.com/axross/runners/issues/56#issuecomment-6511420845)): the
  hard-kill check was not run there, by the maintainer's decision.
- **Unix.** Each job container's `docker run` client is started by the shim, the
  CLI run again as the hidden
  `marshal shim --name <container> -- <docker> <arguments>`. The shim starts the
  client in a process group of its own, with the client's standard error joined to
  its standard output, which `start` reads and masks. Its standard input is the
  read end of a liveness pipe whose write end only `start` holds, and `start`
  never writes to it. When the client ends while the pipe is open, the shim exits
  with its code. When the pipe has closed, because `start` dropped the run or ended
  however it ended, the shim runs `docker rm -f <container>` within 5 s, kills the
  client, and exits, even when the client ended at the same time. For a diagnostic container `start` runs the shim
  as `marshal shim --name <container> --diagnostic -- <docker> <arguments>`, and
  the shim then ends its container with `docker kill` within 5 s and keeps it. The
  shim shares `start`'s standard error, and its own warnings there name containers
  only.
- **VM shim.** A job VM's `tart run` is held by the VM shim, the CLI run again as
  `marshal vm-shim`, with a liveness pipe of its own, as [The VM Shim](./tart-backend.md#the-vm-shim)
  describes. It stops and deletes its VM within 15 s when the pipe closes, reporting
  the lost evidence of a diagnostics-enabled pool's VM on the standard error it shares
  with `start`, and holds no lock.
- **Exporter.** The exporter child under [Diagnostics](./diagnostics.md#diagnostics) has a
  liveness pipe of its own and keeps its own deadline, on every system, and on
  Windows it is also in the Job Object.

Security lists the
[residual risks](../security/local-agent.md#tokens-stay-out-of-the-log-and-job-clients-end-with-start)
that containment and the log leave.

### Console Close on Windows

When the console closes, the user logs off, or the machine shuts down, `start` stops
the instance with forced removal: the slots stop registering, so no container starts
unseen, one `docker rm -f` removes every ordinary container it started, and, at the
same time, one `docker kill` ends every diagnostic container and keeps it without its
export, which the next `start` finalises. Whatever has not finished after 4 s is
dropped, and a diagnostic container whose kill did not finish runs on until the next
`start` finalises it, the answers already sent get up to 0.5 s, and `start`
exits. The 4.5 s fits inside the 5 s the operating system gives a console-close
handler before it ends the process, and Microsoft documents that logoff and shutdown
events reach services only, never an interactive process
([HandlerRoutine](https://learn.microsoft.com/en-us/windows/console/handlerroutine)).
The handlers are registered anyway, and for those two events the Job Object and the
next `start` are the backstop.

That the handler waits for the cleanup is known from the async runtime, not from a
test. Returning from the handler of these three events ends the process at once.
From version 1.44.0
([tokio#7122](https://github.com/tokio-rs/tokio/pull/7122)), tokio's Windows
handler does not return for them while a listener is registered: it notifies the
listener and parks the handler thread, so the process lives until `start` finishes
the removal and exits by itself. `cli/Cargo.toml` requires 1.54.1. No test can raise
the close event. On axpc closing the window worked as described (maintainer's
report, [#56](https://github.com/axross/runners/issues/56#issuecomment-6511420845)),
but the diagnostic containers' kill within the 4 s is not yet verified on axpc
([#56](https://github.com/axross/runners/issues/56#issuecomment-6511420845)): the
report gave no timing.

### What the Command Tests Cover

`start_command.rs` runs `start` and its clients as real processes, through
`commands-helper`, on a test root,
with the `fake-docker` helper named by `MARSHAL_TEST_DOCKER` and the loopback
GitHub stub: the exit codes, resume, `status`'s fields, a restart that judges the file
at the path `start` was given as `validate` does, a `stop` that gets its answer
from an instance with no container, a `stop` that gives up after 35 s while the
lock stays held, a second `start` that runs nothing, a stop within 30 s when
`docker stop` ignores the grace period, the
signals and a second SIGINT on Linux and macOS, at most 10 log files, and a run
through a failed registration and a stop that keeps the token and the JIT
configuration out of the terminal, the log file, and every client's output. Its
process-tree tests observe a child's exit directly: on Unix, killing `start` closes
standard error only when every shim has ended, after which no job container is
left; on Windows, a handle to the job container's client is signalled when
`start` is killed. `runtime_core.rs`, and the board and masking cases in
`pool_preparation.rs` and `slot_worker.rs`, use the fake seams, and
`instance_state.rs` and `core_restart.rs` cover forced removal, the exit deadline,
a panicking core, and no startable pool. The diagnostics tests are listed under
[Diagnostics](./diagnostics.md#diagnostics).

`axmc_start.rs` runs `start` on the axmc profile as real processes, with the fake
`tart`, the fake `softnet` and the fake `docker` in the VMs: a configuration of
Linux and macOS pools that runs each pool in a VM of its OS, a macOS slot that
waits for the limit while a leftover and a running macOS VM count, a macOS job VM
that fails to delete and is deleted again while its slot waits, and a macOS-only
configuration without a golden macOS VM that exits 1; a missing `tart`, a missing
`softnet`, version refusals, Softnet setup refusals and a `softnet` that does not
answer, each exiting 1 before any VM is cloned or booted; a `tart` that does not
answer, which makes the slots wait; a newer patch, with its warnings in the log and
`status`; a restart and two `start`s that a refused prerequisite leaves stopped with
its reason, the last resuming; and a refusal after `start` that holds a slot while
another slot's job ends, and lets it boot again. `prerequisite_gate.rs` shows where
the instance asks for the check, on the fake seams.

`instance_signals.rs` wires the signals into the instance on the fake signal source,
backend, core and clock: a wake that stops and deletes every VM, a primary network
that differs on two checks (a VPN taking the default route, a lost route naming
`none`) and on one check, the same network, a hardware address that is not read, a
resume that arms the sleep baseline and records the network afresh, the gate that
opens at the second check of a first network, and the keep-awake hold held through
running and stopping, released at stopped, and retried after a failure.
`axmc_signals.rs` runs the same as real processes with the scripted source: a wake and
a network change stop the instance with their reasons, no VM is cloned and `status`
waits for a network until one is there, a stop ends that wait, and the hold's state
is read from the scripted source's file.

`build_command.rs` runs `build` the same way, from an empty directory, with
`fake-docker` keeping an image store: the one build's arguments, labels, temporary
tag, and context bytes, equal to the definition's files and the same on a second
run; the order of the calls; the leftover, tag-removal, and prune cases; a failed
build and a failed retag; Docker not answering; the digest, axmc, usage, and
validation refusals; `stop` and SIGINT at each step of an abort, a stalled retag
and its inspect included; and the refusals while an instance exists and while a
build runs. The unit tests in `cli/src/build/steps.rs` run the steps against a
`docker` script on Unix, with a short command budget: a retag that stalls with no
stop, which prunes when it committed and is reported as a stall when it did not,
and one that succeeds within a stop's grace, which commits without an inspect.
`image_check.rs` runs `start` against images that cannot run jobs.
`golden_build.rs` runs the axmc build ([The Golden VM Build](./build-images.md#the-golden-vm-build))
against the fake `tart`, `softnet` and `docker`, which are scripted per step: the order
of the calls and the build VM's name; a failure scripted at the pull, the clone, the boot,
each provisioning call, the engine wait, the image build, the contract check, the tag, the
`sync` before the stop, the shutdown and the rename, each of which deletes the build VM
and keeps the old golden VM, with the rename only after success; a key with another fingerprint and an empty listing,
which fail before anything is installed; a stop at the pull, the clone, a provisioning
call and the image build, each within the stop sequence; a stop after the rename;
leftover build VMs deleted before the pull; the other golden VMs and only the retired
pins deleted after the commit; `TART_NO_AUTO_PRUNE` and `PATH` on every call; and, as a
real process under the VM shim, a missing `tart` or `softnet` exiting 1 before any VM
boots, the default and the configured image name, SIGINT within 30 s, and a killed CLI
whose build VM the shim deletes. `real_docker_build.rs` runs `build` through a real Docker daemon on Linux, with the
test definition in `cli/tests/build-definition/`, the runner image's digest-pinned
base and its declared labels, and fails unless the result carries all five labels,
no temporary tag remains, and the container backend's image check finds the image
matching and an absent tag missing; it is `#[ignore]`d for a local `cargo test`.
Verified on axpc (maintainer's report,
[#56](https://github.com/axross/runners/issues/56#issuecomment-6511420845) and
[#56](https://github.com/axross/runners/issues/56#issuecomment-6511344317)): Ctrl+C
and, apart from its timing, the console close on Windows, and a `build` of the full
image on Docker Desktop. Ctrl+Break is not yet verified on axpc
([#56](https://github.com/axross/runners/issues/56#issuecomment-6511420845)): it
could not be tested. Not verified: macOS and Windows behaviour beyond what the
hosted runners show.
