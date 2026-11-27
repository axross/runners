---
type: Specification
title: "macOS VM pools and capacity"
description: "Which sizing, base-image and VM-count rules apply to building and running a macOS job?"
---

# macOS VM pools and capacity

## macOS VM Pools

An axmc pool with `os: "macos"` runs its jobs in macOS virtual machines. This
section states what `validate` accepts for such a pool, the runner and base records
the golden macOS VM is built from, the names of the macOS VMs, the count of macOS
VMs against the host's limit, how `build` makes the golden macOS VM, how a job runs
in a macOS VM, and what `status` and the agent's snapshot show for them.
The pool rules apply to a pool with `os: "macos"` under the axmc profile only.
axpc pools accept what they accepted before, and so do axmc Linux pools, except
that `slots` is now optional with a default of 2.

### Defaults and Minimums

A macOS pool needs no size or count field, and each field it sets replaces its
default.

| Field         | macOS pool                        | Linux pool on axmc               |
| ------------- | --------------------------------- | -------------------------------- |
| `slots`       | Optional, default 1               | Optional, default 2              |
| `cpus`        | Default 4, at least 2             | Default 2, as before             |
| `memoryGb`    | Default 8, at least 4             | Default 8, as before             |
| `diskGb`      | Optional, default 50, at least 50 | Required, at least 20, as before |
| `diagnostics` | `true` is rejected                | Accepted, as before              |

The minimums are the golden macOS VM's: its disk is 50 GB, and the base needs at
least 2 vCPUs and 4 GB of memory. Their messages, with `i` the pool's index as in
the other axmc messages:

| Rule                    | Path and message                                                                                         |
| ----------------------- | -------------------------------------------------------------------------------------------------------- |
| `cpus` below 2          | `repositories[i].cpus: must be at least 2 on a macOS pool`                                               |
| `memoryGb` below 4      | `repositories[i].memoryGb: must be at least 4 on a macOS pool`                                           |
| `diskGb` below 50       | `repositories[i].diskGb: must be at least 50 on a macOS pool, the golden macOS VM's disk size`           |
| `diagnostics` is `true` | `repositories[i].diagnostics: not supported on a macOS pool, because macOS VM pools have no diagnostics` |

Each message comes at its field's own place in an entry's errors. The
`diagnostics` one follows the boolean type errors of `diagnostics` and
`diagnosticRawRecords` and comes before the `diagnosticDirectory` errors, which
still apply as for any pool.

- **Absent and `null`.** Only an absent field takes its default. A `null`
  `slots` or `diskGb` is a type error, `must be an integer from 1 to <max>`, as a
  `null` `cpus` or `memoryGb` is, and is not reported as missing.
- **The minimum is a rule, not a type error.** A value below its minimum is
  recorded as an error, and the pool still joins the cross-entry and budget
  checks, so one run reports a pool's size and its VM count together. `cpus` is a
  whole number on every axmc pool, so a fractional count is the type error
  `must be an integer from 1 to 64`, and the minimum is not compared.
- **The two-VM rule counts defaults.** A pool without `slots` counts one VM toward
  `budget: macOS pools total N VMs (the sum of their slots), above the limit of 2`,
  so three macOS pools without `slots` are rejected.
- **The budgets use the resolved values.** The CPU, memory, and disk sums include
  the defaults, so a macOS pool without `cpus` uses 4 CPUs of the budget.

