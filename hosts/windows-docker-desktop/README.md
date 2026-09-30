# Windows with Docker Desktop

Scripts that run ephemeral GitHub Actions runners for several repositories from
one Windows machine with Docker Desktop. A supervisor reads a machine-local host
configuration, and for each target repository keeps its slots filled with
single-job containers built from
[`images/actions-runner/`](../../images/actions-runner/README.md).
[Windows Runner Host](../../docs/operations/windows-runner-host.md) owns the
operator procedure: prerequisites, tokens, scheduled tasks, health checks, and
recovery.

| File                           | Purpose                                                                                      |
| ------------------------------ | -------------------------------------------------------------------------------------------- |
| `supervisor.ps1`               | Runs every repository's slots; `-ValidateOnly` checks a configuration and prints its plan    |
| `rebuild-image.ps1`            | Builds the image from this checkout under the configured image name                          |
| `register-scheduled-tasks.ps1` | Registers the supervisor and weekly rebuild tasks and restricts the token files              |
| `runner-host.example.json`     | An example host configuration with two repositories, using obviously fake names              |
| `host-configuration.ps1`       | Reads and validates a configuration; shared by the scripts above                             |
| `docker-commands.ps1`          | Runs the Docker client and reports its exit code; shared by the supervisor and its slot jobs |
| `tests/`                       | The validation test and its invalid-configuration fixtures, run by `mise run test:host`      |

The scripts run on Windows PowerShell 5.1 and PowerShell 7, and their files are
ASCII-only.

## Host configuration

A JSON file kept outside the repository; every script takes its path as
`-ConfigPath`. Copy `runner-host.example.json` and replace every value.

| Field                                | Meaning                                                                                                   |
| ------------------------------------ | --------------------------------------------------------------------------------------------------------- |
| `hostPrefix`                         | Names the scheduled tasks, containers, and volumes. Lowercase letters, digits, and hyphens                |
| `imageName`                          | The local image tag `rebuild-image.ps1` builds and the supervisor runs                                    |
| `repositories[].owner`, `repository` | The target repository. Each owner and repository pair appears once                                        |
| `repositories[].slots`               | How many jobs run at once for this repository, 1 to 16                                                    |
| `repositories[].tokenPath`           | The file holding this repository's own fine-grained token, re-read on every registration                  |
| `repositories[].labels`              | Custom labels, at least one. `self-hosted`, `linux`, and `x64` are always added and not listed            |
| `repositories[].volumes`             | The cache volumes as `suffix` and `mountPath` pairs, possibly none; never shared between entries          |
| `repositories[].prefix`              | Optional override of the container and volume name prefix, `<hostPrefix>-<owner>-<repository>` by default |

An invalid configuration stops the script before it touches Docker, with one line
per problem naming the field. Two entries whose prefixes are equal, or where one
is the other followed by a hyphen, collide and are rejected.

## Commands

From an elevated PowerShell prompt for the registration, and any prompt for the
rest:

```powershell
.\supervisor.ps1 -ConfigPath C:\path\to\runner-host.json -ValidateOnly
.\rebuild-image.ps1 -ConfigPath C:\path\to\runner-host.json
.\register-scheduled-tasks.ps1 -ConfigPath C:\path\to\runner-host.json
.\supervisor.ps1 -ConfigPath C:\path\to\runner-host.json
```

`-ValidateOnly` prints each repository's registration labels, container prefix,
and volume names without calling Docker or GitHub, and exits 1 on an invalid
configuration.

## Tests

```bash
mise run test:host
```

runs [`tests/test-host-configuration.ps1`](./tests/test-host-configuration.ps1).
Each fixture in `tests/fixtures/` breaks one rule and must be rejected with a
message naming its field; the example must be accepted with distinct prefixes and
volume names. The test never calls Docker or GitHub.
