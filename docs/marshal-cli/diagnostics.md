---
type: Specification
title: "Diagnostic jobs and retained evidence"
description: "When is evidence exported, retained or lost after a job or a host failure?"
---

# Diagnostic jobs and retained evidence

## Diagnostics

A pool with `diagnostics` on runs each job in the diagnostic variant under
[Backend](./docker-backend.md#backend): no `--rm`, the label `runners.diagnostic-lifecycle=1`, and the
guard followed by the image's collector, in raw mode when `diagnosticRawRecords` is
on. The rest of the run contract is unchanged. Docker keeps the container after it
ends, and the CLI finalises it: it exports the container's evidence into a bundle in
the pool's sink, `diagnosticDirectory`, and then removes the container with
`docker rm -f`, whatever the export did. The image's
[README](../../images/actions-runner/README.md) owns what the collector records, and
[Windows Runner Host](../runner-hosts/windows-diagnostics.md#collecting-private-diagnostics) the
operator's side: the private sink, reading a bundle, and the manual recovery.

**Recorded settings.** The backend retains each diagnostic container it starts,
with the pool, the `diagnosticDirectory`, and the `diagnosticRawRecords` it ran with,
until a removal of it succeeds. A panic that unwinds the core's workers keeps the
record, as the backend keeps container names. Every finalisation of the container
uses that record, never the settings of a plan read later, so a resume or a
`restart` that changed the sink still exports into the old one. A container an
earlier process left has no record; Startup below adopts it with the pool's
current settings. The record also notes once a bundle was admitted for the
container, so a finalisation after a failed removal only removes it again and
writes no second bundle.

**One owner.** At most one path finalises a container at a time:

- **Job end.** The slot finalises its own container on one clock, started when the
  container ended: export by 18 s, removal by 24 s. The slot registers again only
  after the removal was attempted. A slot that ends while a stop, a `restart`, a
  core failure, or a move to stopped is under way leaves its container to the
  container steps.
- **Container steps.** Each run of them, at a stop, a `restart`, a core failure, or
  a move to stopped, finalises every retained container nobody holds, on the steps'
  clock, and waits, within the exit deadline, for each one another path holds.
  When that path lets go of a container it did not remove, such as a pool
  preparation that the stop cancelled, the steps finalise it on the same clock.
- **Ending early.** A finalisation that ends for any reason, a panic, an error of
  the core, a cancellation, or the exit deadline, leaves its container retained and
  free, so the container steps under way, or the next ones, finalise it.
- **Preparation.** Pool preparation, which runs again at every resume, `restart`,
  and core restart, leaves the containers the backend retains alone, with their
  records.
- **`diagnostics`.** It acts only on retained containers nobody holds whose job has
  ended.

An export with a gap is a warning,
`Diagnostic gap in pool <pool>, container <name>: <text>`, where the text names the
gap: a missing final observation, a full sink, a failed or unsafe
export, or a clock that left no time to begin it. It is never the slot's failure and
does not change its backoff. A removal that fails or runs out of time is a warning,
and the container stays retained for the next finalisation.

**The stop timeline.** The container steps give the diagnostic containers one
`docker stop --time 10`, Docker's default grace, ended at 12 s. Each export starts
when that stop returns and ends by 18 s, and removal starts once the export child has
ended, by 19 s. A stop that fails is a gap, and the export and removal still run. A
container whose export cannot begin by 18 s is removed with a gap. From the request
at 0 s:

| By (s) | Ordinary containers                                   | Diagnostic containers                                 |
| ------ | ----------------------------------------------------- | ----------------------------------------------------- |
| 0      | registering stops                                     | registering stops                                     |
| 12     |                                                       | one `docker stop --time 10` has returned or is killed |
| 18     |                                                       | each export child has ended or is killed              |
| 19     |                                                       | a killed child has ended; removal begins              |
| 20     | one `docker stop --time 15` has returned or is killed |                                                       |
| 24     |                                                       | one `docker rm -f` per container has ended            |
| 26     | one `docker rm -f` for the rest has ended             |                                                       |
| 28     | whatever still runs is dropped                        | whatever still runs is dropped                        |
| 28.5   | answers reach their clients                           |                                                       |

Both columns stay inside the 26 s container steps and the 28 s exit deadline, so
`stop` still ends within 30 s and its 35 s wait is unchanged. A backend that reserves
a release budget `R` (the [Tart Backend](./tart-backend.md#tart-backend) reserves 10 s) leaves the
release the time after the container steps, so the diagnostic column's times fall
earlier by `R` less the 4 s it leaves before the exit deadline. With an `R` of 10 s
the stop is ended at 6 s with a grace of 4 s, each export by 12 s, and each removal
by 18 s; the export and removal windows keep their sizes, and the release keeps its
10 s before 28 s, with no slack after it, so a VM that the release could not delete
in a double fault is deleted by its VM shim within 15 s after `start` exits, or by
the next `start`.

**Startup.** While a pool is prepared, the CLI looks at the labelled containers of
the pool that an earlier process left and the backend does not retain:

- **Diagnostics on.** It retains them with the pool's current settings and
  finalises them on one clock per pool, as at a stop, and reports each one:
  `Diagnostic container <name> that an earlier run left: exported, removed.`, with
  `export gap` or `not removed` in place of either word.
- **Diagnostics off.** It gives them one bounded `docker stop --time 10`, ended at
  12 s, warns for each, and refuses the pool. The reason names
  the manual recovery in
  [Windows Runner Host](../runner-hosts/windows-diagnostics.md#recover-before-disabling-diagnostics).
  The CLI keeps their names until the next preparation of that pool, or until a
  configuration without that pool is applied, so `diagnostics` can report them.

**Forced stop and kept containers.** A second Ctrl+C or SIGINT, and the Windows
console close, skip the export. Each diagnostic container is ended with
`docker kill` and kept, so no job runs on unsupervised and the evidence survives,
while the ordinary containers are removed as before. An export under way has its
liveness pipe closed at once, before the kill; a container that has already stopped
answers that it is not running, and that is ignored. On Unix a diagnostic
container's shim runs in diagnostic mode: when its liveness pipe closes, it ends its
container with `docker kill` instead of removing it. So on Unix a hard kill of
`start` and a forced stop both leave the container stopped and kept. On Windows a
forced stop does the same, but a hard kill ends only the `docker run` client
through the Job Object, so the job runs on until the next `start` finalises the
container, as an ordinary one does. At a core panic on Unix the unwound run lets the
shim end the container before the container steps stop it, so that export has a gap
with no final sample. `start` exits 1 when it kept a diagnostic container, or could
not remove one, and its last line names each one. The next `start` finalises them
while their pool's diagnostics are on, and reports each one; an export gap alone
does not change `start`'s exit code.

**Diagnostic VMs.** On axmc the diagnostic container lives in a job VM, which is the
retained thing: it stays alive after the job until the exporter has read the
evidence through the guest agent, and its removal is the VM's deletion. A graceful
stop, `diagnostics`, and the job end finalise it as they do a container. A forced
stop, a hard kill of `start`, and the next `start` cannot keep the evidence, because
nothing keeps a VM alive without its holder. They delete the VM without an export and
report the loss with the same gap wording as an export gap, and a forced stop still
exits `start` with 1, naming the containers whose evidence is lost. A crash reports
the loss from the VM shim, on the standard error it shares with `start`.

**The exporter.** Each export runs in a child process: the CLI run again as the
hidden
`marshal export --pool <pool> --directory <sink> [--container <name>] [--vm <name>] [--raw-records] [--checkout <path>] --deadline-ms <ms> -- <docker or tart>`.
Without `--container` it only counts the pool's bundles. With `--vm` it reads the
evidence from that VM through `tart exec`, with the program after `--` as `tart`; the
bundle, its files, limits, and exit codes are the same. Its clients are then `tart exec`
clients that run `docker` in the VM, and the Isolation item below applies to them as
it does to the `docker` clients.

- **Sink checks.** Before each bundle the child checks the sink. It must be an
  existing directory outside the checkout above the configuration, judged by the
  checkout rule's lexical comparison, and neither it nor any directory above it may
  be a link or, on Windows, a reparse point; so on macOS a sink under `/tmp` or
  `/var` must be named by its `/private/…` path. A missing sink is refused, never
  created, and nothing is repaired. On Unix the sink must be owned by the current
  user with mode `0700`, a bundle is created `0700`, and each file in it `0600`. On
  Windows the sink's owner and access rules are not checked, and a bundle inherits
  the sink's rules; [Security](../security/local-agent.md#diagnostic-evidence-stays-private)
  records the risk.
- **Admission.** The instance takes a lock for the pool and sink, then starts the
  child. The child counts the sink's `<pool>-bundle-` entries, partial bundles
  included, refuses at 10 as "sink full", creates
  `<pool>-bundle-<32 hexadecimal digits>`, and prints the line `admitted` and then
  `bundles <n>`. The instance releases the lock on `admitted`, or when the child
  ends. A second export of the same pool waits for the lock until its own export
  deadline. Every export happens inside the one instance, which the machine-wide
  lock makes unique, so no named mutex is needed.
- **Identity and archive.** The child writes `identity.txt` from the inspect
  allowlist under [Backend](./docker-backend.md#backend), its output capped at 1 KiB and required to
  match a fixed shape. It reads the `docker cp` archive through a strict ustar
  subset. It accepts the collector's directory entry, named `./`,
  `.`, or `runner-diagnostics/` with size 0, and regular files named `metrics.txt`
  or, only when raw records are on, `raw-<n>.log` with `<n>` of one to three
  digits. Either file name may follow `runner-diagnostics/`, and either of those
  may follow `./`, so `metrics.txt`, `./metrics.txt`,
  `runner-diagnostics/metrics.txt`, and `./runner-diagnostics/metrics.txt` are all
  accepted. It refuses a prefix field, a link or special file, any other name, an
  absolute or `..` path among them, a bad checksum, and a duplicate once those
  prefixes are dropped. It reads at most 132 headers, with `metrics.txt` at most
  8 MiB, raw files 32 MiB together, and 40 MiB of payload in all, less 4 KiB kept
  for the bundle's own files. It writes `metrics.txt`, each accepted raw file, and
  `complete.txt` last. A `metrics.txt` without
  `final=observed` is a gap.
- **Isolation.** The child starts in a process group of its own, as the shim does,
  so a terminal's first Ctrl+C reaches only `start`, and a job-end export goes on
  during a graceful stop. Its standard input is a liveness pipe from the instance,
  and the instance passes it the export deadline that remains. The child keeps that
  deadline itself, and ends when it passes or when its standard input closes,
  killing its clients first: on axpc its `docker` clients, and on axmc its `tart exec`
  clients, which run `docker` in the VM. Each client runs in a process group of its
  own, killed when its call is dropped, with the JIT variable cleared and its standard
  error discarded, and every file system call runs on a thread of its own, so a hung
  sink cannot keep the child from ending. At the export deadline the instance closes the liveness pipe, and kills
  the child 1 s later only if it has not ended. On Windows the child also inherits
  `start`'s Job Object. So the child outlives neither its deadline nor its
  instance, and its clients end with it whenever it ends on its own. A child killed
  1 s after its pipe closed cannot kill its clients: on Unix they run until each
  ends by itself, which is what the `tart exec` clients of axmc do, since macOS has
  no Job Object, and on Windows until `start` ends and its Job Object closes.
- **Exit codes.** 0 is a complete bundle, or a count; 3 a complete bundle with a
  gap; 4 a full sink; and 1 anything else, a refused sink, a failed copy, an archive
  outside the subset, a deadline, or a closed pipe. Containment needs no exit code
  of its own: the Job Object on Windows and the liveness pipe on Unix are in place
  before the child starts.
- **Bounded output.** The child prints only those two lines. On standard error it
  writes at most the one line a panic gives, which names only the code location.
  The CLI never prints or logs the evidence: a warning names only the pool, the
  container, and the gap.

**`diagnostics`.** The client asks the instance to finalise every diagnostic
container it retains whose job has ended and nobody holds: one whose removal failed,
or one a `restart`, a core failure, or a move to stopped left unfinished. It never
touches a container whose job still runs. It finalises them on one 24 s clock, as at
a stop, with each one's recorded settings. A kept container whose pool now has
diagnostics off, from an earlier process, has no recorded settings; `diagnostics`
names it on a line of its own and leaves it to the manual recovery. It prints:

- one line per pool that had a diagnostic container or has diagnostics on:
  `pool <name>: <k> bundles written, <r> of 10 remaining`. The remaining count comes
  from the export children that ran, or for a pool with nothing to export from a
  count-only run of the child under the same 24 s clock; a count that fails is
  `unknown`, and so is the count of a pool whose only container was left to the
  manual recovery, which has no sink to count;
- then one line per container whose export had a gap, whose removal failed, or
  which was left to the manual recovery, such as
  `container <name>: removal failed; it stays retained`.

A retained VM is finalised the same way: the export reads it through the guest
agent, and its removal is the VM's deletion, so a VM whose deletion failed is
reported as `removal failed; it stays retained` and deleted by a later `diagnostics`
once Tart can, without a second bundle.

It exits 0 when every container it acted on was removed, and 1 when one was not or
one was left to the manual recovery. With nothing to act on and no
diagnostics-enabled pool it says so and exits 0. It is allowed while the instance
is stopped, because a stopped instance can still retain containers, and a stop that
arrives while it runs lets it finish within the exit deadline.

No bundle file, warning, log line, or channel answer holds a token or a JIT
configuration: the identity allowlist never reads the environment, and the archive
holds only what the collector wrote.

The tests use the `fake-docker` helper, which can keep containers, answer `inspect`
and `cp` with a scripted archive or a stall, and stand in for the exporter child:

- `diagnostic_archive.rs` covers the archive reader's accepted and refused
  entries, and the limits at and one past each bound;
- `diagnostic_export.rs` runs the container backend and the real exporter child:
  the bundle and its modes, raw mode, a gap, a container removed whatever the
  export did, the identity cases, a retried removal with no second bundle, an 11th
  bundle refused with two exports at once, the startup finalisation, and in real
  time a stalled copy, a stalled copy and removal, an overrunning child, and a
  closed liveness pipe, each ending at its deadline, and all but the killed
  overrunning child with their clients;
- `diagnostic_ownership.rs` drives the instance, the runtime core, and the
  finalisation every backend shares, over the fake backend on the system clock: a stop and a `restart` during a job-end export, a
  stop during a startup export, a panic during a job-end export, `diagnostics` skipping a held container, a resume after a failed
  removal, a container the stop's `docker stop` ends, a `restart` and a core failure
  after the configuration changed, a forced stop, and the `diagnostics` answers and
  states;
- `diagnostic_start.rs` runs `start` and `diagnostics` as processes: a complete
  bundle, a gap, the token and the JIT configuration kept out of every output, a
  failed and then retried removal, the manual-recovery and `unknown` lines, an
  exporter whose instance is killed, a mixed stop within 30 s, and on Linux and
  macOS a first SIGINT during an export, a second SIGINT, and a hard kill;
- `tart_diagnostics.rs` runs the Tart backend and the real exporter child against the
  fake `tart` and the fake `docker` in its VMs: the bundle exported through the guest
  agent, token-free, before the VM is deleted; a copy that hangs in the VM, which ends
  the export at its deadline and its `tart exec` client with the exporter; a
  malformed or oversized archive, a missing final observation, a full sink, a failed
  stop and a clock with no time left to begin the export, each with the container's
  gap; `diagnostics` finalising a retained VM; a VM kept for its finalisation at a
  stop; a forced stop that deletes the VM without an export and reports the gap; and
  a VM already gone;
- `axmc_diagnostics.rs` runs `start`, `stop`, and `diagnostics` as processes on an
  axmc host with a diagnostics-enabled Linux pool: a finished job exported and its VM
  deleted, a graceful stop that exports a running job first, a second SIGINT, the
  next `start` deleting a leftover, a killed `start` whose shim deletes the VM and
  reports the gap, a killed `start` whose exporter ends with the `tart exec` client of
  its copy, and `diagnostics` finalising a VM whose deletion failed;
- `sink.rs`'s unit tests refuse a sink that is missing, under a link, inside the
  checkout, also when the checkout is named through a link, or not private, and on Windows one that is or lies under a junction or
  a directory symbolic link.

Verified on axpc (maintainer's report,
[#56](https://github.com/axross/runners/issues/56#issuecomment-6511420845)): a
diagnostic job on Docker Desktop, and [Windows console-close handling](./foreground-commands.md#console-close-on-windows),
apart from its timing.
Not verified: the macOS behaviour of the process-group signal the SIGINT tests
send, beyond what the hosted runners show.