The disk budget counts 100 GB for the golden macOS VM and its cached base when a
macOS pool is configured, as [Profiles](./json-validation.md#profiles) states.

### Migration

The macOS rules change what axmc accepts, so they are a recorded migration of the
fixtures under [`tests/host-configuration/rust/`](../../tests/host-configuration/rust).
No axpc fixture changes, and no `shared/` fixture changes. `axmc-defaults.json` and
`axmc-diagnostics.json` describe Linux pools, so their outcomes and totals stay.
`axmc-budget-at-limit.json` already sets every macOS value at or above its minimum,
so only the macOS reserve changes it, as the reserve table below lists.

Each existing fixture whose macOS pool had `"diskGb": 40` now has 50, because 40
is below the macOS minimum and each fixture was written to test something else:

| Fixture                                                        | Change                                                                                                                                                                    |
| -------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `accepted/axmc-os-labels.json`                                 | The macOS pool's `diskGb` 40 to 50; it stays accepted, and its labels are what it pins                                                                                    |
| `accepted/axmc-shared-label-different-os.json`                 | The same; it stays accepted with `macOS VMs: 1 of 2`                                                                                                                      |
| `rejected/axmc-label-linux-on-macos.json`                      | The same, so the label error stays the only error                                                                                                                         |
| `rejected/axmc-label-own-platform.json`                        | The same, so the two label errors stay the only errors                                                                                                                    |
| `rejected/axmc-label-x64.json`                                 | The same, so the label error stays the only error                                                                                                                         |
| `rejected/axmc-different-os-shared-label-different-token.json` | The same, so the token error stays the only error                                                                                                                         |
| `rejected/axmc-three-macos-vms.json`                           | Both pools' `diskGb` 40 to 50, and `budget.cpus` 8 to 12, because three default macOS slots of 4 CPUs total 12; the VM-limit error stays the only error                   |
| `rejected/axmc-budget-errors-come-last.json`                   | The first pool's `diskGb` 40 to 50 and `budget.cpus` 8 to 12, for the same reason; the disk total in the last error is 1200 GB (2 × 50 + 1000 + the 100 GB macOS reserve) |

One fixture of M1's changes because its macOS pool is below the new minimum:

| Fixture                                   | Change                                                                                                                                                                   |
| ----------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `rejected/axmc-linux-disk-too-small.json` | M1's fixture: the macOS pool's `diskGb` 19 to 50, because 19 is below the macOS minimum; the Linux pool's error stays the only error, and the macOS pool still gets none |

The 100 GB macOS reserve raises the disk total of every existing fixture with a macOS
pool.
Where the fixture tests something else, its `budget.diskGb` rose by 100 so its
outcome stays; where its expected text quotes the disk line or message, the text
changed:

| Fixture                                                        | Change                                                                                                                                                                   |
| -------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `accepted/axmc-budget-at-limit.json`                           | `budget.diskGb` 190 to 290, so it stays exactly at the limit: `290 of 290 GB, including a 140 GB reserve`                                                                |
| `accepted/axmc-os-labels.json`                                 | `budget.diskGb` 200 to 300; it stays accepted                                                                                                                            |
| `accepted/axmc-shared-label-different-os.json`                 | `budget.diskGb` 200 to 300; it stays accepted                                                                                                                            |
| `rejected/axmc-different-os-shared-label-different-token.json` | `budget.diskGb` 200 to 300, so the token error stays the only error                                                                                                      |
| `rejected/axmc-three-macos-vms.json`                           | `budget.diskGb` 200 to 300, so the VM-limit error stays the only error                                                                                                   |
| `rejected/axmc-budget-errors-come-last.json`                   | Its expected disk message names 1200 GB and the macOS reserve                                                                                                            |
| TOML `accepted/macos-full.toml`                                | `budget.disk_gb` 180 to 280; it stays accepted                                                                                                                           |
| TOML `rejected/macos-budget-totals.toml`                       | Its expected disk messages name the macOS reserve and a total 100 GB higher                                                                                              |
| TOML `rejected/macos-clashes-come-before-budget-totals.toml`   | The first pool's `disk_gb` 40 to 50 and `budget.cpus` 8 to 12, as for `axmc-budget-errors-come-last.json`; its expected disk message names 1200 GB and the macOS reserve |

These fixtures pin the new rules, and each default, minimum, and message has one:

| Fixture                                            | Pins                                                                                             |
| -------------------------------------------------- | ------------------------------------------------------------------------------------------------ |
| `accepted/axmc-macos-defaults.json`                | A pool with only its required fields gets 1 slot, 4 CPUs, 8 GB of memory, and 50 GB of disk      |
| `accepted/axmc-macos-values-replace-defaults.json` | Each value a pool sets replaces its default                                                      |
| `accepted/axmc-macos-minimums.json`                | Each minimum is inclusive: 2 CPUs, 4 GB, 50 GB                                                   |
| `accepted/axmc-macos-two-default-slots.json`       | Two pools without `slots` total two VMs and are accepted                                         |
| `accepted/axmc-macos-diagnostics-false.json`       | `diagnostics: false` is accepted on a macOS pool                                                 |
| `rejected/axmc-macos-three-default-slots.json`     | Three pools without `slots` total three VMs and are rejected                                     |
| `rejected/axmc-macos-cpus-below-minimum.json`      | The `cpus` message                                                                               |
| `rejected/axmc-macos-memory-below-minimum.json`    | The `memoryGb` message                                                                           |
| `rejected/axmc-macos-disk-below-minimum.json`      | The `diskGb` message                                                                             |
| `rejected/axmc-macos-diagnostics.json`             | The `diagnostics` message, with no other error                                                   |
| `rejected/axmc-macos-errors-in-field-order.json`   | The four messages together, in an entry's field order, at the index of a pool after a Linux pool |
| `rejected/axmc-macos-slots-null.json`              | A `null` `slots` is a type error, not a default and not a missing field                          |
| `rejected/axmc-linux-slots-null.json`              | The same for a Linux pool on axmc, where `slots` is optional too                                 |
| `accepted/axmc-linux-default-slots.json`           | A Linux pool without `slots` gets 2 on axmc, beside a macOS pool without it that gets 1          |

### The Runner Record

`RUNNER_RELEASES` in `cli/src/image/runner.rs` holds, for each runner version the
CLI has declared, the release date and the SHA-256 of
`actions-runner-osx-arm64-<version>.tar.gz`, both taken from the
[actions/runner releases](https://github.com/actions/runner/releases) page. The
Linux image's freshness check reads its release date from the image's label, and
a unit test requires the label to equal the record's date. The golden macOS VM's
build takes the digest from the record, and `status` the date.
`DECLARED_RUNNER_VERSION` is the version that both install. One runner bump moves
the Linux image and the macOS golden VM together, and it stays red until its
entry is complete; [Development
Workflow](../releases/runner-update.md#bumping-the-runner) lists the edits.

The tests in `image::runner` fail with a message that names what is missing:

- the declared version must equal the `runner-version` label of the Linux image's
  Dockerfile;
- the declared version must have an entry, which names the date and the archive
  to look up when it has none;
- every entry must have a release date that is a real `YYYY-MM-DD` day and a
  SHA-256 of 64 lowercase hexadecimal digits, and no version may be listed twice.

`MARSHAL_TEST_RUNNER` declares another runner for the tests and for the Mac
verification of a golden macOS VM whose runner is behind. `declared_runner` in
`image::runner` reads it:

- **The format** is `<version>=<sha256>`: a version of three dot-separated
  decimal numbers, each of at most nine digits and none with a leading zero, then
  an equals sign, then 64 lowercase hexadecimal digits. Anything else, such as
  an empty value, a missing part, uppercase digits, whitespace, or a second `=`,
  is refused with a message that names the format and not the value.
- **A debug build** declares that version and digest in place of the record's. The
  record has no date for a version it does not hold, so the runner age of such a
  golden VM shows `unknown`.
- **A release build** ignores the variable, malformed or not, and declares the
  record's entry for `DECLARED_RUNNER_VERSION`, so a release binary cannot be made
  to install a runner the record does not hold. `tests/base_address.rs` shows both
  halves, and CI runs it again in the release profile.

`build` reads the variable for the golden macOS VM's build; `declared_runner`
takes the value from its caller, and the tests pin it.

### The Base Record

`MACOS_BASE` in `cli/src/image/macos_base.rs` holds the macOS base the golden
macOS VM is built from:

| Field           | Holds                                                                                            |
| --------------- | ------------------------------------------------------------------------------------------------ |
| `digest`        | The base's manifest digest; the reference is `ghcr.io/cirruslabs/macos-tahoe-base@<digest>`      |
| `macos_version` | The macOS version the base ships, which the host must be at or above                             |
| `published`     | The day the base was published, from the manifest's `org.cirruslabs.tart.upload-time` annotation |
| `retired`       | The digests of earlier pins, the only bases a build removes from Tart's cache; empty so far      |

The registry tags only `latest`, so the digest is the only fixed reference, and a
base bump records the digest, the version, and the date together. The pinned
base ships macOS 26.6.2 (25G83), as the first Mac check on
[#80](https://github.com/axross/runners/issues/80) read inside a clone of it.

`base_age_warnings` returns the base's age warnings as of a given time, from the
whole days since `published`. Under 30 days there is none, from 30 days the first
and from 60 days the second replaces it, and both thresholds are inclusive, as the
runner's are. Each tells the maintainer to bump the base pin and rebuild the
golden macOS VM, because rebuilding alone does not update macOS. `status` and the
log show them under [macOS Status](#macos-status).

### VM Names

The `macos::name` module writes and reads the names of the macOS VMs Marshal
makes. The job VM form and the Linux forms are M1's, in `runtime::tart::names`,
and the macOS count reads job, Linux golden and Linux build VMs with M1's readers.
Each form is read strictly: the whole name matches, nothing precedes or follows
it, and case matters.

| VM              | Form                                                                                                     |
| --------------- | -------------------------------------------------------------------------------------------------------- |
| Golden macOS VM | `marshal-golden-macos-g<golden VM version>-r<runner version>-<yyyyMMddHHmmss>`                           |
| macOS build VM  | `marshal-build-macos-<yyyyMMddHHmmssfff>`                                                                |
| Job VM, any OS  | `marshal-job-<pool>-<slot>-<yyyyMMddHHmmssfff>`                                                          |
| Linux golden VM | `marshal-golden-linux-g<golden VM version>-c<image contract version>-r<runner version>-<yyyyMMddHHmmss>` |
| Linux build VM  | `marshal-build-linux-<yyyyMMddHHmmssfff>`                                                                |

- **The golden macOS VM version** is `GOLDEN_MACOS_VM_VERSION`, 1 for now. It goes
  up whenever the base digest, the provisioning steps, or the launcher change. In
  a name it is a positive integer without a leading zero, and the runner version
  is three dot-separated numbers, as the runner record writes it.
- **Times** are UTC. A golden VM's name carries its build time to the second and a
  build VM's to the millisecond.
- **The newest golden macOS VM** is the one with the latest build time in its
  name, whatever its golden VM version, so a stale one is found and refused, not
  missed. Names that are not golden macOS VMs' are ignored, so selection never
  returns a Linux golden VM, and the Linux selection never returns a macOS one.
- **A job VM's pool** is everything between `marshal-job-` and the last
  `-<digits>-<17 digits>`, so a pool name may contain hyphens and may end in
  `-<digits>`: `marshal-job-ci-2-1-<stamp>` is slot 1 of pool `ci-2`, and
  `marshal-job-ci-2-<stamp>` is slot 2 of pool `ci`. Pool names are unique per
  host, so a pool name identifies the pool whatever its OS.

`build` writes the golden and build VM names, and the macOS runtime reads them.
`cli/tests/macos_names.rs` pins the forms, the strict reading, the golden VM
selection, and that neither OS's forms or selection match the other's.

### The Two-VM Count

Apple's licence allows two additional macOS VMs on a Mac, so the macOS runtime
counts them before it creates one. `macos::count` is that count as pure logic: it
takes Tart's VM list once (each VM's name and state, running, suspended, or
stopped, where a stopping VM is still running), the configured pools with their
operating systems, and an OS lookup that stands in for `tart get`, and calls no
process itself. The lookup answers macOS, Linux, or unreadable, which covers a
failed call and an answer the reader does not know.

| Counts                                                                                                                                      | Does not count                                                                |
| ------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------- |
| A job VM of a configured macOS pool, or a macOS build VM, by its name form, in any state                                                    | The golden macOS VM while it is stopped                                       |
| A Marshal job VM of a pool that is no longer configured, or whose `os` changed, in any state, when the lookup reports macOS                 | Linux golden and build VMs, by their name forms                               |
| Any other running or suspended VM the lookup reports as macOS, including the golden macOS VM if someone boots it by hand                    | A job VM of a configured Linux pool, unless the lookup reports macOS          |
| Any running or suspended VM whose operating system is unreadable, and a Marshal job VM of a pool that is no longer configured, in any state | Other VMs the lookup reports as Linux, and stopped VMs Marshal did not create |

- **The lookup is asked only what the names leave open**, once for each such VM:
  a Marshal job VM in any state whose pool is not a configured macOS pool, and any
  other running or suspended VM that is not a Linux golden or build VM. A VM the
  name and the configured pools already decide is never asked about.
- **Unreadable counts** because over-counting only makes a job wait, while
  under-counting could exceed the licence. A job VM of an unconfigured pool counts
  in any state when its operating system is unreadable. It is among the leftover
  job VMs that the log and `status` name, so the maintainer can delete it.
- **A job VM of a configured Linux pool is presumed Linux.** Only an explicit
  macOS answer counts it, as a VM left from before the pool's `os` changed. An
  answer of Linux, or an unreadable one, in any state, does not, so a `tart get`
  that fails never blocks macOS jobs because of Linux job VMs. It is not a
  leftover of an unconfigured pool, because `start` deletes a configured pool's
  leftovers.
- **A failed deletion** needs no rule of its own: the VM stays in Tart's list, so
  it counts until a later attempt deletes it.
- **The output** is the VMs that count, each with its reason; the total; whether a
  place is free, below `MAX_MACOS_VMS`; and the counted job VMs of unconfigured
  pools, by name, for the log and `status`, because `start` deletes only the leftovers
  of configured pools.

`macos::place::PlaceLock` is the lock for taking a place, one per instance and
shared by the macOS slots. A slot gives its `hold` the whole "count, then clone"
step, so the lock stays held until the clone is listed and cannot be released
between the two. The second slot then counts after the first's VM exists, and sees
two. A macOS slot counts before each clone, and `build` before it creates the macOS
build VM. `cli/tests/macos_vm_count.rs` runs every row of the table and the race of
slots over a fake Tart.

### The Golden macOS VM Build

`build --profile axmc` builds the Linux golden VM when a Linux pool is configured,
as [the Linux build specification](./build-images.md#the-golden-vm-build) describes, and then the golden macOS VM when a
macOS pool is. The rename commits each golden VM independently. A failure of either build is reported with its message and makes `build` exit 1. A VM already committed stays available. `build::golden_macos` runs the macOS build:

1. It checks Tart and Softnet as `start` does, and refuses a Mac whose macOS is
   older than the base's, with
   `the pinned macOS base is macOS <v>, newer than this Mac's macOS <h>; update the Mac first`.
2. It deletes any leftover macOS build VM; one it cannot delete is a warning. Then
   it counts the macOS VMs, so a leftover it deleted does not count, and refuses a
   full limit with
   `<n> macOS VMs already count against the limit of 2 (<names>), so no macOS build VM was created; stop or delete one of them and build again`.
3. It pulls the base by its pinned digest and clones it to a build VM.
4. It boots the build VM as a job VM boots, with 180 s for the guest agent's first
   answer.
5. Through the guest agent, it removes the base's runner; downloads
   `actions-runner-osx-arm64-<version>.tar.gz` for the declared runner and stops
   unless its SHA-256 equals the runner record's; unpacks it to
   `/Users/admin/actions-runner` and installs the launcher; turns off Remote Login
   and Screen Sharing; and turns off the automatic download and installation of
   macOS updates, leaving the automatic check on. The scripts are in
   `cli/src/golden/scripts/macos/`.
6. It flushes the disk, shuts the VM down and renames it to the golden name.
7. It deletes every other golden macOS VM, and removes from Tart's cache only the
   retired base digests; a failure there is a warning, and the next build tries
   again.

A stop before the rename leaves the previous golden macOS VM in place. If a stop interrupts the rename and Tart cannot list the VMs, check the reported golden name: the new VM may already be active. A stop after the rename skips the remaining cleanup. See [Stop and failure](./build-images.md#stop-and-failure) for the cleanup deadline and how to determine the outcome. The disk keeps the base's layout,
recovery partition included: the first Mac check on
[#80](https://github.com/axross/runners/issues/80) found that the guest agent grows
the disk at boot with it kept. `cli/tests/golden_macos_build.rs` runs the build over
the fake Tart.

### The macOS Job VM

A macOS slot runs one job per VM, on the Tart backend, with M1's slot cycle and
these steps in place of the Linux ones:

1. **A place under the limit.** Under `PlaceLock`, the slot counts the macOS VMs
   and, when a place is free, clones the newest golden macOS VM to the slot's job VM
   name. When two already count, it clones nothing and registers nothing, logs
   `<pool> slot <n>: waiting for the macOS VM limit; <n> count (<names>).` once,
   and counts again every 5 s. A missing golden macOS VM, or one of another golden
   VM version, fails only that slot's attempt, which backs off.
2. **Boot and probe.** It sets the pool's vCPUs, memory and disk, boots the VM
   under its shim, and waits at most 180 s for the guest agent; Linux keeps 120 s.
   It then probes the guest every 3 s for at most 90 s, each probe within 30 s,
   until the disk has grown: the launcher must be present, the runner must report
   the version the golden VM's name records, and `diskutil` must report a disk
   within 1 GB of the pool's `diskGb`, with less than 1 GB outside any partition. A
   failed probe ends the attempt before registration.
3. **The runner.** The JIT configuration goes to the launcher on the guest agent's
   standard input, never on a command line. The launcher runs the runner's own
   `run.sh` as `admin` with the configuration in `ACTIONS_RUNNER_INPUT_JITCONFIG`.
   There is no container, so CPU placement is the VM's vCPUs, and a stop signals
   `Runner.Listener` with `sudo -n pkill -INT`, then `-KILL`.
4. **Deletion.** When the job ends the VM is deleted, as on Linux. A VM the slot
   fails to delete is deleted again every 5 s, before the slot backs off, and it
   counts toward the limit meanwhile.

When `tart run` ends with Tart's VM-limit error, `The number of VMs exceeds the
system limit`, the slot deletes the clone, backs off, and logs
`macOS refused another macOS VM: <error>; if no other macOS VM is running, restart the Mac to release the place`.
Any other boot failure stays a generic boot failure. `start`, a resume and a
`restart` refuse a missing golden macOS VM, or one of another golden VM version,
before any VM boots, with the Linux golden VM's messages: `the golden macOS VM is missing; this CLI needs one with golden VM version <G>`
or `the golden macOS VM <name> has golden VM version <g>, but this CLI needs golden VM version <G>`,
as image refusals: `start` ends with the Linux golden VM's `build` and `start`
advice, and a resume or `restart` stays stopped with its `stop`, `build` and
`start` advice. A later macOS job launch that meets the same refusal fails only that
slot's attempt, which backs off.

### macOS Status

When a macOS pool is configured, `status` adds these lines after the image lines;
with no golden macOS VM, the first three read `missing`:

- `golden-macos-vm: <name>`, or `missing`;
- `golden-macos-vm-version: <n>`, `mismatch (golden VM version <g>, CLI <n>)`, or
  `missing`;
- `golden-macos-runner: <version>, released <d> days ago`,
  `<version>, released unknown` for a version the runner record does not hold, or
  `missing`;
- `macos-base-published: <date>, <d> days ago`;
- one `warning: <text>` line each for a runner released 7 or more days ago, which
  says from 21 days that the runner never updates itself, since GitHub's JIT
  configuration turns that off, and asks for a rebuild before GitHub stops sending
  jobs to the version, a runner whose
  age is unknown, and the base's age from 30 days, as The Base Record says.

The log carries the same warnings at each run of the core, as `Golden macOS VM
freshness: <text>.`. A macOS slot shows `waiting for the macOS VM limit` or
`waiting for VM <name> to be deleted` while it waits, and one `leftover-vm` line
names each counted job VM of a pool that is no longer configured, with
`` delete it with `tart delete <name>` ``, because `start` deletes only the leftovers
of configured pools; the log names each once when a count first finds it.

In the agent's snapshot these are the runner states `waiting_for_vm_limit` and
`waiting_for_vm_deletion`, the runner field `workload_kind: "vm"`, an `images` entry
named `golden macOS VM` with the lines `name`, `golden-vm-version`, `runner-version`
and `base-published` and the warnings `runner_old` with `days`, `runner_age_unknown`
and `base_old` with `days`, and `leftovers` of kind `vm_unconfigured_pool`.
