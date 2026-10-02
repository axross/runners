# Windows with Docker Desktop

Scripts that run ephemeral GitHub Actions runners for several repositories from
one Windows machine with Docker Desktop. A supervisor reads a machine-local host
configuration, and for each target repository keeps its slots filled with
single-job containers built from
[`images/actions-runner/`](../../images/actions-runner/README.md).
[Windows Runner Host](../../docs/operations/windows-runner-host.md) owns the
operator procedure: prerequisites, tokens, scheduled tasks, health checks, and
recovery.

| File                           | Purpose                                                                                                             |
| ------------------------------ | ------------------------------------------------------------------------------------------------------------------- |
| `supervisor.ps1`               | Runs every repository's slots; `-ValidateOnly` checks a configuration and prints its plan                           |
| `rebuild-image.ps1`            | Builds the image from this checkout under the configured image name                                                 |
| `register-scheduled-tasks.ps1` | Registers the supervisor and weekly rebuild tasks and restricts the token files                                     |
| `runner-host.example.json`     | An example host configuration with two repositories, using obviously fake names                                     |
| `host-configuration.ps1`       | Reads and validates a configuration; shared by the scripts above                                                    |
| `docker-commands.ps1`          | Runs the Docker client, and builds a job container's name and arguments; shared by the supervisor and its slot jobs |
| `tests/`                       | The validation test and its rejected and accepted fixtures, run by `mise run test:host`                             |

The scripts target Windows PowerShell 5.1, which the scheduled tasks use, and are
written to run on PowerShell 7 as well. CI runs the configuration test under both;
the rest is unverified on 5.1 until the check on a real host, which
[Windows Runner Host](../../docs/operations/windows-runner-host.md) describes.
Their files are ASCII-only.

## Host configuration

A JSON file kept outside the repository; every script takes its path as
`-ConfigPath`. Copy `runner-host.example.json` and replace every value.

| Field                                | Meaning                                                                                                                                      |
| ------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------- |
| `imageName`                          | The local image tag `rebuild-image.ps1` builds and the supervisor runs                                                                       |
| `repositories[].owner`, `repository` | The target repository. Each owner and repository pair appears once                                                                           |
| `repositories[].slots`               | How many jobs run at once for this repository, 1 to 16                                                                                       |
| `repositories[].tokenPath`           | An absolute Windows path (drive letter or UNC) to the file holding this repository's token, re-read on every registration                    |
| `repositories[].labels`              | Custom labels, at least one; the labels every registration already carries are not listed                                                    |
| `repositories[].volumes`             | The cache volumes as `suffix` and `mountPath` pairs, possibly none                                                                           |
| `repositories[].name`                | Starts the entry's container, runner, and volume names. Lowercase letters, digits, and hyphens, at most 64 characters, unique across entries |
| `repositories[].cpus`                | Optional CPU limit of each job container, a number above 0 and at most 64; 2 when absent                                                     |
| `repositories[].memoryGb`            | Optional memory limit of each job container in whole gigabytes, 1 to 256; 8 when absent                                                      |

An invalid configuration stops the script before it touches Docker, with one line
per problem naming the field. A field not listed above is rejected as unknown, not
ignored, so a misspelled `label` cannot silently drop `labels`. The isolation rules the validation enforces, such
as what a registration carries and which names collide, are in the Per-Repository Isolation on a Runner Host
section of [Security](../../docs/conventions/security.md).

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
name pattern, CPU and memory limits, and volume names without calling Docker or
GitHub, and exits 1 on an invalid configuration.

## Tests

```bash
mise run test:host
```

runs [`tests/test-host-configuration.ps1`](./tests/test-host-configuration.ps1).
Each fixture in `tests/fixtures/` breaks one rule and must be rejected with a
message naming its field; each in `tests/accepted/` sits on the edge of a rule
(no volumes, a UNC token path, the maximum slot count, the limit bounds) and must
be accepted; the example must be accepted with distinct names and volume names.
The test also builds a job container's `docker run` arguments and checks the
limit flags. It never calls Docker or GitHub. CI also runs it under Windows PowerShell 5.1 on a
GitHub-hosted Windows runner.
