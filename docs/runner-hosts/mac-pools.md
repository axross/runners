---
type: Procedure
title: "Run pools on the Mac"
description: "Configure repository or organisation pools, check workflow labels, start the host and remove VMs left by a retired pool."
---

# Run pools on the Mac

## Running the Host

### The host configuration

[`hosts/macos/runner-host.example.json`](../../hosts/macos/runner-host.example.json)
is the configuration to copy. It holds placeholders only: replace the owners,
repositories, names and token paths, and keep the files outside any checkout. It is
the JSON form the last command-line release reads.
[`hosts/macos/config.example.toml`](../../hosts/macos/config.example.toml) is the
same configuration in the TOML form the resident agent reads from
`~/Library/Application Support/marshal/config.toml`, as
[The Agent's Configuration](../marshal-cli/resident-agent.md#the-agents-configuration) describes. Each
entry is a pool of `slots` runners for one repository:

- `os` is `linux` or `macos`, and `diskGb` is the per-VM disk cap. A VM has whole
  vCPUs, so `cpus` is an integer from 1 to 64 on every pool, and a Linux pool's
  `diskGb` is at least 20, the golden VM's disk size.
- `labels` replaces the routing label `axmc`. A Linux runner registers with
  `self-hosted`, `linux`, `ARM64` and the routing label, so a workflow routes a job
  with `runs-on: [axmc, linux]`; a macOS runner registers with `self-hosted`,
  `macOS`, `ARM64` and the routing label, for `runs-on: [axmc, macOS]`.
- `imageName`, when present, names the runner image inside the golden VM; without it
  the image is `actions-runner:local`.
- `tokenPath` names a file that holds one repository's token, for the command line;
  the TOML form has none, since the Mac app's agent keeps each token in the login
  keychain, entered in the main window's Tokens tab; see
  [Tokens and a Locked Keychain](../marshal/mac-app.md#tokens-and-a-locked-keychain). Create the token as
  [Windows Runner Host](./registration.md#the-fine-grained-token) describes,
  one fine-grained token per repository with the Administration permission. On macOS
  the file must be a regular file owned by you with mode `0600`: `chmod 600 <file>`.
- `budget` holds the Mac's `cpus`, `memoryGb` and `diskGb` for all pools together.
  The disk total counts every slot's `diskGb`, plus a fixed 40 GB for the golden VM
  and Tart's image cache when any pool is Linux, and a fixed 100 GB for the golden
  macOS VM and its cached base when any pool is macOS. The example's two Linux slots
  of 4 vCPUs, 8 GB and 40 GB and its macOS pool with only the required fields, one
  slot of 4 vCPUs, 8 GB and 50 GB, fit its budget of 12 CPUs, 24 GB and 270 GB
  exactly.
  [CLI](../marshal-cli/json-validation.md#profiles) has every rule, its message, and the migration from the
  earlier rules.

For an organisation-owned repository, the token's resource owner is the
organisation, as [The Fine-Grained Token](./registration.md#the-fine-grained-token) describes. [The Token Store](../marshal-cli/token-store.md#the-token-store) names the token's Keychain item. Unlocking the keychain does not
resolve a refusal from GitHub; check [the token on GitHub](./registration.md#the-fine-grained-token).

The last command-line release's `validate --profile axmc` checks a JSON file on any
machine.

The VMs run beside the macOS system, and Apple's licence allows two macOS guests at
once. `validate` limits macOS pools to two VMs and does not count Linux VMs. The
first Mac check on #80
([its results](https://github.com/axross/runners/issues/80#issuecomment-6816684419))
booted a Linux VM beside two macOS VMs, so Linux VMs do not count against macOS's
limit.

### Organisation Pools

For a pool configured with `organization`, create its token using the [organisation pool token procedure](./registration.md#organisation-pools). The GitHub steps
are the same on either host. On the Mac, enter the token in the organisation's row
under **Organisations** in the **Tokens** tab. The token stays in the login
keychain, separate from the tokens in repository rows. [Tokens and a Locked Keychain](../marshal/mac-app.md#tokens-and-a-locked-keychain) covers a store failure; a GitHub refusal is a
registration failure, not evidence that the keychain is locked.

Removing an organisation's last pool does not remove its token. Before removing it,
choose **Remove Token** on the organisation's row. If the pool is already gone, delete
the keychain item with the account `github.com/<org>`.

### Starting, watching and stopping

Before the first `start` on axmc, confirm that every self-hosted job in each
repository the pools serve names a routing label: `axpc`, or `axmc` with an OS label
such as `linux`, as in `runs-on: [axmc, linux]`. Use the label your pools carry in
place of `axmc` when `labels` replaces it. A job that names neither, such as one that
names only `self-hosted`, can land on axmc. Search each repository's workflows for
`runs-on` and check every hit.

With the last command-line release, the golden VM is built once and the pools are
started from a terminal of the signed-in session, using the [build and start commands in its program tree](https://github.com/axross/runners/blob/dc8e1ebcc0722997206487f3dcca2ae7e519cf02/docs/operations/macos-runner-host.md#starting-watching-and-stopping). `start` validates the configuration, opens its log under
`~/Library/Logs/marshal/`, checks Tart and Softnet, checks the golden VM of each
configured OS, prepares every pool, and runs every slot until it is stopped. It also holds off idle
sleep and watches for a sleep or a network change; see
[Sleep, Wake, and Network Changes](./mac-resume.md#sleep-wake-and-network-changes). It refuses when
the configuration is invalid, when a prerequisite refuses, when a configured OS's golden VM is
missing or mismatched, or when no pool can start. Nothing starts it at sign-in.

Its clients `status`, `restart`, `stop` and `diagnostics` report the state, each pool
and slot, the golden VM and its age, any warning and a stop reason; stop every job,
delete its VM and read the configuration again; stop the instance within 30 s and end
every VM; and finalise the diagnostic VMs still retained.
[CLI](../marshal-cli/foreground-commands.md#running-a-host) gives each one's fields and waits. In the status, a
slot is `registering` while its VM is cloned, booted and probed,
`running container <name>` while it holds a job (`running VM <name>` for a macOS
slot, whose job runs in its VM with no container), `backing off` after a failure, and
`held: <reason>` when a prerequisite refuses its next boot. A macOS slot shows
`waiting for the macOS VM limit` while two macOS VMs count, and
`waiting for VM <name> to be deleted` while Marshal deletes again, every 5 s, a job VM
it failed to delete. A slot that is held boots
nothing and tries again on its backoff, while jobs on other slots finish; fix the
cause, such as Softnet's SUID bit after a `brew upgrade`, and the slot boots again.

A `restart` deletes every job VM, which ends the chance to observe a stuck one. When a
Linux runner keeps retrying its connection to GitHub, or its jobs fail to resolve
names, do not `restart` yet: read
[Observe a Linux job VM whose network overlaps Docker's bridge](./mac-overlap-observation.md)
first. A `restart` after it needs the maintainer's go.

A client's `stop`, a signal, or closing the terminal of `start` runs the same stop
sequence, which stops the job containers and
deletes every VM within 30 s. A second Ctrl+C skips the grace period: a diagnostic VM
is then deleted without its export. When `start` is killed hard,
each VM's shim deletes its VM within 15 s, and the next `start` takes the instance
lock at once and deletes whatever is left. One instance runs per machine: a `start`
while one runs is refused, and a `start` with the same configuration resumes a stopped
instance.

### Health checks

- **GitHub:** each repository's **Settings, Actions, Runners** lists the sum of its
  pools' `slots` runners, **Idle** while no job is queued, with the labels
  `self-hosted`, `linux`, `ARM64` and the routing label.
- **The instance:** the status shows `state: running`, a `pool:` line for every
  entry, `image-contract:` as the CLI's number rather than `mismatch` or `missing`, and
  `golden-vm:` with the golden VM's name. A `warning:` line is a freshness warning, a
  newer patch of Tart or Softnet, or a keep-awake hold that is lost.
- **The machine:** `tart list --source local` shows the golden VM and, for each slot,
  one running `marshal-job-<pool>-<slot>-<time>` VM, which is an idle runner or one
  with a job. A VM left after a stop is a leftover.
- **The log:** `~/Library/Logs/marshal/marshal-<time>-<pid>.log` keeps the newest ten
  runs. `Using Tart <v> and Softnet <v>.` records the versions in use.
- **A stuck Linux runner:** a runner that keeps retrying its connection, or jobs on it
  that fail to resolve names, is not yet a case for a `restart`, which deletes the VM
  that holds the evidence. Follow
  [Observe a Linux job VM whose network overlaps Docker's bridge](./mac-overlap-observation.md)
  first.

### Adding Actions: read for Job Names

The Mac app names the job each busy runner runs only when the repository's token also
has **Actions**, **Read-only**. Add it as
[Windows Runner Host](./registration.md#adding-actions-read-for-job-names)
describes; the steps are on GitHub and the same on both hosts, and the token's value
does not change, so nothing changes in the login keychain.

## Removing the VMs of a Pool No Longer Configured

The runtime deletes leftover job VMs only for pools that are configured, so a VM of a
pool that was removed from the configuration stays. Cleanup at `start` matches a job
VM by the exact name form `marshal-job-<pool>-<slot>-<yyyyMMddHHmmssfff>` and a
configured pool's name, and it never touches a golden VM, a build VM or any other VM.
List the local VMs, and delete a removed pool's job VMs by name with Tart:

```bash
tart list --source local
tart delete marshal-job-<pool>-<slot>-<time>
```

A job VM of a removed macOS pool also counts toward the two macOS VMs, so
the status names it on a `leftover-vm` line, and the log names it once, with
the `tart delete` command. Delete only VMs whose names begin `marshal-job-` followed
by the removed pool's name and a slot number, and none that is running; a running one is a VM of an instance
that still uses that name, so stop the instance first. Never delete the golden VM by
hand while the runtime runs; `build` replaces it.
