# Windows with Docker Desktop

Scripts that run ephemeral GitHub Actions runners for several repositories from
one Windows machine with Docker Desktop. A supervisor reads a machine-local host
configuration, and for each target repository keeps its slots filled with
single-job containers built from
[`images/actions-runner/`](../../images/actions-runner/README.md).
[Windows Runner Host](../../docs/operations/windows-runner-host.md) owns the
operator procedure: prerequisites, tokens, scheduled tasks, health checks, and
recovery.

| File                              | Purpose                                                                                                     |
| --------------------------------- | ----------------------------------------------------------------------------------------------------------- |
| `supervisor.ps1`                  | Runs every repository's slots; `-ValidateOnly` checks a configuration and prints its plan                   |
| `rebuild-image.ps1`               | Builds the image from this checkout under the configured image name                                         |
| `register-scheduled-tasks.ps1`    | Registers the supervisor and weekly rebuild tasks and restricts the token files                             |
| `runner-host.example.json`        | An example host configuration with two repositories, using obviously fake names                             |
| `host-configuration.ps1`          | Reads and validates a configuration, and builds a slot job's arguments from it; shared by the scripts above |
| `docker-commands.ps1`             | Runs the Docker client and builds job container arguments; shared by the supervisor and its slot jobs       |
| `diagnostic-export.ps1`           | Exports bounded private evidence before diagnostic-container removal, without changing runner outcomes      |
| `diagnostic-process-lifetime.ps1` | Initializes the Windows Job Object lifetime boundary before enabled container work                          |
| `export-runner-diagnostics.ps1`   | Private export worker; isolates storage I/O so the supervisor can enforce its deadline                      |
| `tests/`                          | The validation test and its rejected and accepted fixtures, run by `mise run test:host`                     |

The scripts target Windows PowerShell 5.1, which the scheduled tasks use, and are
written to run on PowerShell 7 as well. CI runs the configuration test under both;
the rest is unverified on 5.1 until the check on a real host, which
[Windows Runner Host](../../docs/operations/windows-runner-host.md) describes.
Their files are ASCII-only.

## Host configuration

A JSON file kept outside the repository; every script takes its path as
`-ConfigPath`. Copy `runner-host.example.json` and replace every value.

| Field                                 | Meaning                                                                                                                                                                               |
| ------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `imageName`                           | The local image tag every entry uses; `rebuild-image.ps1` builds it from the maintained Dockerfile                                                                                    |
| `repositories[].owner`, `repository`  | The target repository. Multiple entries form separate pools with distinct routing labels and names, using the same token file                                                         |
| `repositories[].slots`                | How many jobs run at once in this entry's pool, 1 to 16; set 1 for a dedicated serialized pool                                                                                        |
| `repositories[].tokenPath`            | An absolute Windows path (drive letter or UNC) to the file holding this repository's token, re-read on every registration                                                             |
| `repositories[].labels`               | Non-platform registration labels; absent or empty defaults to `axpc`, a non-empty list replaces it. Must not include `self-hosted`, `linux`, or `x64`, or overlap within a repository |
| `repositories[].volumes`              | The cache volumes as `suffix` and `mountPath` pairs, possibly none                                                                                                                    |
| `repositories[].name`                 | Starts the entry's container, runner, and volume names. Lowercase letters, digits, and hyphens, at most 64 characters, not colliding with another entry's                             |
| `repositories[].cpus`                 | Optional CPU limit of each job container, a number above 0 and at most 64; 2 when absent                                                                                              |
| `repositories[].memoryGb`             | Optional memory limit of each job container in whole gigabytes, 1 to 256; 8 when absent                                                                                               |
| `repositories[].diagnostics`          | Optional boolean, false by default; enables the diagnostic runner lifecycle for this entry only                                                                                       |
| `repositories[].diagnosticDirectory`  | Required with diagnostics, invalid without them; absolute local Windows directory, outside the checkout, pre-created and private to the operator                                      |
| `repositories[].diagnosticRawRecords` | Optional boolean, false by default; separate opt-in for private daemon/crash records, invalid when true without diagnostics                                                           |

An invalid configuration stops the script before it touches Docker, with one line
per problem naming the field. A field not listed above is rejected as unknown, not
ignored, so a misspelled `label` cannot silently drop `labels`. The isolation rules the validation enforces, such
as what a registration carries and which names collide, are in the Per-Repository Isolation on a Runner Host
section of [Security](../../docs/conventions/security.md).

Non-empty `labels` lists no longer add to `axpc`. Before rollout, add `axpc`
explicitly to an existing general entry's custom-label list to preserve its
registration, as the example does. Missing or empty lists keep the default.
The image, resource limits, slots, mounts, and diagnostic settings do not change.

## Commands

From an elevated PowerShell prompt for the registration, and any prompt for the
rest. Before the first command, follow the Allowing the Scripts to Run section of
[Windows Runner Host](../../docs/operations/windows-runner-host.md).

```powershell
.\supervisor.ps1 -ConfigPath C:\path\to\runner-host.json -ValidateOnly
.\rebuild-image.ps1 -ConfigPath C:\path\to\runner-host.json
.\register-scheduled-tasks.ps1 -ConfigPath C:\path\to\runner-host.json
.\supervisor.ps1 -ConfigPath C:\path\to\runner-host.json
```

`-ValidateOnly` prints each repository's registration labels, name, container
name pattern, CPU and memory limits, and volume names, plus the global image, without calling Docker or
GitHub, and exits 1 on an invalid configuration.
It also prints the two diagnostic opt-ins, not the private evidence path.
Validation checks types and normalized Windows checkout exclusion without opening
the sink. Runtime privacy, access or quota failures warn and do not prevent a
runner starting. A diagnostic-marked stale container after disabling is different:
startup retains it and refuses that entry until explicit private recovery/cleanup.
The opt-in procedure and retention limits are in
[Windows Runner Host](../../docs/operations/windows-runner-host.md).

## Tests

```bash
mise run test:host
```

runs [`tests/test-host-configuration.ps1`](./tests/test-host-configuration.ps1).
Each fixture in `tests/fixtures/` breaks one rule and must be rejected with a
message naming its field; each in `tests/accepted/` sits on the edge of a rule
(no volumes, a UNC token path, the maximum slot count, the limit bounds) and must
be accepted; the example must be accepted with distinct names and volume names.
The split-pool fixture checks label defaults and replacement, isolated same-repository
storage, a one-slot dedicated pool, and rejection of cross-route labels or
incorrect token sharing. The test also builds a job container's `docker run` arguments, checks the limit
flags, and checks that a slot job's positional arguments line up with its worker
script block's parameters. It also checks the pattern that recognizes an entry's
stale containers. It never calls Docker or GitHub. CI also runs it under Windows
PowerShell 5.1 on a GitHub-hosted Windows runner.
It includes [`tests/test-diagnostics.ps1`](./tests/test-diagnostics.ps1), covering
per-entry opt-in, tar path/type/size rejection, private sinks, partial-bundle
quotas, stuck stop subprocesses, immutable rollback markers, queued deadlines,
single-owner completion, runner retry/result preservation, exporter descendants
and Windows owner-death containment. Real image
export is exercised by the image's existing smoke command, which also needs
PowerShell 7 on its Linux test host; Docker Desktop remains a separate check.
