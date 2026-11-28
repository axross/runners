# Windows with Docker Desktop

The host configuration for running ephemeral GitHub Actions runners for several
repositories or organisations from one Windows machine with Docker Desktop.
Marshal's agent reads a machine-local host configuration and keeps each pool's slots
filled with single-job containers built from
[`images/actions-runner/`](../../images/actions-runner/README.md).

[Windows Runner Host](../../docs/runner-hosts/index.md#windows-with-docker-desktop) owns the
operator procedure for the axpc host: prerequisites, installing Marshal, tokens,
health checks, recovery, and rolling back. [CLI](../../docs/marshal-cli/index.md) owns
how a configuration is read and the rules it must meet. This
directory holds `runner-host.example.json`, an example host configuration with two
repositories using obviously fake names; `config.example.toml`, the same
configuration as the agent reads it; and this reference for every field of the
configuration.

## Host configuration

The JSON form, `config.json`, which Marshal migrates to `config.toml` at its first
start, and which the last command-line release read from the path its `--config`
named. Each field below has a TOML key in `config.toml`, as the next section says.

| Field                                 | Meaning                                                                                                                                                                                                        |
| ------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `imageName`                           | The local image tag every entry uses; Marshal's Build image builds it from the image definition embedded in the app                                                                                            |
| `repositories[].owner`, `repository`  | The target repository. Multiple entries can form separate execution pools                                                                                                                                      |
| `repositories[].slots`                | How many jobs run at once in this entry's pool, 1 to 16; set 1 for a dedicated serialized pool                                                                                                                 |
| `repositories[].tokenPath`            | An absolute Windows path (drive letter or UNC) to a token file. Read only by the last command-line release; Marshal ignores it, reads the token from the Credential Manager, and the TOML form has no such key |
| `repositories[].labels`               | Non-platform registration labels; absent or empty defaults to `axpc`, a non-empty list replaces it                                                                                                             |
| `repositories[].volumes`              | The cache volumes as `suffix` and `mountPath` pairs, possibly none                                                                                                                                             |
| `repositories[].name`                 | Starts the entry's container, runner, and volume names. Lowercase letters, digits, and hyphens, at most 64 characters, not colliding with another entry's                                                      |
| `repositories[].cpus`                 | Optional CPU-time quota of each job container, a number above 0 and at most 64; 2 when absent. Automatic affinity selects ceiling(`cpus`) available CPUs                                                       |
| `repositories[].memoryGb`             | Optional memory limit of each job container in whole gigabytes, 1 to 256; 8 when absent                                                                                                                        |
| `repositories[].diagnostics`          | Optional boolean, false by default; enables the diagnostic runner lifecycle for this entry only                                                                                                                |
| `repositories[].diagnosticDirectory`  | Required with diagnostics, invalid without them; absolute local Windows directory, outside the checkout, pre-created and private to the operator                                                               |
| `repositories[].diagnosticRawRecords` | Optional boolean, false by default; separate opt-in for private daemon/crash records, invalid when true without diagnostics                                                                                    |

Marshal does not use an invalid configuration: its agent stops, or keeps the last
valid configuration, before it touches Docker, and the window lists one line per
problem naming the field. A field not listed above is rejected as unknown, not
ignored, so a misspelled `label` cannot silently drop `labels`. The isolation rules the validation enforces, such
as what a registration carries and which names collide, are in the Per-Repository Isolation on a Runner Host
section of [Security](../../docs/security/pool-isolation.md#per-repository-isolation-on-a-runner-host).

Non-empty `labels` lists no longer add to `axpc`. Before rollout, add `axpc`
explicitly to an existing general entry's custom-label list to preserve its
registration, as the example does. Missing or empty lists keep the default.
The image, resource limits, slots, mounts, and diagnostic settings do not change.

## The agent's configuration

The resident agent does not take `--config`. It reads `config.toml` in
`%LOCALAPPDATA%\marshal\`, the TOML form of the same configuration, with
snake_case keys, `repository = "owner/name"` in place of `owner` and `repository`,
`runners` in place of `slots`, a required `routing_label`, and no `tokenPath`: it
reads each pool's token from the Credential Manager.

A TOML pool can instead select `organization` with a required `runner_group`,
without `repository`. JSON remains repository-only. Organisation and repository
pools use separate tokens. [CLI](../../docs/marshal-cli/index.md) owns the target
exclusivity, forbidden settings, and routing-label rules;
[Windows Runner Host](../../docs/runner-hosts/index.md#windows-with-docker-desktop) owns the
organisation token steps;
[Security](../../docs/security/pool-isolation.md#organisation-pools-share-a-runner-groups-trust-boundary)
states the runner-group and private-repository fork-policy requirements.

[`config.example.toml`](./config.example.toml) is the example above in that form.
When only `config.json` is in that folder, the agent's first start migrates it to
`config.toml` and leaves `config.json` in place. The keys, the problems it reports,
and the migration are in [CLI](../../docs/marshal-cli/toml-configuration.md#the-toml-configuration).
Its optional `[lifecycle]` table, shown commented out in the example with the
Windows defaults, says whether the agent registers at sign-in, starts the runners at
launch, keeps Windows awake while runners run, stops them on a network change, and
starts them again after a sleep or network stop;
[The Agent's Signals](../../docs/marshal-cli/agent-lifecycle.md#the-agents-signals) describes
each one.

A pool may also set the optional `scale_set` key, which makes it on demand: the
agent registers no runner and starts no container for it until GitHub offers a job to
that scale-set name, and workflows put the name in `runs-on`. The key's rules are in
[TOML configuration](../../docs/marshal-cli/toml-configuration.md#schema-version-1),
what the agent does with it is in
[On-Demand Pools](../../docs/marshal-cli/on-demand-pools.md), and the operator's
steps are in
[On-Demand Pools in the Resident Agent](../../docs/runner-hosts/windows-pools.md#on-demand-pools-in-the-resident-agent).

## Checking a configuration

Marshal checks `config.toml` at its start, at every Start all runners and at every
Reload config.toml or Apply of the window's configuration editor, without
calling Docker or GitHub, and its window names every problem, as
[The Host Configuration](../../docs/runner-hosts/windows-pools.md#the-host-configuration)
describes. The library's plan of an accepted file holds each repository's
registration labels, name, container name pattern, CPU and memory limits, and volume
names, plus the global image.
Quota and planned affinity cardinality are separate; actual CPU IDs require
runtime discovery. There is no CPU-ID, workload-role, or affinity opt-in field.
Every ordinary and diagnostic job has affinity, distributed across measured
Docker-visible IDs in entry/slot order. Slot counts and consumer build flags do
not change. Affinity reserves nothing and guarantees neither compiler worker
counts nor an OOM remedy. Allocation, enforcement, and separately authorized
Desktop/WSL verification are in
[Windows Runner Host](../../docs/runner-hosts/windows-pools.md#automatic-cpu-affinity).
It also holds the two diagnostic opt-ins, not the private evidence path.
Validation checks types and normalized Windows checkout exclusion without opening
the sink. Runtime privacy, access or quota failures warn and do not prevent a
runner starting. A diagnostic-marked stale container after disabling is different:
startup retains it and refuses that entry until explicit private recovery/cleanup.
The opt-in procedure and retention limits are in
[Windows Runner Host](../../docs/runner-hosts/index.md#windows-with-docker-desktop).

## Fixtures

The fixtures live in [`tests/host-configuration/`](../../tests/host-configuration)
at the repository root, and one `expectations.json` there records the outcome and the
strings each must give. The CLI's tests read them, and
[CLI](../../docs/marshal-cli/configuration-fixtures.md#fixtures-and-tests) describes the set. Each
fixture in `shared/rejected/` breaks one rule and must be rejected with a message
naming its field; each in `shared/accepted/` sits on the edge of a rule (no volumes,
a UNC token path, the maximum slot count, the limit bounds) and must be accepted;
the example must be accepted with distinct names and volume names. The split-pool
fixture checks label defaults and replacement, isolated same-repository storage, a
one-slot dedicated pool, and rejection of cross-route labels or incorrect token
sharing. The fixture tests never call Docker or GitHub.
