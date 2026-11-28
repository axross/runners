---
type: Specification
title: "Docker backend contract"
description: "Which Docker commands, isolation and resource limits implement a container job?"
---

# Docker backend contract

## Backend

The runtime asks a backend to wait until it is available; check the configured
image, as [The Image Contract](./runner-images.md#the-image-contract) describes; list a pool's
containers and remove containers; prepare volumes; prepare and release a job host,
as below; choose a slot's CPUs; run a
container under the run contract and report its exit code; name the containers it
has started and not seen end; stop, end, or remove a set of containers within a
budget; and, for a diagnostics-enabled pool, retain each diagnostic container it
starts with the settings it ran with, export one by a deadline, and count a pool's
bundles in its sink, as [Diagnostics](./diagnostics.md#diagnostics) describes. The runtime's own
finalisation, the same for every backend, decides whether to export and removes
the container after it. The JIT
configuration reaches the backend as a `Secret`, whose `Debug` form is redacted and
which has no `Display`. The container backend passes it as an environment variable
by name; the [Tart Backend](./tart-backend.md#tart-backend), which runs the containers in a virtual
machine, passes it on the standard input of the launcher in the VM. A fake backend implements the same seam for the tests of the code
above it.

The backend also provides two values the runtime core would otherwise take from the
container backend's modules: the command a job container runs after the launcher
verifies its CPUs, and the wait between checks while the engine does not answer.
The container backend gives `/home/runner/run.sh` and 5 s, so nothing on axpc
changes.

**Job hosts.** A job host is whatever one job runs on. A container on axpc runs
directly on Docker Desktop, so the container backend's host is nothing; a backend
that gives each job a virtual machine makes and deletes that machine as the host.
Two steps, per job and per slot, carry this:

- `prepare_job_host` runs after the image check and before any CPU probe or
  registration. It is given the slot, its vCPUs, memory and disk, and answers one of
  three outcomes. **Ready**: the host can take the job. **Failed**: the host could
  not be made, and the slot backs off like any other failed attempt. **Held**: a
  host prerequisite refuses this boot, with a reason. A held slot boots nothing,
  warns in the log, shows `held: <reason>` in `status`, and tries again on the same
  5 s doubling to 300 s schedule, and a job running on another slot is not
  affected. The reason `status` shows is scrubbed, joined onto one line and cut to
  500 characters followed by `...`, as a backing-off runner's is; the log line keeps
  the whole reason. A held slot is not a failure of the runtime core, and it counts
  for no restart. An engine that does not answer is not an outcome of this step: the
  backend waits for it in `wait_until_available`, or reports the image check as
  unavailable, as Docker does today.
- `release_job_host` is given a budget and gets rid of the host. It runs once after
  every preparation, whatever the preparation answered, and on every path from there
  on, before the slot backs off: after a failed or held preparation, a failed CPU
  probe, a failed or refused registration, a run that ended or failed, and a stop at
  any of those points. A diagnostic container is exported first, so the host is
  released after the export. A release that fails is a warning and changes nothing
  else, except that a macOS VM still waiting for its deletion is then deleted again
  through `finish_release` until it is gone, before the slot backs off. A stop ends
  that wait and its sequence releases the host; a drain ends it too, and the slot's
  next preparation deletes the VM.

The backend counts a host as prepared from the start of its preparation until its
release and lists the hosts it still counts, so the stop sequence can release the
ones whose worker could not: a release does nothing for a host that is not
prepared, and may run twice at once for one host. A backend also names the budget
the stop sequence reserves for its releases; the container backend has no hosts, so
its answer is zero.

The container backend prepares every host as ready without running a command,
releases nothing, and lists no host. Its argument lists, messages, and exit codes
are unchanged.

The container backend runs `docker` with the run contract's argument lists. The
image smoke tests run the same container commands through the helpers
`smoke-docker-commands.ps1` and `smoke-diagnostic-export.ps1` in
[`images/actions-runner/tests/`](../../images/actions-runner/tests). A test reads the
launcher guard and the probe's reader script out of `smoke-docker-commands.ps1`
and compares them with the backend's, so a change to either side alone fails the
CLI's tests; the tests compare every other list with one written out in the test:

| Purpose                    | Command                                                                                                                                                                                                                                                        |
| -------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Wait for Docker            | `docker info`, again every 5 s with a warning while it fails or cannot start                                                                                                                                                                                   |
| Check the image            | `docker image inspect --format <labels> <image>`, then `docker image ls --quiet <image>` when it fails                                                                                                                                                         |
| List a pool                | `docker ps -a --filter name=<pool>- --format {{.Names}}`, then the same with `--filter label=runners.diagnostic-lifecycle=1`                                                                                                                                   |
| Remove                     | `docker rm -f <names>`                                                                                                                                                                                                                                         |
| Volumes                    | `docker volume create <name>` for each, then `docker run --rm --pull never --user root --mount type=volume,source=<name>,target=<path>... <image> chown runner:docker <paths>`                                                                                 |
| Probe a slot's CPUs        | `docker run --rm --pull never --network none --name <pool>-cpu-probe-<slot> --label runners.cpu-probe=1 <image> bash -c <reader>`                                                                                                                              |
| Run a job container        | `docker run --rm --pull never --name <n> --cpus <q> --cpuset-cpus <set> --memory <G>g --memory-swap <G>g <--mount ...> -e ACTIONS_RUNNER_INPUT_JITCONFIG <image> bash -c <launcher> -- <set> /home/runner/run.sh`                                              |
| Run a diagnostic container | `docker run --pull never --name <n> --label runners.diagnostic-lifecycle=1 --cpus <q> --cpuset-cpus <set> --memory <G>g --memory-swap <G>g <--mount ...> -e ACTIONS_RUNNER_INPUT_JITCONFIG <image> bash -c <diagnostic launcher> -- <set> /home/runner/run.sh` |
| Read its identity          | `docker inspect --format <allowlist> <n>`                                                                                                                                                                                                                      |
| Copy its evidence          | `docker cp <n>:/tmp/runner-diagnostics/. -`                                                                                                                                                                                                                    |
| Stop, for the later `stop` | `docker stop --time <seconds> <names>`, one call for all names                                                                                                                                                                                                 |
| End without removing       | `docker kill <names>`, one call for all names                                                                                                                                                                                                                  |

The stop call always passes `--time <seconds>`, the grace its caller chose, so no
caller inherits Docker's default of 10 s.

The launcher is the guard script followed by `exec "$@"`. It reads
`Cpus_allowed_list` from `/proc/self/status` and exits 125 unless it equals the set
the container was given. The Docker runtime's job container has no network option,
`--privileged`, socket mount, or `--user`, and an ordinary one has no label. The VM
runtime's has the same list and one more option, a bind mount of the job VM's own
Docker socket, as [Tart Backend](./tart-backend.md#tart-backend) describes. The JIT variable is
passed to the one `docker run` client that starts the container, in that client's
environment, and is removed from every other client's.

The diagnostic variant differs from an ordinary job container only in its missing
`--rm`, its label, and its launcher: the same guard, then
`exec /usr/local/bin/runner-diagnostics`, with the argument `raw-run` when
`diagnosticRawRecords` is on, or, in an image without the collector, a
`Diagnostic gap: collector missing.` line on standard error and the runner itself.
The identity allowlist is
`{{.Image}}|{{.HostConfig.NanoCpus}}|{{.HostConfig.Memory}}|{{.HostConfig.MemorySwap}}|{{.HostConfig.CpuQuota}}|{{.HostConfig.CpuPeriod}}|{{.HostConfig.CpusetCpus}}|{{.State.OOMKilled}}|{{.State.ExitCode}}`,
so the environment, and the JIT configuration in it, is never read. A unit test in
`cli/src/image/contract.rs` fails when `smoke-docker-commands.ps1` no longer holds the
variant's argument list and launcher as the CLI writes them. The identity and copy
commands are held equal to `smoke-diagnostic-export.ps1` by a test in
`cli/src/runtime/docker_args.rs`, and the completion marker by one in
`cli/src/diagnostics/exporter.rs`.

Every client starts in its own process group: `process_group(0)` on Unix, and
`CREATE_NEW_PROCESS_GROUP` on Windows. A Ctrl+C in the terminal then reaches the CLI
only, which can stop the containers in order. A test reads the group of every client
the fake `docker` sees on Linux and macOS; no safe API reads a Windows process's
creation flags back. On Windows every client and the exporter child also get
`CREATE_NO_WINDOW`, so a resident agent that has no console, as the app's agent has
none, does not open a console window for each `docker` call. This applies to the
foreground commands too, since they share the builders. Their process group is
unchanged. Most clients have piped or closed standard streams, but on Windows two
children inherit the CLI's own: in terminal mode `build`'s `docker build` inherits its standard output
and standard error, and the exporter child inherits its standard error. How a terminal
run behaves on Windows with the flag, including `start`, Ctrl+C and whether a terminal
`build` still streams its output, is not established here: the Windows CI tests only
record the flags or run the CLI with piped output, so only the axpc run shows it. The one function that applies the flags is called by the client builder and
by the exporter's command builder, and a test build records what each applied, so
two unit tests assert both flags at the two builders, which every client and exporter
call site goes through. Whether Windows shows a window is not observable here and is
listed with the axpc checks. A client is killed when the future that waits for it is
dropped.

Every client other than a job container is bounded. A listing, a volume command,
and an info call are bounded at 120 s. A removal and a stop take the budget their caller passes; stale cleanup removes
within 120 s. The CPU probe and the removal of a leftover probe have the budgets
under [CPU Placement](./pool-runtime.md#cpu-placement). A `docker info` that fails, hangs, or cannot start (the
program is missing) means Docker is not available yet. Output from a job container
is forwarded to the log line by line, and the log masks every secret in it. A line
longer than 64 KiB goes in pieces, each cut where it splits no masked secret, the
bytes after the cut waiting for the next piece; see [The Log](./foreground-commands.md#the-log).
The output of any other client is read within its budget, and when it becomes error
text it is joined onto one line and cut to 500 characters followed by `...`, as a
registration error is.
