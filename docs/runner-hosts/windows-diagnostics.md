---
type: Procedure
title: "Collect and recover private diagnostics"
description: "Prepare a private sink, recover retained containers and verify that evidence is safe before disabling diagnostics."
---

# Collect and recover private diagnostics

## Collecting private diagnostics

Diagnostics are off by default. Enable them only for the entry whose evidence
you need, after separate authorization to operate that host. The default
example enables neither diagnostics nor raw records. No resource allocation,
build command, tool version, registration or mount changes with this opt-in.
The sink is not exposed to the job as a bind mount or volume.

Collect private evidence as follows:

1. Pre-create a directory on a local Windows filesystem, outside any checkout of
   this repository.
   Restrict its owner and all allowed access rules to the account that runs
   Marshal. Marshal MUST NOT create the sink, relax permissions, or
   repair its access controls, and on Windows it does not check them, so this step
   is the only protection the evidence has. For a new, empty example directory,
   review the path before running these commands as that account:

   ```powershell
   $sink = 'C:\example-evidence'
   New-Item -ItemType Directory -Path $sink
   $sid = [Security.Principal.WindowsIdentity]::GetCurrent().User
   $acl = New-Object Security.AccessControl.DirectorySecurity
   $acl.SetOwner($sid)
   $acl.SetAccessRuleProtection($true, $false)
   $rule = New-Object Security.AccessControl.FileSystemAccessRule $sid, 'FullControl', 'ContainerInherit, ObjectInherit', 'None', 'Allow'
   $acl.AddAccessRule($rule)
   Set-Acl -LiteralPath $sink -AclObject $acl
   ```

2. Set `diagnostics` to `true` and `diagnostic_directory` to that directory in
   the target pool. Leave `diagnostic_raw_records` absent or `false` unless
   private daemon/crash retention is separately wanted. Set it to `true` only
   with diagnostics enabled. Apply the change with **Stop all runners**, then
   **Start all runners**, in an authorized quiet window, and confirm that the
   window names no configuration problem. Validation rejects a sink
   inside a checkout of this repository without Docker, GitHub or token reads, but does
   not check that the sink exists, is private, or is writable.
3. After a container stops, the agent exports its evidence into a new
   `<name>-bundle-<32 hexadecimal digits>` directory in the sink, then removes the
   container, also when the export fails. Inspect the bundle privately.
   `identity.txt` holds only the actual immutable image ID, CPU/memory/swap/affinity
   readbacks, container exit and Docker's OOM flag. `metrics.txt` holds the bounded
   observer output, and `raw-<n>.log` files the raw records when they are on;
   [the image README](../../images/actions-runner/README.md)
   owns fields, sampling limits, and the exact raw-record locations. A
   `complete.txt` marker, written last, means copying finished, not that every
   observation was available or that the incident's cause is known.
4. Treat any directory without `complete.txt` as incomplete. A missing
   `final=observed` metric marks unavailable final observations after abrupt
   termination or a collection bound. Review every `gap=` and `unavailable`
   field. Missing counters MUST NOT be read as zero. A counter delta is
   container-wide, not a kill attributed to a daemon. Docker's `OOMKilled=false`
   does not exclude a killed child. Correlate time samples and process PIDs with
   separately authorized host kernel/WSL evidence; the container cannot read
   enclosing-host kill records.
5. Keep all raw records private. Marshal never shows or logs them.
   Retained contents are untrusted operator evidence, MUST NOT become build
   inputs, and MUST NOT be published without human sanitization. Public
   summaries MUST omit credentials, registrations, private paths and
   machine-identifying values; an allowlisted metric file is not automatic
   permission to publish it.

