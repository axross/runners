---
type: Procedure
title: "Configure Windows runner pools"
description: "Set pool resources and routing, migrate a configuration and identify which cache volumes a repository may reuse."
---

# Configure Windows runner pools

## The Host Configuration

Marshal reads its configuration from `%LOCALAPPDATA%\marshal\config.toml`, which its
first start migrates from a `config.json` in that folder, the command line's JSON
form. Start from
[`config.example.toml`](../../hosts/windows-docker-desktop/config.example.toml), and
see the Host configuration section of the
[host README](../../hosts/windows-docker-desktop/README.md) for every field, which
names each by its JSON key, and [Schema Version 1](../marshal-cli/toml-configuration.md#schema-version-1) for
the TOML key each JSON key becomes, such as `runners` for `slots` and `memory_gb` for
`memoryGb`; [The Agent's Configuration](../marshal-cli/resident-agent.md#the-agents-configuration) says how
the agent loads it. Its `[lifecycle]` table sets what the agent does around sign-in,
sleep and the network: with no table it does not register at sign-in, starts the
runners at launch, keeps Windows from idle sleep while runners run, stops every
runner on a sleep or a primary-network change, and does not start them again on its
own; [The Agent's Signals](../marshal-cli/agent-lifecycle.md#the-agents-signals) gives each setting, the Run
key value, and Windows' Startup apps. The rules below name the JSON keys. The rules
that matter operationally:

- **One entry per execution pool**, each with its own `name`, `slots`,
  `labels`, and `volumes`. Every entry uses the global `imageName`. The TOML form
  has no token key: the pool's token is the repository's entry in the Credential
  Manager.
- **Unknown fields are rejected, not ignored.** A misspelled field such as `label`
  for `labels` fails validation naming the field, so a typo cannot silently drop
  a setting.
- **Labels select the pool.** Missing or empty `labels` default to `axpc`;
  a non-empty list replaces that default while retaining `self-hosted`, `linux`,
  and `x64`. Before rollout, add `axpc` to each existing non-empty general
  custom-label list that lacks it; otherwise ordinary `axpc` jobs no longer
  match that entry. Label and token isolation rules are in the
  Per-Repository Isolation on a Runner Host
  section of [Security](../security/pool-isolation.md#per-repository-isolation-on-a-runner-host).
- **Every entry has a `name`** that starts its container and runner names,
  `<name>-<index>-<timestamp>` with a 1-based slot index, and its volume names,
  `<name>-<suffix>`. It is lowercase letters, digits, and hyphens, starting with
  a letter or digit, and at most 64 characters, because it starts every runner
  name. GitHub documents no limit for runner names; 64 is this project's
  assumption. Choose it short and recognisable. A name that collides with another
  entry's is rejected, and the error names both. The collision rule is in the
  Per-Repository Isolation on a Runner Host section of
  [Security](../security/pool-isolation.md#per-repository-isolation-on-a-runner-host).
- **One configuration per machine is the supported setup**, because the instance
  lock is machine-wide: one Marshal agent runs the pools on the machine, whichever
  user runs it.
- **Every job container has CPU and memory limits.** `cpus` is a number above 0
  and at most 64, 2 when absent. `memoryGb` is an integer from 1 to 256, 8 when
  absent. Each job container of the entry starts with `--cpus` and `--memory`
  set to them and `--memory-swap` equal to `--memory`, so it gets no swap beyond
  its memory. Docker takes `--cpus` as a CPU-time quota and `--memory` as
  a size with a unit suffix such as `g`; see
  [Docker's resource constraints](https://docs.docker.com/engine/containers/resource_constraints/).
  The limit applies to each container, not to an entry's slots together or to the
  host. The short container that resets volume ownership at startup has none.

Marshal checks the file when it starts, at every **Start all runners** and at every
**Reload config.toml** or Apply of the window's configuration editor, and calls
neither Docker nor GitHub to do it. A file that breaks a rule is not used: the window
names every problem, by its key and line, in its stopped banner when Marshal has no
configuration in use, and in a footer warning beside the last valid configuration
when it has one, which it keeps running on, as
[The Configuration File](../marshal/configuration.md#the-configuration-file) describes. Marshal does not
watch the file, so after an edit by hand take the one step the window names in its
footer warning, its Settings tab and its stopped banner, and read the window: while
the runners are stopped, **Start all runners**; while they run, **Reload config.toml**
in the window's footer, which lists each pool's effect and warns before a change stops
a running job, or **Stop all runners** then **Start all runners** when the window
offers no Reload; and, while a migration of `config.json` is pending or was rejected,
quit Marshal and launch it again. A Reload or an applied edit in the window also puts
the file's `[lifecycle]` settings in effect at once, the Run key and the keep-awake,
network and resume handling included. It does not check that a pool has a token; the
Tokens tab shows which have none.

### Split a repository into general and Android pools

Keep the general entry's existing name, slots, resource limits, labels, mounts,
and diagnostic settings. If its `labels` list is non-empty, explicitly include
`axpc` alongside its existing aliases to preserve the effective registration.
Leave the global image unchanged. Add an entry for the same repository with a
distinct non-nested name, a dedicated label in `labels`,
and `runners = 1`. The two pools share the repository's token. For example, the
added pool can be:

```toml
[[pool]]
name = "example-android"
repository = "example-owner/example-repo"
labels = ["example-android-route"]
runners = 1
```

Use operator-local values, not these example names. Neither the image name nor
the entry name identifies a workload to Marshal. Set resource limits
only from approved host evidence; a dedicated slot does not reserve host memory.
No shared SDK, workspace, or new persistent cache is needed. Existing storage
choices follow the [shared-storage rules in Security](../security/pool-isolation.md#shared-runner-storage-is-a-cache-poisoning-surface).
Diagnostics remain independent per entry, with private
retention keyed by its name even if two entries use the same private sink.

Deployment and workflow execution require separate scoped authorization:

1. Record the existing configuration, the window's pools, actual slots, image IDs
   and tagged rollback images. Do not rebuild the image during this change.
2. Establish a quiet maintenance window: finish affected jobs, including old
   workflow revisions still using the general label. Do not restart a busy
   instance; **Stop all runners** stops every entry's containers, not just Android's.
   Do not restart Docker or WSL for this change.
3. Apply the split configuration with **Stop all runners**, then
   **Start all runners**, only under the approved host-operation scope, and confirm
   that the window names no configuration problem. Verify the
   actual registrations: one Android slot, no `axpc` on Android, no dedicated
   label on general runners, and no extra dedicated-label registrations on
   this or another host. If runner-inventory access is unavailable, obtain
   operator evidence instead of expanding permissions. Inspect immutable image
   identities and enforced limits; a source test is not this readback.
   Routing alone needs no image rebuild; rebuilding remains a separately
   authorized operation.
4. Enable consumer routing only after the pool is verified. Change only the
   selected jobs' `runs-on` under that repository's approved plan. A missing
   dedicated runner leaves jobs queued; do not fall back to general runners.
5. With permission for named non-publishing executions, queue two Android
   requests in distinct existing concurrency groups. Verify that their actual
   execution intervals do not overlap and their existing artifact/alignment
   checks pass. Run general CI alongside Android on its unchanged slots;
   where at least two slots exist, verify two representative general jobs
   overlap. Do not change cancellation rules or execute release publication
   as a test. Stop after a failure and retain evidence rather than retrying
   unboundedly.
6. Record queue/build durations, effective labels/images/limits, actual host
   concurrency, diagnostic gaps and rollback in the approved evidence
   destination after human sanitization. Serialization success does not prove
   OOM, and missing counters are not zero. Host pressure can still affect one
   Android build beside general CI.
7. After the validation window, resume the routine under
   [Rebuilding the Image](./windows-image.md#rebuilding-the-image-for-operating-system-updates).

For rollback, first finish affected jobs and obtain the required operation
scope. In a quiet window, choose **Stop all runners**. If the runners do not stop,
keep rollback pending rather than force an unsafe stop. Restore the saved
host configuration/images, keeping the explicit `axpc` migration in restored
general custom-label lists, because Marshal reads a non-empty list as a
replacement for `axpc`.
Restore consumer routing under its separate authorization while the runners
remain stopped, then choose **Start all runners**, confirm that the window names no
configuration problem, and read back actual
registrations, labels, slot counts and image IDs. Check that no old dedicated
job or extra dedicated-label registration bypasses the restored routing.
Preserve general capacity and private evidence; remove
only identified obsolete pool registrations/containers, never prune unrelated
storage. Do not rebuild until the intended image is restored. No host change is
authorized by this procedure alone.

## Automatic CPU Affinity

Every ordinary and diagnostic job container receives `--cpuset-cpus` alongside
its unchanged `--cpus` quota. This changes ordinary execution placement without
adding a JSON field or an opt-in. A quota of 1.5 keeps quota 1.5 and selects two
allowed logical CPUs: affinity cardinality is ceiling(`cpus`), an allocation
policy, not an equivalence between CPU time and CPU IDs. Quota alone does not
restrict scheduling to a particular set.

The configuration reader enumerates entries in file order and slots in index
order. Each slot advances a cumulative position by its cardinality. Before
every registration attempt, a short probe reads its own `Cpus_allowed_list` on
the slot's effective local image and Docker context. The probe has no CPU set,
network, registration, mounts, or image pull. The slot chooses consecutive IDs
from that sorted measured inventory, starting at position modulo inventory size
and wrapping. Sparse IDs are preserved, not replaced with a range from a host
count. With a stable common inventory, selections are disjoint when their total
cardinality fits and can overlap when overcommitted. Topology changes between
observations can also create overlap. No CPU is reserved or exclusive.

Discovery waits at most 30 seconds, and removing a leftover probe at most 5
seconds; [CLI](../marshal-cli/pool-runtime.md#cpu-placement) gives the exact budgets. Each slot uses one
stable probe name, `<name>-cpu-probe-<index>`, and immutable marker
`runners.cpu-probe=1`. A retry or restart removes that exact marked probe before
creating another. Malformed/empty output, insufficient capacity, timeout, or
unconfirmed cleanup fails the attempt before JIT registration. An unavailable
daemon can prevent confirmed deletion; the deadline bounds waiting, not guaranteed
daemon cleanup. No broad prune or job-container deletion is used for probes.

Docker enforces the requested cpuset; before the runner or diagnostic
launcher starts, a Bash guard compares its kernel `Cpus_allowed_list` with the
selected canonical set. Missing/unreadable/mismatched readback exits 125 without
launching the runner or falling back unrestricted. Per-attempt discovery and
this check protect against stale topology after Docker/WSL changes. The short
volume-ownership helper has no new CPU policy. Labels, slots, images, memory,
swap, mounts, consumer tools/build flags, and diagnostic opt-ins remain their
own controls. Affinity does not guarantee compiler process counts, lower memory
use, or an OOM remedy; the Android failure cause remains unconfirmed.

The image smoke checks actual Docker `HostConfig`, parent/child process sets,
effective cgroup cpuset and quota, quota-only controls, distributed selections,
and refusal controls. `nproc` and `getconf` are recorded as separate processor
API observations, not worker counts. Missing topology/layout coverage fails as
incomplete evidence. Hosted Linux enforcement is not Docker Desktop/WSL proof.
The real-host check is a separately authorized post-merge follow-up on
[the affinity issue](https://github.com/axross/runners/issues/25), not fulfilled
by source tests. Before an authorized rollout, save private source/configuration
and image rollback material, establish a quiet maintenance window, and verify
all entries' requested/process/cgroup sets, quotas, diagnostics, capacity, and
topology-change behavior on the intended context. Publish only sanitized source
and image identities, settings/readbacks, gaps, and observed throughput/pressure
there. Do not publish tokens, full inspect output, or raw private diagnostics.

There is no configuration switch to disable affinity, and every release applies
it, so there is no supported way back to placement without affinity. The
PowerShell scripts that once offered one remain only in this repository's history,
unsupported, and a fault in affinity is fixed forward in a new release. The feature
changes no WSL allocation and no Windows process-affinity setting. A failed runtime
readback blocks rollout, not independent pool work.

## Cache Volumes

An entry's `volumes` are named Docker volumes that outlive the throwaway
containers, so a job finds the toolchains and downloads an earlier job left. That
makes each a shared writable surface, and [the shared storage policy](../security/pool-isolation.md#shared-runner-storage-is-a-cache-poisoning-surface)
owns the rules that govern it. Each is declared here with the reason it exists.
The suffixes are those of
[`config.example.toml`](../../hosts/windows-docker-desktop/config.example.toml);
the volume's name is the entry's `name` plus the suffix.

| Suffix      | Mount path             | Why it exists                                                                          |
| ----------- | ---------------------- | -------------------------------------------------------------------------------------- |
| `toolcache` | `/opt/hostedtoolcache` | Language runtimes that the `setup-*` actions install and `RUNNER_TOOL_CACHE` points at |
| `cargo`     | `/home/runner/.cargo`  | Cargo's registry downloads and the binaries it installs                                |
| `rustup`    | `/home/runner/.rustup` | Rust toolchains                                                                        |
| `npm`       | `/home/runner/.npm`    | npm's content-addressed package cache                                                  |

Any job routed to the entry's labels can write to every one of them, and no
other entry mounts them. When the agent prepares a pool, it creates the entry's
volumes and resets their ownership with a short root container that mounts only
those volumes.

An entry keeps only the volumes in the table above. The host does not enforce
that: it accepts any suffix and mount path, so the operator does. A volume
for anything else, such as a compiler or build-system cache whose contents a later
job links or executes without re-verifying them, is unsupported because no rule in
[the shared storage policy](../security/pool-isolation.md#shared-runner-storage-is-a-cache-poisoning-surface) allows it.
The Android SDK and `~/.android` MUST stay container-local, never cache-volume
mount points: a job must not replace the image's verified NDKs or leave SDK
metadata for a later job to trust.

Before listing volumes for a repository, check which of its workflows hold a
deployment secret, and whether the repository accepts pull requests from forks.
If either applies, the pool lists no `[[pool.volumes]]`, so each job starts from the
image alone, or those workflows, including the fork pull request workflows, run on
a GitHub-hosted runner. That includes `npm` and every other volume in the table,
not only the toolchain volumes. The host cannot check this; the operator does. The
rule and its reason are in the Shared Runner Storage Is a Cache-Poisoning Surface
section of [Security](../security/pool-isolation.md#shared-runner-storage-is-a-cache-poisoning-surface).

## Moving to the New Configuration Format

A configuration that still has `hostPrefix` or a per-entry `prefix` is rejected,
naming the field, including the `config.json` Marshal migrates at its first start.
To move it:

1. Note the `hostPrefix`, and each entry's prefix, `<hostPrefix>-<owner>-<repository>`
   in lowercase unless the entry set `prefix`. Quit Marshal if it runs.
2. Rewrite the file: give every entry a `name`, and delete `hostPrefix` and every
   `prefix`. An entry whose `name` equals its previous prefix, which is possible
   only when the prefix is a valid `name`, keeps its volumes; any other `name`
   starts from empty volumes.
3. Remove what the earlier names left, skipping an entry whose `name` equals its
   previous prefix. Remove a volume only once no container uses it:

   ```powershell
   docker ps -a --filter "name=<previous prefix>-"
   docker rm -f <container name>
   docker volume rm <previous prefix>-<suffix>
   ```

4. Continue from [Installing Marshal](./windows-setup.md#installing-marshal) on a host without it, or
   start Marshal on one that has it, and confirm that the window names no
   configuration problem.

Job containers had no CPU or memory limit before. Without `cpus` and `memoryGb`,
each is now capped at 2 CPUs and 8 GB, so set both on an entry whose jobs need
more.

## Removing an Entry

Remove the pool from the configuration, then choose **Stop all runners** and
**Start all runners**, and confirm that the window names no configuration problem. A volume outlives its entry: the entry's volumes and any stopped
containers stay until removed by hand with `docker volume rm` and `docker rm`. The
isolation rules the host enforces between repositories are in
[Per-Repository Isolation on a Runner Host](../security/pool-isolation.md#per-repository-isolation-on-a-runner-host).