The sink admits at most ten bundles per entry, counting every `<name>-bundle-`
entry, partials included, and at most 40 MiB of payload per bundle, less a small
reservation for the bundle's own files. A bundle inherits the sink's access rules.
The export reads only the collector's regular files, by name and within fixed
sizes, and refuses links, special files, unsafe paths, and duplicates; it never
copies a container's whole filesystem or its logs. At a job end the export ends by
18 s and the removal by 24 s after the container ended, and **Stop all runners**
finalises the diagnostic containers within its 30 s. A full, inaccessible or unsafe
sink, a failed or partial copy, a missing collector, or an unavailable final sample
is a diagnostic gap, logged as
`Diagnostic gap in pool <pool>, container <name>: <text>`, never the runner's
failure. A removal that fails leaves the container retained for the next
finalisation, at the next stop or start of the runners. The window's Diagnostics
section of each pool shows its bundles written, the room left of 10, and each
container it still keeps. [CLI](../marshal-cli/diagnostics.md#diagnostics) owns the timeline, the
exporter, and the startup finalisation.

Each diagnostic container carries only the immutable
`runners.diagnostic-lifecycle=1` label, not a registration or private path. When
the agent prepares a pool, it finds the labelled containers that an earlier run of
the pool left, such as those an ended agent or the command line kept. With the
pool's diagnostics on, it exports and removes each one with the pool's current
settings before any of the pool's slots registers. Ordinary unlabelled containers
keep their ordinary removal even when the entry now enables diagnostics. Prior
evidence is never automatically deleted. Operators MUST review and privately
archive or remove prior bundles before collecting beyond the quota. Retention
outside this feature is their decision. A diagnostic job and its export on Docker
Desktop were verified on axpc with the command line: the bundle was written with its
`complete.txt`, and its `diagnostics` reported the counts and exited 0 (maintainer's
report, [#56](https://github.com/axross/runners/issues/56#issuecomment-6511420845)).

### Recover before disabling diagnostics

Recover an entry's diagnostic containers before turning its diagnostics off, and do
not start another runner against the retained evidence. When an entry's
diagnostics are already off and labelled containers of it remain, the agent gives
them one bounded `docker stop`, warns for each, and refuses that entry, naming this
section; the other entries run, and the window names the entry's reason. The agent
never removes or exports them while the entry's diagnostics are off, and does not
silently turn collection on.

Recover them with Marshal alone, after separate host authorization:

1. **Check the sink first.** Confirm that the entry's sink exists and that its
   access control step under
   [Collecting private diagnostics](#collecting-private-diagnostics) was done. List
   the containers to recover, keeping only names that are the entry's `name`, a
   hyphen, a slot number, a hyphen, and 17 digits, and record those names:

   ```powershell
   docker ps -a --filter "name=<name>-" --filter "label=runners.diagnostic-lifecycle=1" --format "{{.Names}}"
   (Get-ChildItem -LiteralPath 'C:\path\to\private-evidence' -Filter '<name>-bundle-*').Count
   ```

   The second command counts the entry's bundles in the sink. Archive enough of
   them privately, outside the sink, that the count is below 10 minus the number of
   containers to recover. The export refuses a bundle once the entry has 10, and the
   entry's next jobs after step 2 take quota too, so the count leaves them room. The
   next step removes a container even when its export fails, so a sink that fills
   during the recovery loses that container's evidence.
   Never dump configuration or registration values.

2. **Recover.** Keep `diagnostics` on for that entry, or turn it on again with
   `diagnostic_directory` set to the sink from step 1. Then choose
   **Start all runners**, or, when the runners are already running,
   **Stop all runners** first, in an authorized quiet window, because a stop stops
   every entry's job containers. The entry's preparation adopts each leftover container, exports it, and
   removes it before any slot registers, and logs one line for each:
   `Diagnostic container <name> that an earlier run left: exported, removed.`, with
   `export gap` or `not removed` in place of either word. It removes the container
   even when the export fails, reporting an `export gap`; that is why step 1 comes
   first.
3. **Confirm.** First wait until the agent's log has step 2's line for every
   container step 1 recorded.
   Treat a line with `export gap` or `not removed` as not recovered: an
   `export gap` container is already removed, and its evidence is lost. Run step 1's
   `docker ps -a` listing again and confirm that none of the names step 1 recorded
   is still listed. Other names can appear: step 2 started the entry's slots with
   diagnostics on, so each live slot's job container has the same name pattern and
   label. Those are expected, and must not be removed. Then confirm that the
   entry's Diagnostics section in the window keeps no container. A container still
   kept, such as one whose removal failed, is finalised again at the next
   **Stop all runners**; or abandon it as in step 5.
4. **Then disable.** Choose **Stop all runners**, set `diagnostics` to `false`,
   remove `diagnostic_directory` and `diagnostic_raw_records`, then choose
   **Start all runners** and confirm that the window names no configuration
   problem.
5. **Abandon, only by choice.** If you choose to abandon a container's evidence,
   choose **Stop all runners**, then remove only that identified, labelled
   container with `docker rm -f <name>`, never with a prune, and start the runners
   again.

This procedure is source guidance, not authorization to operate a host.

The CLI's diagnostic tests run on Linux, macOS, and Windows hosted runners, and
CI's image smoke command exercises the image's lifecycle and export with synthetic
jobs, no production registration, networking or shared mounts; neither is Docker
Desktop/WSL runtime behavior. Required hosted PR image evidence is still needed
when a local machine has no Docker daemon. Real-host diagnostic overhead and
retention remain unmeasured until a separately authorized Windows check.

Diagnostics alone MUST NOT be described as fixing daemon disappearance or
establishing an OOM kill, JVM crash, actual slots or peak usage. Missing runtime
evidence remains unknown.
Real-host diagnosis, memory/slot/CPU-affinity comparisons selected independently
from measured pressure and host headroom, and ordinary setup-action/SDK-manager/
wrapper reuse verification are separately approved follow-ups. Do not infer a
resource remedy or tool-reuse correction from fixture results, and do not rerun
the consumer workflow, alter host settings, roll out an image or inject build
options under this diagnostic-source change.
