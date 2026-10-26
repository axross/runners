# CLI

The Rust command line tool under [`cli/`](../../cli): what `validate` accepts and
prints, how it stays equal to the PowerShell validator, the shared fixtures both
read, the runtime layer that runs a host's pools (the container backend, JIT
registration, CPU placement, and the slot loop), the instance layer that keeps
`start` to one instance per machine (the instance state, the single-instance lock,
and the control channel), the image contract every runner image declares and the
check of it, running a host with `build`, `start`, `stop`, `restart`, `status`, and
`diagnostics`, the diagnostic containers a diagnostics-enabled pool runs and their
export, the toolchain and the gates that cover it, how a release is built and
installed, and why its name is fixed. The
binary is `marshal`. The PowerShell supervisor and rebuild task
under [`hosts/windows-docker-desktop/`](../../hosts/windows-docker-desktop) still
run the axpc host until Phase 4b, and the CLI must not be started beside them; see
[Windows Runner Host](./windows-runner-host.md).

## Running It

```bash
marshal validate --config <path> [--profile axpc|axmc]
marshal build --config <path> [--profile axpc|axmc]
marshal start --config <path> [--profile axpc|axmc]
marshal stop
marshal restart
marshal status
marshal diagnostics
marshal --version
```

`validate` reads the configuration file and the marker files above it, and writes
nothing. It does not check that a token file exists or who can read it; `start`
does. Running a Host describes `build`, `start` and the four clients, and their
exit codes. This section is about `validate`.

| Exit | Meaning                                                                                                                                                                                              |
| ---- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 0    | The configuration is valid. The plan summary is on standard output, with `\n` line endings.                                                                                                          |
| 1    | The file is missing or unreadable, is not valid JSON, or breaks a rule. Every error is on standard error, in the PowerShell validator's format and order, and nothing is printed to standard output. |
| 2    | A usage error, including no `--profile` on an operating system that has no default.                                                                                                                  |

Without `--profile`, Windows is `axpc` and macOS is `axmc`. On any other operating
system `validate` stops with exit 2 and asks for `--profile`. `--profile` lets the
maintainer, the tests, and CI check either host's configuration anywhere.

A rejected configuration prints `Invalid host configuration (<path>):` and then
one line per error, a hyphen then `<path>: <problem>`, where the path uses the notation
`repositories[0].labels`. A configuration that is not an object keeps the path in
the header and has the single error `configuration: must be an object`.

A configuration larger than 1 MiB (1048576 bytes) is refused before it is parsed:
`Host configuration cannot be read: <path>: the file is larger than 1048576 bytes (1 MiB)`,
exit 1. Real configurations are a few kilobytes, so the limit only stops a mistaken
path from being read whole. It is a Rust-only rule, pinned by
`a_file_over_the_size_limit_is_rejected_without_being_parsed` in
`cli/tests/command.rs`, which also checks a file of exactly 1 MiB is read.

The uniqueness checks compare every pair of entries, so their time grows with the
square of the number of entries, and it grows with the labels per entry too. On a
release build in a 4-CPU cloud container, 8,000 entries (about 0.8 MB) in distinct
repositories take about 0.6 s when nothing collides, and 8,000 entries in one
repository with a distinct label each take about 0.9 s. With many labels per entry it takes
longer: 1,500 entries in one repository with 50 labels each take about 3 s. Colliding
entries grow the error output with the square too, because every error is reported:
8,000 entries in one repository with the default labels print about 32 million error
lines and use about 9 GB of memory, and several collisions per pair or shorter entries
(the 1 MiB limit fits about 11,900) give more. The 1 MiB limit bounds both. Real
configurations are expected to have about ten entries.

Known limitation: when `validate` cannot write its output, for example a closed
pipe, it exits 1, the same code as a rejected configuration. The behaviour is
unchanged on purpose; a caller that must tell the two apart reads the output.

## Profiles

A profile names a host. The shared rules and messages are the PowerShell
validator's. The profile decides the fields, the platform labels, the routing
label, the path form, and the extra checks.

| Aspect                  | `axpc` (Windows, Docker Desktop)                                         | `axmc` (macOS host)                                                                       |
| ----------------------- | ------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------- |
| Platform labels         | `self-hosted, linux, x64`                                                | `self-hosted, linux, ARM64` for `os: linux`; `self-hosted, macOS, ARM64` for `macos`      |
| Routing label           | `axpc`, the default when `labels` is absent or empty; a list replaces it | `axmc`, with the same default and replacement                                             |
| Paths                   | Drive-letter or UNC                                                      | Absolute POSIX: start with `/`, no control characters, no empty, `.`, or `..` parts       |
| Root fields             | `imageName` (required), `repositories`                                   | `budget` (required), `repositories`, `imageName` (optional; its use is undecided)         |
| Entry fields            | The existing schema, with `volumes`                                      | The same without `volumes`, plus `os` and `diskGb`, both required                         |
| Foreign platform labels | Accepted and listed, such as `ARM64`                                     | Rejected: another host's or OS's platform label, such as `macOS` on a Linux pool or `x64` |

On `axmc`, `os` is `linux` or `macos`, `diskGb` is the per-VM disk cap, and
`budget` holds `cpus`, `memoryGb`, and `diskGb`. `volumes` is rejected on an entry
even as `null`, because VM pools keep no volumes. The axmc fields are provisional:
later phases may extend them but MUST NOT change what this phase accepts without a
recorded migration.

### New Rules

These rules exist on `axmc` only. Their messages follow the `<path>: <problem>`
form, and each is pinned by a fixture under
[`tests/host-configuration/rust/`](../../tests/host-configuration/rust). A later
phase MUST NOT change the text without a recorded migration.

| Rule                                                  | Path and message                                                                                                                                                               |
| ----------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `volumes` present                                     | `repositories[i].volumes: not supported on axmc, because VM pools keep no volumes`                                                                                             |
| `os` missing, or not `linux` or `macos`               | `repositories[i].os: required field is missing`, or `...os: must be "linux" or "macos"`                                                                                        |
| `diskGb` missing or out of range                      | `repositories[i].diskGb: required field is missing`, or `...diskGb: must be an integer from 1 to 16384`                                                                        |
| `budget` missing, not an object, or a member unusable | `budget: required field is missing` or `budget: must be an object`; `budget.cpus` follows the entry `cpus` rule; `budget.memoryGb` is 1 to 1024; `budget.diskGb` is 1 to 16384 |
| A platform label the pool cannot provide              | `repositories[i].labels: '<label>' is a platform label that a <os> pool on axmc cannot provide, so list only non-platform labels`                                              |
| More than two macOS VMs                               | `budget: macOS pools total N VMs (the sum of their slots), above the limit of 2`                                                                                               |
| CPU, memory, or disk use above the budget             | `budget: pools total N CPUs (the sum of slots times cpus), above the budget of M`, and the same for `GB of memory` and `GB of disk`                                            |

The budget errors come after every per-entry and cross-entry error. They sum
`slots` times the limit over the entries that join the cross-entry checks. The
macOS VM limit is fixed by the host, so it is checked even when `budget` itself is
unusable.

### Labels

`validate` rejects two entries for the same repository that share a non-platform
label and have the same `os`, and accepts them when `os` differs. On axpc every
entry is Linux, so the existing rule and message stand.
[Security](../conventions/security.md) owns the rule, including what a consumer
names in `runs-on` and the cross-host case that `validate` cannot see.

### The Checkout Rule

`diagnosticDirectory` MUST lie outside this project's checkout, so evidence a
diagnostic run collects cannot be committed by accident. The checkout is the first
directory above the configuration file that holds both `.git` (a directory or a
file, which a worktree has) and `cli/Cargo.toml`; another repository, such as a
dotfiles repository at the home directory, does not count. The comparison is
lexical on absolute paths after collapsing repeated separators, and does not
resolve links. It is case-insensitive on axpc and case-sensitive on axmc. When no
checkout is found, a note on standard error says the rule was not applied, so the
summary on standard output stays equal to the PowerShell one. The rule applies on
both profiles but can only bite where the profile's path form matches the
operating system: axpc on Windows, axmc on Linux and macOS.

Known limitation: the axmc comparison is case-sensitive, while macOS's default APFS
volume is case-insensitive. A `diagnosticDirectory` that differs from the checkout
only in case therefore passes the rule even though it names the same directory. The
validator cannot know a volume's case sensitivity, so it does not guess.

## Parity With the PowerShell Validator

On the shared fixtures the CLI accepts and rejects what the PowerShell validator
does, with the same ordered error lists and the same summaries, until the
PowerShell one is removed. Behaviour that is easy to miss:

- **Required and direct fields.** A field the PowerShell validator reads as a
  required value reports `null` as `required field is missing`: `repositories`,
  `imageName` on axpc, and per entry `owner`, `repository`, `tokenPath`, `slots`,
  `name`, `volumes`, `diagnosticDirectory`, a volume's `suffix` and `mountPath`,
  and on axmc `budget`, its members, `os`, and `diskGb`. A field it reads directly
  reports `null` as its own type error: `cpus`, `memoryGb`, `labels`,
  `diagnostics`, and `diagnosticRawRecords`.
- **Names.** Known fields are found case-insensitively; the unknown-field check is
  case-sensitive, so `"Owner"` gives only `Owner: unknown field`.
- **Patterns.** A pattern matches the whole value, so a trailing newline does not
  pass, as with PowerShell's `\z`.
- **Numbers.** Integer fields accept only an integer literal: `8.0` and `1e1` are
  rejected. Decimal fields accept integers, decimals, and exponents. A CPU count is
  rounded half away from zero at nine decimals on its 15-significant-digit form,
  and a value that rounds to zero is rejected. An out-of-range literal such as
  `1e400` gets the field's range error, not a parse error.
- **Cross-entry checks.** An entry joins them only when its owner, repository,
  token path, labels, volumes, slots, name, `cpus`, and `memoryGb` are all usable
  (on axmc: the same without volumes, with `os` and `diskGb`). A bad `cpus`,
  `memoryGb`, or `labels` excludes it. An unknown field, or a bad `diagnostics`,
  `diagnosticRawRecords`, or `diagnosticDirectory`, does not.
- **Strings.** Owner, repository, label, and name comparisons are case-insensitive
  over ASCII, which their patterns allow. Token paths compare under Unicode simple
  case folding. Every string in a `labels` list joins the duplicate set, as in
  PowerShell, including one that fails its own pattern.
- **Input.** The file is UTF-8 strict JSON. A leading byte-order mark is ignored.
  An exact duplicate key keeps its last value at the position of its last
  occurrence. Two keys that differ only in case, and an empty key, are errors. A
  value more than 128 levels below the root is an error, and so is a file over 1
  MiB (see Running It). PowerShell 7 reads about 1024 levels and accepts large
  files, and it rejects an empty key as Rust does.

### Declared Divergences

These inputs are outside parity. Each is either a Rust-only fixture under
[`tests/host-configuration/rust/raw/`](../../tests/host-configuration/rust/raw), whose
bytes MUST NOT be reformatted, or a Rust test where the input is not a file. A
fixture's expectation records PowerShell's own outcome, or `skip` where PowerShell
cannot run it or its result depends on the platform's globalization data.
The case-folding rows are not files: `fold::tests::simple_folding_differs_from_powershell_eq_on_declared_pairs`
in `cli/src/fold.rs` pins Rust's side of them, and the PowerShell side is
recorded in the table. Eight `rust/raw/` fixtures record `powershell: accept`:
`comment.json`, `trailing-comma.json`, `leading-zero-integer.json`,
`single-quoted-string.json`, `unquoted-property-name.json`, and
`lone-surrogate-escape.json` for the strict-JSON row, `invalid-utf8.json` for the
UTF-8 row, and `root-array.json` for the root row. The parity test runs PowerShell on
them to confirm it. The other inputs in the strict-JSON row were probed by hand with
PowerShell 7.6.6 and have no fixture. The depth and size rows are Rust tests, not
fixtures: `the_depth_limit_is_128_levels_below_the_root` in `cli/src/json.rs` and the
size test named under Running It, with PowerShell's side probed by hand.

| Input                                                                                                                                                                                                                                                                       | Rust                                                               | PowerShell                                                                                                                                                                                                                                |
| --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| A missing file, truncated or otherwise invalid JSON, two keys that differ only in case, an empty key                                                                                                                                                                        | Rejects with its own wording                                       | Rejects with a .NET message                                                                                                                                                                                                               |
| A value more than 128 levels below the root, a file over 1 MiB                                                                                                                                                                                                              | Rejects                                                            | Reads them; PowerShell 7 reads about 1024 levels                                                                                                                                                                                          |
| Invalid UTF-8                                                                                                                                                                                                                                                               | Rejects                                                            | Substitutes U+FFFD and may accept                                                                                                                                                                                                         |
| A non-object root                                                                                                                                                                                                                                                           | Rejects and keeps the path in the header; a root array is rejected | Omits the path; unrolls a one-object array                                                                                                                                                                                                |
| Anything outside RFC 8259 (strict JSON): comments, trailing commas, single-quoted strings, unquoted property names, leading zeros (`01`), `.5`, `0x2`, `NaN`, no-break space or form-feed whitespace, a trailing NUL, U+2028 after the document, and lone surrogate escapes | Rejects with its own wording                                       | PowerShell 7 (Json.NET) accepts each at the JSON level, and accepts `NaN` for `cpus`, where it passes the range check (`slots` rejects it); Windows PowerShell 5.1 rejects comments and trailing commas, and the rest is not probed there |
| Token paths that `-eq` equates but simple case folding does not                                                                                                                                                                                                             | Treats them as different                                           | Treats them as equal                                                                                                                                                                                                                      |
| A `null` entry in `repositories`                                                                                                                                                                                                                                            | Rejects: `repositories[0]: must be an object`                      | Rejects with a binding error                                                                                                                                                                                                              |
| The checkout rule                                                                                                                                                                                                                                                           | Applies it on both profiles                                        | Anchors on the supervisor's own repository, on Windows only                                                                                                                                                                               |
| U+017F against `s` or `S`, U+0345 against U+03B9 or U+0399, U+1E9B against U+1E61 or U+1E60                                                                                                                                                                                 | Equal under simple case folding                                    | `-eq` treats each pair as different (PowerShell 7.6.6 on Linux)                                                                                                                                                                           |
| The Kelvin sign U+212A against `k` or `K`                                                                                                                                                                                                                                   | Equal under simple case folding                                    | The result follows the platform's globalization data (equal on PowerShell 7.6.6 on Linux)                                                                                                                                                 |

## The Runtime Layer

The library holds the code that runs a host's pools: the container backend, JIT
registration, and the loop of one slot. `start` reaches it through the instance
layer, and the tests also drive it through its seams. It reproduces the
PowerShell supervisor, and where it differs the difference is listed here.

| Module                                              | Holds                                                                                  |
| --------------------------------------------------- | -------------------------------------------------------------------------------------- |
| `affinity`                                          | The CPU offsets `validate` and the backend share, and the selection of a slot's CPUs   |
| `runtime::backend`                                  | The backend seam                                                                       |
| `runtime::docker`, `runtime::docker_args`           | The container backend and the exact `docker` argument lists                            |
| `runtime::registration`, `token`, `scrub`, `secret` | JIT registration, the token file, error text, and the type that hides a secret         |
| `runtime::clock`, `runtime::log`                    | The time source and the sink for the runtime's messages, each a seam the tests replace |
| `runtime::name`                                     | Runner and container names, and the test for a pool's own                              |
| `runtime::pool`                                     | Pools from a plan, and their preparation                                               |
| `runtime::worker`                                   | The slot loop and its backoff                                                          |
| `runtime::retained`                                 | The diagnostic containers the backend retains, their records, and who finalises each   |
| `runtime::diagnostic`                               | Finalising diagnostic containers at job end, in the container steps, and at startup    |
| `diagnostics`, `diagnostics::exporter`              | The exporter child and the lines and exit codes it answers with                        |
| `diagnostics::sink`, `archive`, `identity`          | The sink checks and admission, the strict archive reader, and the identity readback    |
| `runtime::board`                                    | Each pool's and each slot's state, which `status` reports                              |
| `runtime::fake`                                     | In-memory stand-ins for the seams, used by tests; a debug or test build only           |

### Backend

The runtime asks a backend to wait until it is available; check the configured
image, as [The Image Contract](#the-image-contract) describes; list a pool's
containers and remove containers; prepare volumes; choose a slot's CPUs; run a
container under the run contract and report its exit code; name the containers it
has started and not seen end; stop, end, or remove a set of containers within a
budget; and, for a diagnostics-enabled pool, retain each diagnostic container it
starts with the settings it ran with, export one by a deadline, and count a pool's
bundles in its sink, as [Diagnostics](#diagnostics) describes. The runtime's own
finalisation, the same for every backend, decides whether to export and removes
the container after it. The JIT
configuration reaches the backend as a `Secret`, whose `Debug` form is redacted and
which has no `Display`. The container backend passes it as an environment variable
by name; a later backend that runs the containers in a virtual machine can pass it
another way. A fake backend implements the same seam for the tests of the code
above it.

The container backend runs `docker` with the PowerShell supervisor's argument
lists, byte for byte, except for the stop call below. A test reads the launcher
guard and the probe's reader script out of `docker-commands.ps1` and compares
them; the tests compare every other list with one written out in the test:

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

The stop call differs from the supervisor's. `supervisor.ps1` runs
`docker stop <names>`, so Docker's default grace of 10 s applies. The backend always
passes `--time <seconds>`, the grace its caller chose, so no caller inherits a
default it did not pick.

The launcher is the guard script followed by `exec "$@"`. It reads
`Cpus_allowed_list` from `/proc/self/status` and exits 125 unless it equals the set
the container was given. A job container has no network option, `--privileged`,
socket mount, or `--user`, and an ordinary one has no label. The JIT variable is
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
`cli/src/image/contract.rs` fails when `docker-commands.ps1` no longer holds the
variant's argument list and launcher as the CLI writes them.

Every client starts in its own process group: `process_group(0)` on Unix, and
`CREATE_NEW_PROCESS_GROUP` on Windows. A Ctrl+C in the terminal then reaches the CLI
only, which can stop the containers in order. A test reads the group of every client
the fake `docker` sees on Linux and macOS; no safe API reads a Windows process's
creation flags, so the Windows flag is checked by review. A client is killed when
the future that waits for it is dropped.

Every client other than a job container is bounded, which the PowerShell
supervisor did not do. A listing, a volume command, and an info call are bounded at
120 s. A removal and a stop take the budget their caller passes; stale cleanup removes
within 120 s. The CPU probe and the removal of a leftover probe have the budgets
under CPU Placement. A `docker info` that fails, hangs, or cannot start (the
program is missing) means Docker is not available yet. Output from a job container
is forwarded to the log line by line, and the log masks every secret in it. A line
longer than 64 KiB goes in pieces, each cut where it splits no masked secret, the
bytes after the cut waiting for the next piece; see [The Log](#the-log).
The output of any other client is read within its budget, and when it becomes error
text it is joined onto one line and cut to 500 characters followed by `...`, as a
registration error is.

### CPU Placement

A slot starts at its pool's offset: the CPUs of every earlier pool's slots, then
the CPUs of its own earlier slots, each slot counting `ceil(cpus)` CPUs. The summary
`validate` prints and the backend call the same functions in `affinity`, and a test
compares what the summary prints with what the pools carry.

The probe is a short container with no network, no mounts, no registration and no
pull, which prints the CPUs it may use. A leftover probe of the same name is
removed first, and only when `docker ps` finds it by its exact name and the
`runners.cpu-probe=1` label and prints exactly that name. The probe has a budget of
30 s, of which 1 s is kept to end a client that overran it. Removing a leftover
probe has 5 s. Output is capped at 64 KiB, and one byte more is refused. Standard
error is discarded. The removal runs again after the probe whatever happened, and a
failed removal outranks the probe's result. Any doubt is an error; the backend never
returns an unrestricted set.

The selection starts at the offset modulo the number of allowed CPUs and takes
consecutive IDs from the sorted list, wrapping. It writes them as `a,b-c`. The list
must be ordered, distinct ranges of IDs that fit a 32-bit signed integer, with no
space, sign or leading zero. The syntax of the whole list is judged before any ID's
range or order, as PowerShell does, so a list with several defects is refused as
invalid. The PowerShell golden case is a test: the inventory
`2,5-6,11,14-15` with five slots of two CPUs gives `2,5`, `6,11`, `14-15`, `2,5`,
and `2,15`, and the rejected inventories are the PowerShell test's.

### Registration

A registration is `POST /repos/<owner>/<repo>/actions/runners/generate-jitconfig`
to `https://api.github.com`, with the PowerShell supervisor's headers
(`Authorization: Bearer <token>`, `Accept: application/vnd.github+json`,
`X-GitHub-Api-Version: 2022-11-28`, `Content-Type: application/json`) and body
(`name`, `runner_group_id` 1, and the pool's labels). It reads `encoded_jit_config`;
a response without one is a failure. It differs from PowerShell in four ways:

- it sends `User-Agent: marshal/<version>`;
- it follows no redirect, so a redirect is a failure;
- it fails a connection after 10 s and a whole request after 30 s;
- it fails a response larger than 1 MiB, whether the size is declared or streamed.

Each failure goes into the slot's backoff. The error text has the token replaced by
`<token>`, whitespace collapsed, and is cut to 500 characters followed by `...`,
as the PowerShell supervisor does. The token is read again from its file at every
registration, trimmed, and held in a `Secret`; the header that carries it is marked
sensitive. The HTTPS client is built without reqwest's `json` feature: the body is
three validated fields written by a small writer with the escapes RFC 8259
requires, and the response is read with the crate's own parser. The client keeps
reqwest's handling of the `HTTPS_PROXY` and `NO_PROXY` environment variables, and
does not read the Windows system proxy, which the PowerShell supervisor did; no
test covers the proxy variables.

TLS uses rustls with the `ring` provider, installed explicitly by
`install_tls_provider` before the client is built, and the platform's certificate
verifier. Installing twice is fine. A failure to install it or to build the client
is a setup error for the caller to report, never a panic. A test builds the client
on every operating system in CI; a handshake with GitHub is first exercised on a
real host.

The base address can be overridden only by a debug build, and only to
`http://127.0.0.1:<port>`, for the loopback tests; a debug build refuses any other
value. A release build ignores the request and always uses GitHub, so a release
binary cannot send a token anywhere else. `tests/base_address.rs` shows both halves,
and CI runs it again in the release profile.

### Names and the Slot Loop

A runner is named `<pool>-<slot>-<yyyyMMddHHmmssfff>` in UTC, and its container has
the same name. PowerShell used local time; the pattern, and so the cleanup of its
leftovers, is unchanged. A name is recognised as a pool's own only when it is the
pool's name, a hyphen, a slot number, a hyphen, and exactly 17 ASCII digits.

A slot loops until it is asked to stop. Each attempt checks the image, then probes
the CPUs, then registers, then runs the container. An image that cannot run jobs,
or a check Docker does not answer, registers nothing and goes into the backoff. A
failed probe or registration, or a non-zero exit, waits
5 s, then 10 s, doubling to 300 s; an exit of 0 resets the delay and starts the
replacement at once. A wait ends early when a stop is asked for, and a stop that arrives as a registration completes starts no container. Time and sleeping
come through a `Clock` seam, so the tests do not wait.

Preparing the pools follows the PowerShell supervisor, with the image check
between the wait for Docker and the pools' cleanup. A pool whose token file is
missing or blank is skipped with a warning that names the pool and the reason, and
never the token. Docker is waited for once. Each remaining pool's own stale
containers are cleaned up and its volumes prepared; a pool that fails either is
skipped. When no pool is left there is nothing to run. Stale cleanup removes only
containers that match the pool's own pattern and do not carry the
`runners.diagnostic-lifecycle=1` label; a failed removal is a warning only, and a
failed listing skips the pool. The backend's own retained containers keep their
records and are left alone. Any other labelled container is finalised with the
pool's current settings when the pool's diagnostics are on; when they are off it
gets a bounded stop and the pool is refused, as [Diagnostics](#diagnostics)
describes.

### What the Tests Cover

`docker_backend.rs` runs the real backend against the `fake-docker` helper, a
program built by Cargo because Windows finds only `.exe` commands. It checks the
example configuration's argument lists, the JIT variable reaching only the `run`
client and no argument, the process groups, the probe limits, the stale cleanup and
the wait for Docker. `registration.rs` runs the client against a loopback stub for
the request, the error text, a redirect, a stalled response and an oversize
response. `slot_worker.rs`, `pool_preparation.rs` and `cpu_affinity.rs` use the
fake seams, and `image_check.rs` drives the image check through the fake backend,
the instance, and the real backend against `fake-docker`. The diagnostics tests are
listed under [Diagnostics](#diagnostics). `real_docker.rs` runs a container and the
probe through a real Docker daemon on Linux, and exports a container's small
`metrics.txt` through the exporter child from a real `docker cp` stream; it is
`#[ignore]`d for a local `cargo test`, and CI runs it in its own step, where it
fails when Docker or the image is missing. Not verified: a real
Docker daemon outside that CI step, the TLS handshake with GitHub, and Windows and
macOS runtime behaviour beyond what the hosted runners show.

## The Instance Layer

The library also holds what makes `start` one instance per machine: the
instance's state and the restarts of its runtime core, the single-instance lock,
the control channel that other terminals use, the checks for private files and
directories, the runtime core itself, and the log file. `start` and its clients
use it, and the tests also drive it through its seams, the runtime layer's fake
backend and clock, and a helper process.

| Module                                    | Holds                                                                                       |
| ----------------------------------------- | ------------------------------------------------------------------------------------------- |
| `instance::lifecycle`                     | The instance's state, the answer to each request, and the runtime core's restarts           |
| `instance::restart`                       | When a failed runtime core starts again, and when the instance gives up                     |
| `instance::source`                        | The instance's own configuration file, read again at a restart or a resume                  |
| `instance::lock`, `instance::roots`       | The single-instance lock, and where the lock and the channel live                           |
| `instance::channel`, `instance::protocol` | The control channel's server and client, and its text protocol                              |
| `instance::windows`                       | The current user's security identifier and the descriptor that admits only that user        |
| `private`                                 | The Unix checks for a private directory and a private file, and the current user ID         |
| `instance::runtime_core`                  | The runtime core `start` runs: pool preparation and every slot's loop                       |
| `instance::log_file`                      | The log `start` writes through, its file, the file's cap, and the files kept                |
| `instance::fake`                          | A scripted runtime core and configuration source, used by tests; a debug or test build only |

### Instance State

An instance is running or stopped, with stopping and resuming between them, and it
starts running. `build` holds the lock as an instance of its own, in the state
`building`, or `stopping` once it is asked to stop; [Build](#build) describes it.
One task owns the state and answers every request, so a request
that arrives during a transition is answered at once: `status` reports the state,
`stop` is taken, and `restart`, `diagnostics`, and `resume` are refused, naming the state. A
running instance refuses a resume too.

- `stop` ends the instance from every state. From running it first runs the
  container steps; from stopping it lets the stop finish; from resuming it ends
  without waiting for the configuration; from stopped it touches no container.
- `restart` runs the container steps, then resumes. On a stopped instance it
  resumes directly.
- A resume names a profile and a path. A stopped instance refuses one whose profile
  or path is not the one it was started with, and reads nothing. The path is the
  canonical one `start` will send; the instance only compares it.
- Resuming clears the runtime core's failures, reads the instance's own
  configuration file again, and validates it. A valid file runs the core with the
  new plan; an invalid one leaves the instance stopped, with the errors as its
  reason. The file is fixed when the instance is built, and no request names
  another, so nothing a client sends makes the instance read a different file.
  `start` fixes it as the path it was given, made absolute but not resolved, so
  each read finds the checkout above it as the first validation did; the
  canonical path is kept only to compare with a resume's.
- A resume and a `restart` check the image after the file validates, before the
  instance answers. An image that cannot run jobs leaves the instance stopped,
  with a reason that names the image and says to run `marshal stop`, then
  `marshal build --config <path>`, then `marshal start --config <path>`, because
  `build` is refused while the instance exists. When Docker does not answer the
  check, the instance answers as before and runs the core, which checks again once
  Docker answers and moves the instance to stopped with that reason.

The container steps stop every ordinary container the backend has started and not
seen end, in one `docker stop` call with a grace of 15 s, ended at 20 s, and then
force-remove them in one call ended 6 s later, so they take 26 s at the most.
Alongside, they finalise the diagnostic containers the backend retains, on their
own clock, by 24 s; [Diagnostics](#diagnostics) has the timeline. A forced removal,
which a second Ctrl+C or SIGINT asks for, abandons the stop or skips it and removes
at once. At a stop, the container steps and the core's wind-down share an exit
deadline of 28 s from the request; whatever still runs then is dropped, and when the
removal had not finished, how the instance ended records a failed removal. The
instance reads the containers before it tells the core to stop, because dropping a
worker forgets its container. How the instance ended records whether every removal
in its life succeeded, at a stop, a restart, or a failure of the core, so a failed
removal is still reported when a later stop has nothing to remove.

The runtime core is the part that runs the pools. It fails when it ends, with an
error, a panic, or neither, while no stop was asked for. The instance polls it
inside `catch_unwind`, so a panic becomes a failure carrying the panic's message.
Unwinding drops the core's workers, so the container backend keeps the name of
each container whose run a panic unwound through, and forgets it when a removal
succeeds; the instance stops and removes those containers before the restart. When
no pool can start at the instance's first run of the core, the instance ends and
`start` exits 1; at a later run, after a restart or a resume, that is a failure
like any other. A slot's registration or
container failure stays in that slot's backoff and is not a core failure. After a
failure the instance runs the container steps for the containers the core reports,
then starts the core again after 10 s for the first failure in a sliding 10-minute
window, 30 s for the second, and 60 s for the third. A fourth failure in the window
stops the instance, with the restart limit as its reason. A core restart that
finds an image that cannot run jobs moves the instance straight to stopped, with
the reason a resume gives, and counts no failure. A failure exactly 10
minutes old still counts and an older one does not; this window replaces the
PowerShell supervisor's reset after 10 healthy minutes. A clock that moves back
forgets nothing.

### Single-Instance Lock

The lock is machine-wide, across every OS user and configuration file. The
operating system releases it when the process ends, however it ends, and no child
process inherits it.

- **Windows.** The lock is the first instance of the named pipe
  `\\.\pipe\marshal-instance`, created with an instance limit of one, remote
  clients rejected, a handle no child inherits, and the descriptor below that
  admits only the creating user. The process keeps it open and it carries no
  message. Creating the name again fails for every user, with access denied or pipe
  busy, and that is "held".
- **Unix.** The lock is an exclusive advisory lock (`flock`) on a regular file: on
  macOS `/Users/Shared/.marshal-instance.lock`, and on Linux only a debug build's
  test root, because Linux is not a supported host. The creator sets the file's
  mode to `0644` after creating it, whatever the umask, so another user can open
  it. The file is opened for writing and, when the user may not write it,
  read-only; the lock then refuses that user rather than the open. Both opens pass
  `O_NOFOLLOW` and `O_NONBLOCK`, and what they open must be a regular file. So a
  link at the path is refused without its target being opened, a FIFO is refused
  at once instead of waiting for a reader, and anything that is not a regular file
  is "cannot be opened", never "held". After locking, the path must still name
  the locked file, not a replacement, or the lock is "lost".
- **Unix residual case.** That last check runs once, when the lock is taken. The
  lock file's owner, the user who created it first, can remove it while an
  instance holds it and create a new one, and a second instance can then lock the
  new file while the first still holds the old one. So can anyone else who may
  remove the file from its directory. The owner can also hold the lock and keep
  every other user's `start` from running.

The three refusals read differently: the lock is held by another process; the lock
cannot be opened, with the reason; or the lock file was replaced or removed after
it was locked.

### Control Channel

The channel is per user, and only the instance's own user can reach it. The
holder of the lock opens it, so a leftover Unix socket proves that no instance owns
it.

- **Windows.** The named pipe `\\.\pipe\marshal-control-<user SID>` is created as
  the first instance, with the descriptor `D:P(A;;GA;;;<user SID>)`, which grants
  all access to that user, to no one else, and inherits nothing, and with remote
  clients rejected. The pipe crate's blocking calls run on threads of their own: one
  accepts, and each connection is served on its own thread, which hands the
  request to the instance and waits for the answer. Up to 16 connections are served
  at once; a further one is closed unanswered. A connection that sends nothing
  holds its thread until it disconnects, because the blocking calls have no
  timeout. Closing the channel wakes the accepting thread with a connection of its
  own, trying twice, and waits for that thread only when a wake connects; otherwise
  the thread stays blocked until the next client, which it closes unanswered, or
  until the process ends. Serving threads are not joined, and a connection that
  sends nothing keeps its thread's handle on the instance's request queue, so the
  instance cannot rely on that queue closing to learn that the channel has closed.
  Another user who creates this user's pipe name first stops the channel
  from opening, and a client of this user would then reach that user's pipe; that
  is the accepted cost of squatting a name on a single-user host.
- **Unix.** The socket `control.sock` is in a directory that must be a directory,
  not a link, owned by the current user, with mode `0700`: on macOS
  `~/Library/Application Support/marshal/`, and on Linux only a debug build's test
  root. The server creates the directory with that mode when it is missing and
  refuses one that is not private, without changing it. It removes a leftover
  socket of the current user and refuses anything else at the path. It sets the
  socket to `0600` and checks it. It accepts a peer only when the peer's user ID
  is its own, and closes any other unanswered. Up to 16 connections are served at
  once, and a further one waits its turn. A request must arrive within 5 s.
  A socket path longer than the operating system allows is a startup failure.
  The client checks the directory before it connects, so it does not talk to a
  socket another user placed.

On both systems an ending instance first drops its request queue, so a request
still waiting is answered that the instance is ending, then stops accepting and
waits up to 0.5 s for every connection it accepted to end, so an answer it has
given, such as `stop`'s, is written before the process exits. A connection still
sending its request does not hold the exit longer than that.

The protocol is UTF-8 lines. A request is one line of at most 16 KiB: `status`,
`stop`, `restart`, `diagnostics`, or `resume <profile> <path>`, where the path is
the rest of the line and may hold spaces. A path with a line break, or one that is not UTF-8, is
refused before the client connects. A response is a line `ok` or `refused` with an
optional message, then a line per field, a name, a space and its value, and then a
line `end`; a line break in a value becomes a space, so it cannot end the response
early. A response carries states, reasons, paths, and names, never a token or a JIT
configuration. Any other line is answered with `refused`, or closed unanswered
when it is too long, and never reaches the instance.

A client gives up connecting after 2 s and treats that as no instance, as it does a
missing socket or pipe, so the moment before a new instance opens its channel and
after an ending one closes it reads as no instance. It then waits for the answer as
long as its caller allows, and an answer that does not arrive in time is "did not
answer in time". The client reads at most 64 KiB.

### Roots and Private Files

The lock file, the socket directory, the log directory and the pipe names are fixed
for a host. Only a debug build can move them, to a test root, which suffixes the
Windows pipe names with a hash of the root; a release build ignores the request, as
it ignores a GitHub base address. `tests/instance_roots.rs` shows both halves and
each host's fixed roots, and CI runs it again in the release profile. `std` has no
call for the process's own user ID and `unsafe` is forbidden, so the CLI reads its
user ID as the owner of a file it creates in the temporary directory. The file's
name has a random part, and the CLI only ever creates it new: a name that already
exists is skipped, never opened, and after eight taken names reading the user ID
fails.

On macOS a token file must also be a regular file owned by the current user with
mode `0600`, checked on the file that is opened, or reading it fails and the pool is
skipped with a warning; at a registration the failure goes into the slot's backoff.
On Windows the token file's access control list is not checked, because no safe call
reads it.

### What the Instance Tests Cover

`instance_state.rs`, `core_restart.rs`, `instance_lock.rs`, `control_channel.rs`,
and `instance_roots.rs` drive the instance layer through its seams as one OS user,
and each test's name says what it shows. Two CI steps cover what one account cannot.
On the Linux and macOS hosted runners, `tests/ci/other-user-lock.sh` copies the
helper to a directory under `/tmp`, holds the lock in a debug-only root there, and
runs the copy as `nobody` through `sudo`; it fails unless `nobody` is refused with
the "held" exit code. On the Windows hosted runner,
`tests/ci/control-pipe-security.ps1` has the helper open the control pipe, connects
to it, reads the live pipe's descriptor through .NET, and fails unless the access
control list is protected and holds one entry, allowing the current user full
access. Neither crate the CLI uses reads a live pipe's descriptor without `unsafe`,
so that check is in PowerShell. Not verified: the Windows lock against a second OS
user, and the rejection of remote clients, which a hosted runner cannot show; and
macOS and Windows behaviour beyond what the hosted runners show.

## The Image Contract

The CLI runs jobs only in an image built for its run contract: the argument lists,
the launcher, and the labels under [Backend](#backend), and what they need from the
image. The image declares the contract it was built for, and the CLI checks it
before any pool runs and before each job.

| Module              | Holds                                                                                    |
| ------------------- | ---------------------------------------------------------------------------------------- |
| `image::contract`   | The label names, and this CLI's contract version, read from the embedded Dockerfile      |
| `image::definition` | The image definition embedded in the binary, and the debug-only test definition          |
| `image::archive`    | The build context: a ustar archive of the definition, written by the CLI itself          |
| `image::check`      | The image check's format and outcomes, the refusal, the freshness warnings, and `status` |

### Labels

An image `build` makes carries five labels. The Dockerfile's final stage declares
the first three, so an image built from the definition another way, in CI, by hand,
or by the PowerShell rebuild task, carries those three only; `build` adds the last
two on its command line, because they describe the build, not the definition.

| Label                                       | Value                                           | Set by         |
| ------------------------------------------- | ----------------------------------------------- | -------------- |
| `com.github.axross.runners.contract`        | The image contract version, a positive integer  | The Dockerfile |
| `com.github.axross.runners.runner-version`  | The runner version the base image pins          | The Dockerfile |
| `com.github.axross.runners.runner-released` | That release's date, `YYYY-MM-DD`, at 00:00 UTC | The Dockerfile |
| `com.github.axross.runners.image`           | `actions-runner`, which the prune filters by    | `build`        |
| `com.github.axross.runners.built`           | The build time, in Unix seconds                 | `build`        |

The CLI's own contract version is read from the same embedded Dockerfile when the
binary is compiled, so the two cannot drift inside one binary; a Dockerfile without
a well-formed version does not compile. Contract 1 is the run contract as Phase 2
left it, plus the diagnostic variant's use of the collector's path and output
directory.

Unit tests in `cli/src/image/contract.rs` keep the declaration honest:

- `APPROVED_CONTRACTS` pins a fingerprint of the run contract's argument lists, the
  launcher, the diagnostic variant's argument lists and launcher as
  `docker-commands.ps1` gives them, the label keys and the inspect format, against
  the version they were approved for. Changing any of them without raising the
  contract version fails, and the failure names the row to add with the new
  version.
- `RUNNER_RELEASES` holds each runner version's release date. The declared runner
  version must equal the base image's tag, have a row, and declare the row's date.
  A Dependabot runner bump stays red until both labels and the row are updated by
  hand; see
  [Development Workflow](./development-workflow.md#bumping-the-runner).
- The embedded set must be exactly the Dockerfile and every file it copies, and
  `cli/tests/build-definition/Dockerfile` must start from the same base and declare
  the same labels.

### The Image Check

The check reads the configured image's labels with one
`docker image inspect --format [{{json (index .Config.Labels "<label>")}},...]`, for
the contract, build-time, and runner labels. A name whose last path part has no tag
is checked as `<name>:latest`, as `docker run` resolves it. When inspect fails, a
`docker image ls --quiet <name>` tells a missing image from Docker not answering.
Both calls share a bound of 4 s, so a resume's check fits inside `start`'s 10 s
wait, and the container steps and the check fit inside `restart`'s 35 s.

| Outcome            | When                                                                         |
| ------------------ | ---------------------------------------------------------------------------- |
| Matching           | Inspect succeeds, and the contract label equals the CLI's                    |
| Mismatched         | Inspect succeeds, but the contract label is missing, malformed, or different |
| Missing            | Inspect fails, and the listing succeeds with empty output                    |
| Docker unavailable | Any other failure, or the 4 s running out                                    |

Docker unavailable is never a mismatch. A mismatched image's message is
`the image <name> has image contract <m>, but this CLI needs image contract <n>`,
and a missing one's is
`the image <name> is missing; this CLI needs one with image contract <n>`. The
check runs:

- **At the first run of the core.** Once Docker answers, before any pool is
  prepared. An image that cannot run jobs ends the instance, and `start` exits 1.
- **At a resume, a `restart`, and a core restart.** As
  [Instance State](#instance-state) describes: the instance stays stopped, with
  the stop, build, start sequence as its reason.
- **At each job launch.** Before the CPU probe and registration. An image that
  cannot run jobs registers nothing and goes into the slot's backoff with the
  message; a check Docker does not answer goes into the backoff as a Docker
  failure. `build` cannot run while an instance exists, so a mismatch here means the
  image was replaced another way, such as by the PowerShell rebuild task.
  `docker run` resolves the name again after the check, and a replacement between
  the two is the accepted residual case.

### Freshness

The check also reads the build-time and runner-release labels. An age is
⌊(now − t) / 86,400 s⌋ in days, where t is the build time, or the release date at
00:00 UTC. An age under 7 days gives no warning, 7 to 20 days the first, and 21 or
more the second:

| Age        | Build age                                                                                     | Runner release age                                                                                                                                        |
| ---------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 7 to 20    | `` the image is <d> days old and should be rebuilt with `marshal build` ``                    | `runner <v> was released <d> days ago, and a newer runner may exist`                                                                                      |
| 21 or more | `` the image's operating-system packages are <d> days old; rebuild it with `marshal build` `` | `runner <v> was released <d> days ago, so each job's runner likely updates itself at start; update the CLI to get a newer runner, then rebuild the image` |

An image without a build-time label, such as one the PowerShell rebuild task or CI
built, has an unknown build age, and its warning says it was not built by
`marshal build`. `start` logs the current warnings at each run of the core, at
start, a resume, a `restart`, and a core restart, and not at a job launch. Neither
the warnings nor an unknown age block a start.

`status` shows the image as of the latest check, a job launch's included:

- `image: <name>`;
- `image-contract: <n>`, `mismatch (image <m>, CLI <n>)`, `missing`, or
  `not checked`, which shows only before the first check Docker answers; a later
  check Docker does not answer leaves the latest answer in place;
- `image-built: <d> days ago`, or `unknown`;
- `runner: <version>, released <d> days ago`, or `unknown`;
- one `warning: <text>` line per freshness warning.

`image-built` and `runner` read `unknown` before the first check Docker answers, and
when the image is missing or its labels are missing or malformed.

## Running a Host

`start` runs the host's pools in the foreground through the instance layer and the
runtime layer, and `stop`, `restart`, `status`, and `diagnostics` talk to it from another terminal
over the control channel. `build` builds the runner image, and the same clients
talk to it. On axpc the PowerShell supervisor and rebuild task keep running the
host until Phase 4b, and neither `start` nor `build` may run beside them; see
[Windows Runner Host](./windows-runner-host.md).

| Module                   | Holds                                                                                                |
| ------------------------ | ---------------------------------------------------------------------------------------------------- |
| `start`                  | `start`: the lock, validation, the log file, containment, the channel, the instance, and its signals |
| `build`                  | `build`: the lock, validation, the channel, its states, and its `docker` steps and their aborts      |
| `control`                | The clients `stop`, `restart`, `status`, and `diagnostics`                                           |
| `shim`                   | The Unix shim between `start` and one job container's `docker run` client                            |
| `diagnostics`            | The exporter child, listed under The Runtime Layer                                                   |
| `instance::runtime_core` | The runtime core, listed under The Instance Layer                                                    |
| `instance::log_file`     | The log, listed under The Instance Layer                                                             |

### Start

A fresh `start` does these in order:

1. It takes the instance lock. When the lock is held it asks this user's instance
   to resume instead, as below.
2. It validates the configuration as `validate` does. An invalid one prints every
   error and exits 1, and nothing is written. With `--profile axmc` a valid
   configuration still exits 1, because the VM backend comes in Phase 6.
3. It opens its log file and logs the plan summary.
4. It contains its children, builds the HTTPS client, and opens the control
   channel. A failure of any of them, including a TLS setup failure, exits 1.
5. It runs the instance. The runtime core waits for Docker every 5 s, checks the
   image as [The Image Contract](#the-image-contract) describes, prepares the pools
   as [Names and the Slot Loop](#names-and-the-slot-loop) describes, and then runs
   every slot's loop until a stop. When the image cannot run jobs, the instance
   ends, and `start` exits 1, saying to run `marshal build --config <path>`, then
   `marshal start --config <path>`. When no pool can start, `start` exits 1.

On Linux a release build exits 1 at the first step, because Linux is not a
supported host, and `tests/instance_roots.rs` shows it in the release profile. A
debug build runs on a test root, for the tests. On macOS, `start --profile axpc`
runs the container backend with Unix containment, which serves the tests, not an
operated host. `MARSHAL_TEST_ROOT` moves the lock, the channel, and the log
directory, `MARSHAL_TEST_GITHUB_API` moves the GitHub base address,
`MARSHAL_TEST_DOCKER` names the `docker` client that `start` and `build` run, the
Unix shim's removal in `start` included, and `MARSHAL_TEST_BUILD_DEFINITION` names
a test definition that `build` builds instead of the embedded one. Only a debug
build honours any of them: in the release profile `tests/base_address.rs` shows
that the client stays `docker`, and `tests/build_definition.rs` that `build` keeps
the embedded definition. The tests name the fake client rather than put it first on `PATH`,
because Windows searches the program's directory and the system directories before
`PATH`, so a real `docker.exe` in `System32` would win.

A `start` that finds the lock held changes nothing. It canonicalises its
configuration path, sends `resume <profile> <path>`, and waits up to 10 s:

- A stopped instance started with the same path and profile reads its own file
  again. A valid file runs it and `start` exits 0; an invalid one leaves it stopped
  with the new reason and `start` exits 1.
- A running, stopping, resuming, or building instance refuses, naming its state,
  and so does a stopped one started with another path or profile. `start` exits 1.
- A stopped instance whose image cannot run jobs stays stopped with that reason,
  and `start` exits 1.
- When nothing answers on this user's channel, `start` exits 1, saying that the
  lock is held by another OS user or by an instance of this user that is still
  starting or ending. The lock does not say which, and `start` never connects to
  another user's channel.

### The Clients

Each client gives up connecting after 2 s. With no instance it says so and exits
1, and an answer that does not arrive in time exits 1, saying the instance did not
answer in time.

- `status` waits up to 5 s and prints one `name: value` line per field: `state`;
  `reason`, when stopped; `config`; `profile`; one `pool` line per pool, which is
  `waiting for Docker`, `preparing`, `running`, or `skipped: <reason>`; one `slot`
  line per slot, which is `registering` (the CPU probe included), `backing off`,
  or `running container <name>`; and the image lines under
  [The Image Contract](#the-image-contract). Pools and slots are listed while the
  runtime core runs; the image lines are listed on every call, as of the latest
  check. A `build` answers `state`, `image`, and `elapsed: <s> s`.
- `restart` waits up to 35 s for the instance to run its container steps, read its
  file again, check the image, and answer. An invalid file or an image that cannot
  run jobs leaves the instance stopped, and `restart` exits 1. It is refused while
  the instance is stopping, resuming, or building.
- `stop` asks the instance to stop, then checks about every 200 ms whether the
  instance lock is free, by taking it and letting it go at once, until 35 s after
  its request. It exits 0 once the lock is free, so a `start` right after it finds
  the lock free, and 1 when the instance still holds it.
- `diagnostics` waits up to 30 s for the instance to finalise the diagnostic
  containers it retains, and prints the answer; [Diagnostics](#diagnostics)
  describes its lines and exit codes. It is refused while the instance is
  stopping, resuming, or building, and while another `diagnostics` is under way.

### Stopping

`stop`, SIGINT, SIGTERM, and SIGHUP on Unix, and Ctrl+C and Ctrl+Break on Windows
end the instance from any state with the stop sequence, which takes 30 s at the
most from the request. SIGHUP is how macOS reports a closed terminal. A signal
`start` cannot listen for is a warning at startup, and that signal keeps its
default action; the others still stop the instance.

1. The slots stop registering at once.
2. The container steps under [Instance State](#instance-state) run: one
   `docker stop --time 15`, ended at 20 s, and then one `docker rm -f`, ended 6 s
   later; alongside, each diagnostic container is exported and removed by 24 s.
3. At 28 s whatever still runs is dropped. A `docker` client is killed; on Unix a
   job container's shim sees its liveness pipe close, removes its container, or
   ends a diagnostic one and keeps it, within 5 s, and ends.
4. `start` gives the answers it has already sent, such as the one to `stop`, up to
   0.5 s to reach their clients, closes the channel, and exits, which releases the
   lock.

A second SIGINT, Ctrl+C, or Ctrl+Break skips the grace period: a `docker stop` under
way is abandoned and the ordinary containers are removed at once, while each
diagnostic container is ended with `docker kill` and kept without its export.
`start` then exits 1, and its last line names each kept container. Raising Ctrl+C in a
Windows test needs `unsafe`, so the Windows handling is checked by review and on
axpc in Phase 4b. That includes Ctrl+Break: a client's own process group keeps
Ctrl+C from it, but whether it also keeps Ctrl+Break from it is unverified.

| Exit | Meaning                                                                                                                                                                                                                                      |
| ---- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 0    | `start` stopped and every removal of containers in its life succeeded, or a client's request succeeded                                                                                                                                       |
| 1    | An invalid configuration, a refused or failed `start`, a failed resume, no pool that can start, a client with no instance, a refused request or no answer in time, a failed removal at a stop, or a diagnostic container kept or not removed |
| 2    | A usage error                                                                                                                                                                                                                                |

### Diagnostics

A pool with `diagnostics` on runs each job in the diagnostic variant under
[Backend](#backend): no `--rm`, the label `runners.diagnostic-lifecycle=1`, and the
guard followed by the image's collector, in raw mode when `diagnosticRawRecords` is
on. The rest of the run contract is unchanged. Docker keeps the container after it
ends, and the CLI finalises it: it exports the container's evidence into a bundle in
the pool's sink, `diagnosticDirectory`, and then removes the container with
`docker rm -f`, whatever the export did. The image's
[README](../../images/actions-runner/README.md) owns what the collector records, and
[Windows Runner Host](./windows-runner-host.md#collecting-private-diagnostics) the
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
`Diagnostic gap in pool <pool>, container <name>: <text>`, with the PowerShell
supervisor's text: a missing final observation, a full sink, a failed or unsafe
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
`stop` still ends within 30 s and its 35 s wait is unchanged.

**Startup.** While a pool is prepared, the CLI looks at the labelled containers of
the pool that an earlier process left and the backend does not retain:

- **Diagnostics on.** It retains them with the pool's current settings and
  finalises them on one clock per pool, as at a stop, and reports each one:
  `Diagnostic container <name> that an earlier run left: exported, removed.`, with
  `export gap` or `not removed` in place of either word.
- **Diagnostics off.** It gives them one bounded `docker stop --time 10`, ended at
  12 s, as PowerShell does, warns for each, and refuses the pool. The reason names
  the manual recovery in
  [Windows Runner Host](./windows-runner-host.md#recover-before-disabling-diagnostics).
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

**The exporter.** Each export runs in a child process: the CLI run again as the
hidden
`marshal export --pool <pool> --directory <sink> [--container <name>] [--raw-records] [--checkout <path>] --deadline-ms <ms> -- <docker>`.
Without `--container` it only counts the pool's bundles.

- **Sink checks.** Before each bundle the child checks the sink. It must be an
  existing directory outside the checkout above the configuration, judged by the
  checkout rule's lexical comparison, and neither it nor any directory above it may
  be a link or, on Windows, a reparse point; so on macOS a sink under `/tmp` or
  `/var` must be named by its `/private/…` path. A missing sink is refused, never
  created, and nothing is repaired. On Unix the sink must be owned by the current
  user with mode `0700`, a bundle is created `0700`, and each file in it `0600`. On
  Windows the sink's owner and access rules are not checked, and a bundle inherits
  the sink's rules; [Security](../conventions/security.md#diagnostic-evidence-stays-private)
  records the risk.
- **Admission.** The instance takes a lock for the pool and sink, then starts the
  child. The child counts the sink's `<pool>-bundle-` entries, partial bundles
  included, refuses at 10 as "sink full", creates
  `<pool>-bundle-<32 hexadecimal digits>`, and prints the line `admitted` and then
  `bundles <n>`. The instance releases the lock on `admitted`, or when the child
  ends. A second export of the same pool waits for the lock until its own export
  deadline, where PowerShell gave up after 0.5 s. Every export happens inside the
  one instance, which the machine-wide lock makes unique, so this lock replaces
  PowerShell's named mutex, which the CLI does not take.
- **Identity and archive.** The child writes `identity.txt` from the inspect
  allowlist under [Backend](#backend), its output capped at 1 KiB and required to
  match a fixed shape, as PowerShell's `Get-DiagnosticIdentity` does. It reads the
  `docker cp` archive through the same strict ustar subset as
  `Copy-DiagnosticTar`. It accepts the collector's directory entry, named `./`,
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
  `complete.txt` last, in PowerShell's formats. A `metrics.txt` without
  `final=observed` is a gap.
- **Isolation.** The child starts in a process group of its own, as the shim does,
  so a terminal's first Ctrl+C reaches only `start`, and a job-end export goes on
  during a graceful stop. Its standard input is a liveness pipe from the instance,
  and the instance passes it the export deadline that remains. The child keeps that
  deadline itself, and ends when it passes or when its standard input closes,
  killing its `docker` clients first. Each client runs in a process group of its
  own, with the JIT variable cleared and its standard error discarded, and every file
  system call runs on a thread of its own, so a hung sink cannot keep the child from
  ending. At the export deadline the instance closes the liveness pipe, and kills
  the child 1 s later only if it has not ended. On Windows the child also inherits
  `start`'s Job Object. So the child outlives neither its deadline nor its
  instance, and its clients end with it whenever it ends on its own. A child killed
  1 s after its pipe closed cannot kill its clients: on Unix they run until each
  ends by itself, and on Windows until `start` ends and its Job Object closes.
- **Exit codes.** 0 is a complete bundle, or a count; 3 a complete bundle with a
  gap; 4 a full sink; and 1 anything else, a refused sink, a failed copy, an archive
  outside the subset, a deadline, or a closed pipe. PowerShell's 5, "not admitted to
  containment", is not used: the Job Object on Windows and the liveness pipe on Unix
  are in place before the child starts.
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

- `diagnostic_archive.rs` ports every `Get-DiagnosticTar` case of the PowerShell
  test, and the limits at and one past each bound;
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
- `sink.rs`'s unit tests refuse a sink that is missing, under a link, inside the
  checkout, also when the checkout is named through a link, or not private, and on Windows one that is or lies under a junction or
  a directory symbolic link.

Not verified: the Windows console-close handling below, a diagnostic job on Docker
Desktop, and the macOS behaviour of the process-group signal the SIGINT tests send,
beyond what the hosted runners show.

### Build

`build` builds the image the configuration names, from the definition embedded in
the binary, and reads nothing from a checkout or the working directory. It does
these in order:

1. It takes the instance lock. When the lock is held it exits 1 and changes
   nothing, naming the instance's state when this user's channel answers within
   5 s, and otherwise giving `start`'s message for a lock held elsewhere.
2. It validates the configuration as `validate` does; an invalid one exits 1.
   `--profile axmc` exits 1 after validation, because axmc's image is a golden VM,
   which comes in Phase 6. An image name given by digest is refused, because it
   cannot be tagged. It prints the name it builds.
3. It contains its children as `start` does on Windows, and opens the control
   channel. While it builds, `status` answers `state: building`, `image: <name>`,
   and `elapsed: <s> s`; a `start`, a resume, `restart`, and `diagnostics` are refused, naming
   the build; `stop` aborts it.
4. Docker must answer `docker info` within 30 s, or `build` exits 1 before any
   build. A `docker info` that fails ends it at once, and the message gives the
   client's output, or why the client could not start. `build` is run by hand, so
   it does not wait for Docker as `start` does.
5. It removes the temporary tags of the image's repository that carry the image
   label, which an earlier `build` that was killed left. A failed removal is a
   warning.
6. It runs one
   `docker build --pull --no-cache --label com.github.axross.runners.image=actions-runner --label com.github.axross.runners.built=<s> --tag <temporary> -`,
   with the context on standard input and the client's output on the terminal.
   The temporary tag is `<repository>:marshal-build-<yyyyMMddHHmmssfff>` in UTC,
   where the repository is the name without its tag: everything before the last
   `:` that follows the last `/`, so a registry port stays.
7. It tags the build as the configured name, which commits it. It then removes the
   temporary tag, and prunes the dangling images that carry the image label, as
   the PowerShell rebuild script does. A failed tag removal is a warning naming the
   tag, which the next `build` removes, and a failed prune is a warning; both still
   exit 0.
8. A failed build or a failed retag removes the temporary tag, leaves the
   configured name as it was, and exits 1. A retag that stalls for 120 s with no
   stop asked is ended, and a `docker image inspect` of the configured name, as
   below, decides whether it committed; one that committed still prunes.

The context is a ustar archive of the embedded files at their paths in
`images/actions-runner/`, sorted by path, with mode `0644`, owner 0, and time 0, so
the same binary always sends the same bytes. A debug build builds the directory
named by `MARSHAL_TEST_BUILD_DEFINITION` instead, for the real-Docker test; a
release build ignores it, and `tests/build_definition.rs` shows both halves.

`stop`, SIGINT, SIGTERM, and SIGHUP on Unix, and Ctrl+C and Ctrl+Break on Windows
abort a build within 30 s of the request. The build client is killed at once; the
CLI has no graceful way to end it.

- **Before the retag.** The temporary tag is removed, by 26 s, and `build` exits 1.
  The configured name keeps its previous image. A failed removal names the tag.
- **During the retag.** The `docker tag` client gets up to 5 s from the stop. When
  it succeeds within them, the build has committed, and `build` exits 0. Otherwise
  a `docker image inspect` of the configured name, within 4 s, decides. This
  build's build-time label means the build committed, and `build` exits 0; another
  means it did not, and `build` exits 1. A failed or late inspect is "commit
  unknown", which names the image, and `build` exits 1.
- **After the retag.** The build has committed: the tag removal finishes, the prune
  is skipped for the next `build`, and `build` exits 0.
- At 28 s whatever still runs is dropped, and `build` exits 1.

An orphaned build client can still apply the temporary tag after `build` removed
it, on Unix always and on Windows until the Job Object closes, and a hard kill or a
console close can leave one too. The next `build` removes it.

| Exit | Meaning                                                                              |
| ---- | ------------------------------------------------------------------------------------ |
| 0    | The configured name holds the new image                                              |
| 1    | A refused, invalid, or failed `build`, Docker not answering, or an uncommitted abort |
| 2    | A usage error                                                                        |

### The Log

Each `start` writes one file, `marshal-<yyyyMMddHHmmssfff>-<pid>.log` in UTC, in a
private per-user directory:

- **Windows.** `%LOCALAPPDATA%\marshal\logs\`, created when missing. Its access
  control list is not checked, because no safe call reads it.
- **macOS.** `~/Library/Logs/marshal/`, created `0700` when missing and refused
  when it is not a directory owned by the user with that mode. A file is created
  new, set to `0600` whatever the umask, and checked.

The file is opened after validation succeeds, so a refused `start` writes none.
Every line goes to the terminal and to the file, which adds a time stamp. A write
to a terminal that is gone is ignored, and the file goes on. The newest 10 log
files by name are kept, the new one included, and only regular files named as a
log file count. An old file that cannot be removed is a warning in the new log,
and `start` goes on. A file stops at 10 MiB: a line that would leave no room for the
last line is not written, and the last line says that the file reached 10 MiB. A
failed write stops the file, with a warning on the terminal.

Every token read, from its first read in pool preparation on, and every JIT
configuration, from its registration until its container's outcome is reported,
is replaced by `<masked>` in every line, including the job container's output the
backend forwards, a secret that runs across two pieces of a long line included.
The default panic hook would write a panic's message to the terminal unmasked, so
`start` replaces it with one that names only where the panic happened. A panic of
the runtime core reaches the log as a failure that carries its message, with every
token read so far masked. A JIT configuration in that message is not: its mask
ends as the panic unwinds the slot that held it, before the instance logs the
failure.

### Containment

- **Windows.** At startup `start` puts itself in a Job Object with kill-on-close,
  through `win32job`, and never closes its handle. The operating system closes it
  when the process ends, however it ends and after its exit code is set, and every
  child in the job dies then. Docker Desktop keeps a container running after its
  client is killed, so the next `start`'s stale cleanup removes it.
- **Unix.** Each job container's `docker run` client is started by the shim, the
  CLI run again as the hidden
  `marshal shim --name <container> -- <docker> <arguments>`. The shim starts the
  client in a process group of its own, with the client's standard error joined to
  its standard output, which `start` reads and masks. Its standard input is the
  read end of a liveness pipe whose write end only `start` holds, and `start`
  never writes to it. When the client ends while the pipe is open, the shim exits
  with its code. When the pipe has closed, because `start` dropped the run or ended
  however it ended, the shim runs `docker rm -f <container>` within 5 s, kills the
  client, and exits, even when the client ended at the same time. For a diagnostic container `start` runs the shim
  as `marshal shim --name <container> --diagnostic -- <docker> <arguments>`, and
  the shim then ends its container with `docker kill` within 5 s and keeps it. The
  shim shares `start`'s standard error, and its own warnings there name containers
  only.
- **Exporter.** The exporter child under [Diagnostics](#diagnostics) has a
  liveness pipe of its own and keeps its own deadline, on every system, and on
  Windows it is also in the Job Object.

Security lists the
[residual risks](../conventions/security.md#tokens-stay-out-of-the-log-and-job-clients-end-with-start)
that containment and the log leave.

### Console Close on Windows

When the console closes, the user logs off, or the machine shuts down, `start` stops
the instance with forced removal: the slots stop registering, so no container starts
unseen, one `docker rm -f` removes every ordinary container it started, and, at the
same time, one `docker kill` ends every diagnostic container and keeps it without its
export, which the next `start` finalises. Whatever has not finished after 4 s is
dropped, and a diagnostic container whose kill did not finish runs on until the next
`start` finalises it, the answers already sent get up to 0.5 s, and `start`
exits. The 4.5 s fits inside the 5 s the operating system gives a console-close
handler before it ends the process, and Microsoft documents that logoff and shutdown
events reach services only, never an interactive process
([HandlerRoutine](https://learn.microsoft.com/en-us/windows/console/handlerroutine)).
The handlers are registered anyway, and for those two events the Job Object and the
next `start` are the backstop.

That the handler waits for the cleanup is known from the async runtime, not from a
test. Returning from the handler of these three events ends the process at once.
From version 1.44.0
([tokio#7122](https://github.com/tokio-rs/tokio/pull/7122)), tokio's Windows
handler does not return for them while a listener is registered: it notifies the
listener and parks the handler thread, so the process lives until `start` finishes
the removal and exits by itself. `cli/Cargo.toml` requires 1.54.1. No test can raise
the close event, so this, the diagnostic containers' kill within the 4 s included,
is unverified until axpc checks it in Phase 4b.

### What the Command Tests Cover

`start_command.rs` runs `start` and its clients as real processes on a test root,
with the `fake-docker` helper named by `MARSHAL_TEST_DOCKER` and the loopback
GitHub stub: the exit codes, resume, `status`'s fields, a restart that judges the file
at the path `start` was given as `validate` does, a `stop` that gets its answer
from an instance with no container, a `stop` that gives up after 35 s while the
lock stays held, a second `start` that runs nothing, a stop within 30 s when
`docker stop` ignores the grace period, the
signals and a second SIGINT on Linux and macOS, at most 10 log files, and a run
through a failed registration and a stop that keeps the token and the JIT
configuration out of the terminal, the log file, and every client's output. Its
process-tree tests observe a child's exit directly: on Unix, killing `start` closes
standard error only when every shim has ended, after which no job container is
left; on Windows, a handle to the job container's client is signalled when
`start` is killed. `runtime_core.rs`, and the board and masking cases in
`pool_preparation.rs` and `slot_worker.rs`, use the fake seams, and
`instance_state.rs` and `core_restart.rs` cover forced removal, the exit deadline,
a panicking core, and no startable pool. The diagnostics tests are listed under
[Diagnostics](#diagnostics).

`build_command.rs` runs `build` the same way, from an empty directory, with
`fake-docker` keeping an image store: the one build's arguments, labels, temporary
tag, and context bytes, equal to the definition's files and the same on a second
run; the order of the calls; the leftover, tag-removal, and prune cases; a failed
build and a failed retag; Docker not answering; the digest, axmc, usage, and
validation refusals; `stop` and SIGINT at each step of an abort, a stalled retag
and its inspect included; and the refusals while an instance exists and while a
build runs. The unit tests in `cli/src/build/steps.rs` run the steps against a
`docker` script on Unix, with a short command budget: a retag that stalls with no
stop, which prunes when it committed and is reported as a stall when it did not,
and one that succeeds within a stop's grace, which commits without an inspect.
`image_check.rs` runs `start` against images that cannot run jobs.
`real_docker_build.rs` runs `build` through a real Docker daemon on Linux, with the
test definition in `cli/tests/build-definition/`, the runner image's digest-pinned
base and its declared labels, and fails unless the result carries all five labels,
no temporary tag remains, and the container backend's image check finds the image
matching and an absent tag missing; it is `#[ignore]`d for a local `cargo test`. Not
verified: Ctrl+C, Ctrl+Break, and the console close on Windows, a `build` of the
full image by the CLI, which axpc shows in Phase 4b, and macOS and Windows behaviour
beyond what the hosted runners show.

## Fixtures and Tests

[`tests/host-configuration/`](../../tests/host-configuration) holds every fixture
and one [`expectations.json`](../../tests/host-configuration/expectations.json),
which the Rust tests, the PowerShell test, and the parity harness all read:

- `shared/accepted/` and `shared/rejected/` are read by both validators.
- `shared/pwsh7/` holds shared fixtures that Windows PowerShell 5.1 may judge
  differently, such as `1e400`. The Rust tests and the parity harness read them;
  the PowerShell test skips them below version 7.
- `rust/accepted/` and `rust/rejected/` hold the axmc and Rust-only rule fixtures.
- `rust/raw/` holds the declared divergences. `.prettierignore` and
  `.gitattributes` (`-text`) keep their bytes unchanged.

Each expectation names the fixture's profile, its outcome, `contains` and `lacks`
string arrays, and for a divergence fixture PowerShell's own outcome. The Rust tests
read the file into derived types through the crate's serde reader, with exact key
spelling, so an unknown or misspelled key fails the read. Expected
strings are ASCII, with newlines as JSON `\n` escapes. Both readers fail when a
fixture has no expectation or an expectation has no fixture. The byte-order-mark
input is built at run time from the axpc example, because a formatter or editor
would strip a stored mark.

`mise run test:parity` runs
[`parity.ps1`](../../tests/host-configuration/parity.ps1) under PowerShell 7. It
runs both validators over the shared fixtures, including `shared/pwsh7/`, the axpc
example, and the byte-order-mark copy. It fails when their outcomes differ, when
their ordered errors or summaries differ after the configuration path is
normalized, or when an expected string is missing or an unwanted one is present. It
also runs PowerShell over the divergence fixtures and fails only when its outcome
differs from the recorded one. Before comparing anything, a self-test feeds it a
deliberately wrong expectation and a stand-in validator that drops an error, and
requires both to fail. It runs on Linux only, because that is where the check job
has PowerShell 7 and the pinned tools; Windows PowerShell 5.1 runs the PowerShell
test against the same fixtures in its own job.

## Toolchain and Gates

The toolchain is one exact stable release in
[`cli/rust-toolchain.toml`](../../cli/rust-toolchain.toml), the minimal profile with
`rustfmt` and `clippy`, and [`cli/Cargo.lock`](../../cli/Cargo.lock) is committed
and used with `--locked`. [Agent Sessions](./agent-sessions.md) describes
provisioning it. The crate forbids `unsafe` and denies `unwrap`, `expect`, `panic`,
indexing, and the debug macros outside tests, because input paths must collect
errors, not abort.

| Task                     | Runs                                                 |
| ------------------------ | ---------------------------------------------------- |
| `format`, `format:check` | `cargo fmt`, and `cargo fmt --check`                 |
| `lint:rust`              | `cargo clippy --locked --all-targets -- -D warnings` |
| `test:rust`              | `cargo test --locked`                                |
| `deny:rust`              | `cargo deny --locked check bans licenses sources`    |
| `test:parity`            | `build:rust`, then the parity harness                |

Each Rust task first checks for `cargo`, `rustfmt`, and `clippy` and fails at once
with one line naming `.agents/install-rust`. A task never skips when the toolchain is
missing, because a gate that silently does not run would let a session report a
change as checked when it was not.

The `rust` job in
[`merge-checks.yaml`](../../.github/workflows/merge-checks.yaml) runs format check,
clippy, and tests on Linux, Windows, and macOS; the axpc checkout-rule cases that
need drive-letter paths run only on Windows. Four more steps run on Linux only: the
base-address, instance-roots, and build-definition tests again with `--release` and
`MARSHAL_EXPECT_RELEASE=1`, which makes each fail if built in the wrong profile;
`docker build --target base` of
[`images/actions-runner`](../../images/actions-runner), tagged and never pushed;
the `real_docker` test with `--ignored`, pointed at that tag by
`MARSHAL_REAL_DOCKER_IMAGE`, which also exports a container's evidence through the
exporter child from a real `docker cp` stream; and the `real_docker_build` test with
`--ignored`,
which removes the image it builds. The Windows and macOS hosted runners have no
Linux Docker daemon, and each test fails rather than skips when one is missing.
One more step runs on Linux and macOS, `tests/ci/other-user-lock.sh`, and one on
Windows, `tests/ci/control-pipe-security.ps1` under PowerShell 7; What the Instance
Tests Cover says what each shows. The `fake-docker` and `instance-helper` helpers are
further binaries in the package (`cli/helpers/`, `test = false`) that only the tests
and those steps run, and `default-run = "marshal"` keeps `cargo run` on the CLI.
`instance-helper` does nothing in a release build. A release MUST NOT ship either
helper; [Releases](#releases) says how the release build keeps them out. Tests find fixtures relative to the
crate's manifest directory, and the checkout-rule tests build a temporary directory
with the marker files at test time. The advisory check, `cargo deny --locked check advisories`,
fetches the RustSec database, so it runs in
[`rust-advisories.yaml`](../../.github/workflows/rust-advisories.yaml) and not in
`mise run check`; see [Development Workflow](./development-workflow.md).

## Releases

A release is a `v<version>` tag on the default branch, where `<version>` is the
`version` in `cli/Cargo.toml` at the tagged commit, such as `v0.1.0`. Releases are
not marked as prereleases. [Development Workflow](./development-workflow.md) gives
the maintainer's steps for cutting one, and
[Security](../conventions/security.md#a-release-is-published-only-from-an-approved-tag)
records the controls around it. `marshal --version` prints `marshal <version>`, from
the same field.

Each release carries three files, and nothing else:

| File                                | Built on         | Compiler host target     |
| ----------------------------------- | ---------------- | ------------------------ |
| `marshal-<version>-windows-x64.exe` | `windows-latest` | `x86_64-pc-windows-msvc` |
| `marshal-<version>-macos-arm64`     | `macos-latest`   | `aarch64-apple-darwin`   |
| `SHA256SUMS`                        | `ubuntu-latest`  | -                        |

The files are bare binaries, not archives, since there is one file per target.
`SHA256SUMS` is in the `sha256sum` format, one line per binary. The binaries are
not code-signed or notarised. Nothing modifies a binary after linking, so the macOS
binary keeps the ad-hoc signature its linker gave it.

[`release-build.yaml`](../../.github/workflows/release-build.yaml) builds them. It
is a reusable workflow with `contents: read` and no publish or attest step. Each
target builds natively on its hosted runner, with no cross-compilation, in one leg
that:

1. fails unless `rustc`'s host target is the one in the table, so a change of
   runner image fails the build instead of shipping the wrong architecture;
2. runs the base-address, instance-roots, and build-definition tests with
   `--release` and `MARSHAL_EXPECT_RELEASE=1`, the same tests the Linux `rust` job
   runs, on the target itself;
3. builds `cargo build --locked --release --bin marshal`;
4. fails unless the binary's `--version` prints `marshal <version>`, with the
   version read from `cli/Cargo.toml` by `.github/scripts/package-version.sh`;
5. copies the binary to its release name, and on Windows fails if it imports a
   `vcruntime` DLL;
6. hands its SHA-256 to the next job as a job output, and uploads that one file by
   name.

An assemble job then downloads both binaries, fails unless they are exactly the two
expected files and each SHA-256 matches the handed-over value, writes
`SHA256SUMS`, and uploads the three files as one artifact, `marshal-release`.

The Windows binary links the C runtime statically: `+crt-static` is set for
`x86_64-pc-windows-msvc` in [`cli/.cargo/config.toml`](../../cli/.cargo/config.toml),
so `marshal.exe` needs no Visual C++ redistributable on the host. It is set there,
not in a workflow, so the debug tests, the release-profile tests, and the release
build are compiled the same way. The import check runs `dumpbin /dependents`, found
through `vswhere` among the MSVC tools the build links with. It fails unless
`dumpbin` listed the binary's dependencies, `KERNEL32.dll` among them, so a run that
read nothing cannot pass.

A release never ships `fake-docker` or `instance-helper`. The release-profile tests
build both into `target/release`, so the build names `marshal` alone and copies only
that file out, each leg's artifact holds that one file, and the assemble and
publish jobs refuse a set of files that holds anything else.

[`merge-checks.yaml`](../../.github/workflows/merge-checks.yaml) calls the build
workflow on every pull request and every push to the default branch, as the dry
run: both legs and the assemble job run, and the `marshal-release` artifact is
uploaded to the run, but nothing is attested or published.
[`release.yaml`](../../.github/workflows/release.yaml) runs only on a `v*` tag:

1. A check job runs `.github/scripts/check-release-tag.sh`, which fails, naming the
   cause, unless the tag is `v` followed by `cli/Cargo.toml`'s version at the tagged
   commit and that commit is on the default branch. `mise run test:release-tag`
   tests it against a temporary git repository.
2. The build workflow builds and assembles the files.
3. A publish job in the protected `release` Environment waits for the maintainer's
   approval. It then downloads the artifact, fails unless it holds exactly the
   tag's three files and `sha256sum --check` passes, attests the build provenance
   of the two binaries with `actions/attest`, and creates the release with
   `gh release create --verify-tag --generate-notes`.

The runner image's definition is embedded in the binary (see [Build](#build)), so a
change to it, such as a runner bump, reaches a host only through a new release and
an update.

## Installing, Updating and Rolling Back

`marshal` is installed from a release, never built on the host. The procedure
accepts a binary only when its checksum matches `SHA256SUMS`, the tag's commit is
on the default branch, and its attestation verifies against `release.yaml`, the
exact tag, and that commit. It needs `gh` 2.102 or later, signed in
(`gh auth status`).

Until Phase 4b, the binary MUST NOT be started on axpc beside the PowerShell
supervisor; see [Windows Runner Host](./windows-runner-host.md). Once axpc runs the
CLI, it can roll back to the PowerShell path by stopping the CLI and registering
its Task Scheduler tasks again, as
[Registering the Scheduled Tasks](./windows-runner-host.md#registering-the-scheduled-tasks)
describes.

| OS      | Install path                                  |
| ------- | --------------------------------------------- |
| macOS   | `~/.local/bin/marshal`                        |
| Windows | `%LOCALAPPDATA%\Programs\marshal\marshal.exe` |

The operator adds the directory to the user's `PATH`. Updating is the procedure
with a newer tag, and rolling back is the procedure with the previous tag; the tag
ruleset keeps an old tag from moving.

Run the commands in order in one shell. Each check prints a line that starts with
`refused:` when it fails. On such a line, stop: run nothing after it and do not
install the binary.

1. Download the binary and `SHA256SUMS`.
2. Refuse unless the binary's SHA-256 matches its line in `SHA256SUMS`.
3. Resolve the commit the tag points to, dereferencing an annotated tag.
4. Refuse unless the compare API reports that commit `identical` to or `behind`
   the default branch, which means it is on it.
5. Refuse unless `gh attestation verify` passes, pinned to `release.yaml`, the tag,
   and the commit from step 3, with self-hosted runners denied. An attestation made
   by any other workflow, ref, or commit fails here.
6. Stop the instance if one is running, refusing if it does not stop, then replace
   the binary in the install directory. On macOS, remove the quarantine attribute
   if present.
7. Refuse unless the installed binary's `--version` prints `marshal <version>`.

On macOS, in bash or zsh:

```bash
version=0.1.0
asset="marshal-${version}-macos-arm64"
cd "$(mktemp -d)" || echo "refused: no temporary directory"

# 1. Download.
gh release download "v${version}" --repo axross/runners --pattern "$asset" --pattern SHA256SUMS \
  || echo "refused: the release files could not be downloaded"

# 2. The checksum.
awk -v file="$asset" '$2 == file' SHA256SUMS > "${asset}.sha256"
{ [ -s "${asset}.sha256" ] && shasum -a 256 -c "${asset}.sha256"; } \
  || echo "refused: ${asset} does not match SHA256SUMS"

# 3. The tag's commit.
read -r type sha <<<"$(gh api "repos/axross/runners/git/ref/tags/v${version}" --jq '.object.type + " " + .object.sha')"
if [ "$type" = tag ]; then
  read -r type sha <<<"$(gh api "repos/axross/runners/git/tags/${sha}" --jq '.object.type + " " + .object.sha')"
fi
if [ "$type" = commit ]; then commit="$sha"; else echo "refused: v${version} does not resolve to a commit"; fi

# 4. The commit is on the default branch.
comparison="$(gh api "repos/axross/runners/compare/main...${commit}" --jq .status)"
case "$comparison" in
  identical | behind) echo "${commit} is on main." ;;
  *) echo "refused: ${commit} is not on main (${comparison})" ;;
esac

# 5. The attestation.
gh attestation verify "$asset" --repo axross/runners \
  --signer-workflow axross/runners/.github/workflows/release.yaml \
  --source-ref "refs/tags/v${version}" --source-digest "$commit" \
  --deny-self-hosted-runners \
  || echo "refused: the attestation does not verify"

# 6. Stop the instance, then replace the binary through a rename.
if [ -x ~/.local/bin/marshal ]; then
  stopped="$(~/.local/bin/marshal stop 2>&1)" || [ "$stopped" = "No instance is running." ] \
    || echo "refused: the instance did not stop: ${stopped}"
fi
mkdir -p ~/.local/bin
cp "$asset" ~/.local/bin/marshal.new
chmod 0755 ~/.local/bin/marshal.new
mv -f ~/.local/bin/marshal.new ~/.local/bin/marshal
if xattr -p com.apple.quarantine ~/.local/bin/marshal >/dev/null 2>&1; then
  xattr -d com.apple.quarantine ~/.local/bin/marshal
fi

# 7. The installed version.
[ "$(~/.local/bin/marshal --version)" = "marshal ${version}" ] \
  || echo "refused: the installed binary is not marshal ${version}"
```

On Windows, in PowerShell:

```powershell
$version = '0.1.0'
$asset = "marshal-$version-windows-x64.exe"
$work = New-Item -ItemType Directory -Path (Join-Path ([IO.Path]::GetTempPath()) ([IO.Path]::GetRandomFileName()))
Set-Location -LiteralPath $work

# 1. Download.
gh release download "v$version" --repo axross/runners --pattern $asset --pattern SHA256SUMS
if ($LASTEXITCODE -ne 0) { Write-Host 'refused: the release files could not be downloaded' }

# 2. The checksum.
$listed = @(Get-Content -LiteralPath SHA256SUMS | Where-Object { ($_ -split '\s+')[1] -eq $asset })
$actual = (Get-FileHash -Algorithm SHA256 -LiteralPath $asset).Hash.ToLowerInvariant()
if ($listed.Count -ne 1 -or ($listed[0] -split '\s+')[0] -ne $actual) { Write-Host "refused: $asset does not match SHA256SUMS" }

# 3. The tag's commit.
$object = (gh api "repos/axross/runners/git/ref/tags/v$version" | ConvertFrom-Json).object
if ($object.type -eq 'tag') { $object = (gh api "repos/axross/runners/git/tags/$($object.sha)" | ConvertFrom-Json).object }
if ($object.type -eq 'commit') { $commit = $object.sha } else { Write-Host "refused: v$version does not resolve to a commit" }

# 4. The commit is on the default branch.
$comparison = gh api "repos/axross/runners/compare/main...$commit" --jq .status
if ($comparison -in 'identical', 'behind') { Write-Host "$commit is on main." } else { Write-Host "refused: $commit is not on main ($comparison)" }

# 5. The attestation.
gh attestation verify $asset --repo axross/runners `
  --signer-workflow axross/runners/.github/workflows/release.yaml `
  --source-ref "refs/tags/v$version" --source-digest $commit `
  --deny-self-hosted-runners
if ($LASTEXITCODE -ne 0) { Write-Host 'refused: the attestation does not verify' }

# 6. Stop the instance, then replace the binary.
$directory = Join-Path $env:LOCALAPPDATA 'Programs\marshal'
$target = Join-Path $directory 'marshal.exe'
if (Test-Path -LiteralPath $target) {
  $stopped = (& $target stop 2>&1 | ForEach-Object { "$_" }) -join "`n"
  if ($LASTEXITCODE -ne 0 -and $stopped.Trim() -ne 'No instance is running.') { Write-Host "refused: the instance did not stop: $stopped" }
}
New-Item -ItemType Directory -Force -Path $directory | Out-Null
Copy-Item -LiteralPath $asset -Destination "$target.new" -Force
Move-Item -LiteralPath "$target.new" -Destination $target -Force

# 7. The installed version.
if ((& $target --version) -ne "marshal $version") { Write-Host "refused: the installed binary is not marshal $version" }
```

Windows cannot replace an executable that is running, so step 6 also fails while an
instance still runs from the install path. A binary fetched with
`gh release download` is assumed to carry no Mark of the Web, and the procedure
does not clear one; Phase 4b checks that, and whether SmartScreen intervenes, on
axpc.

## Dependencies

A new crate MUST NOT be added without an entry here, and every crate in
[`cli/Cargo.lock`](../../cli/Cargo.lock) has one. [`cli/deny.toml`](../../cli/deny.toml)
allows crates.io only, one version of each crate, and the licences below. The CLI
is unpublished, so it carries no licence field of its own; `deny.toml` ignores
private crates for that reason.

| Crate                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              | Why                                                                                                                                                                                                                                                                                                                            | Licence                                                                                     |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------- |
| `clap`, with `clap_builder`, `clap_lex`, and `anstyle`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             | Argument parsing, with default features off                                                                                                                                                                                                                                                                                    | MIT OR Apache-2.0                                                                           |
| `regex`, with `regex-automata` and `regex-syntax`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  | The whole-value patterns for names, labels, and image names                                                                                                                                                                                                                                                                    | MIT OR Apache-2.0                                                                           |
| `aho-corasick` and `memchr`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        | Optional dependencies of `regex`                                                                                                                                                                                                                                                                                               | Unlicense OR MIT                                                                            |
| `serde`, with `serde_core`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         | Reading the parsed JSON tree and the shared expectations into derived types, with default features off                                                                                                                                                                                                                         | MIT OR Apache-2.0                                                                           |
| `serde_derive`, `syn` 3, `quote`, and `proc-macro2`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                | The `Deserialize` derive; these run at build time                                                                                                                                                                                                                                                                              | MIT OR Apache-2.0                                                                           |
| `unicode-ident`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    | A dependency of `syn` and `proc-macro2`                                                                                                                                                                                                                                                                                        | (MIT OR Apache-2.0) AND Unicode-3.0                                                         |
| `tokio`, `tokio-macros`, `mio`, `bytes`, and `slab`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                | The async runtime for the slot loop and the container backend, on one thread, with only the `rt`, `time`, `sync`, `io-util`, `process`, `macros`, `net`, and `signal` features; `tokio-macros` supplies `select!` and `join!`, `net` serves the Unix control socket, and `signal` delivers the stop signals and console events | MIT                                                                                         |
| `socket2`, `signal-hook-registry`, `errno`, `smallvec`, `once_cell`, `cfg-if`, `log`, `itoa`, and `bitflags`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       | Operating-system and small-utility dependencies of `tokio`, `reqwest`, and their dependencies                                                                                                                                                                                                                                  | MIT OR Apache-2.0                                                                           |
| `libc`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             | The `O_NOFOLLOW` and `O_NONBLOCK` values for the Unix lock's open, declared directly on Unix with default features off; `tokio` and its dependencies use it too                                                                                                                                                                | MIT OR Apache-2.0                                                                           |
| `pin-project-lite` and `atomic-waker`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              | Futures plumbing under `tokio` and `hyper`                                                                                                                                                                                                                                                                                     | Apache-2.0 OR MIT                                                                           |
| `futures-core`, `futures-task`, `futures-channel`, and `futures-util`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              | Future and channel traits `hyper` and `reqwest` use                                                                                                                                                                                                                                                                            | MIT OR Apache-2.0                                                                           |
| `reqwest`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          | The one HTTPS request to GitHub's JIT endpoint, with default features off and only `rustls-no-provider`, so no JSON, cookie, proxy-crate, or compression feature is built                                                                                                                                                      | MIT OR Apache-2.0                                                                           |
| `hyper`, `hyper-util`, `http-body`, `http-body-util`, `tower`, `tower-http`, `tower-layer`, `tower-service`, `tracing`, `tracing-core`, `want`, and `try-lock`                                                                                                                                                                                                                                                                                                                                                                                                                                                     | The HTTP client stack under `reqwest`                                                                                                                                                                                                                                                                                          | MIT                                                                                         |
| `http`, `httparse`, `base64`, and `ipnet`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          | HTTP types, header parsing, header encoding, and proxy address matching under `reqwest`                                                                                                                                                                                                                                        | MIT OR Apache-2.0                                                                           |
| `sync_wrapper`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     | A dependency of `reqwest`                                                                                                                                                                                                                                                                                                      | Apache-2.0                                                                                  |
| `url`, `form_urlencoded`, `percent-encoding`, `idna`, and `idna_adapter`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           | Parsing the request URL, under `reqwest`                                                                                                                                                                                                                                                                                       | MIT OR Apache-2.0                                                                           |
| `utf8_iter`, `stable_deref_trait`, and `zeroize`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | Dependencies of `idna` and the TLS stack                                                                                                                                                                                                                                                                                       | Apache-2.0 OR MIT for `utf8_iter` and `zeroize`, MIT OR Apache-2.0 for `stable_deref_trait` |
| `icu_collections`, `icu_locale_core`, `icu_normalizer`, `icu_normalizer_data`, `icu_properties`, `icu_properties_data`, `icu_provider`, `litemap`, `potential_utf`, `tinystr`, `writeable`, `yoke`, `zerofrom`, `zerotrie`, and `zerovec`                                                                                                                                                                                                                                                                                                                                                                          | Unicode normalisation data for `idna`                                                                                                                                                                                                                                                                                          | Unicode-3.0                                                                                 |
| `displaydoc`, `yoke-derive`, `zerofrom-derive`, `zerovec-derive`, and `synstructure`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               | Derives and their helper for the Unicode crates above; these run at build time                                                                                                                                                                                                                                                 | MIT OR Apache-2.0 for `displaydoc`, MIT for `synstructure`, Unicode-3.0 for the rest        |
| `rustls`, `hyper-rustls`, and `rustls-native-certs`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                | TLS for `reqwest`, with only the `ring`, `std`, and `tls12` features                                                                                                                                                                                                                                                           | Apache-2.0 OR ISC OR MIT                                                                    |
| `tokio-rustls`, `rustls-pki-types`, and `rustls-platform-verifier`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 | TLS glue, certificate types, and the platform's certificate verifier                                                                                                                                                                                                                                                           | MIT OR Apache-2.0                                                                           |
| `rustls-webpki` and `untrusted`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    | Certificate validation for `rustls`                                                                                                                                                                                                                                                                                            | ISC                                                                                         |
| `ring`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             | The cryptography provider, installed explicitly                                                                                                                                                                                                                                                                                | Apache-2.0 AND ISC                                                                          |
| `cc`, `shlex`, `find-msvc-tools`, and `getrandom`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  | Build-time C compiler discovery for `ring`, and its random-number source                                                                                                                                                                                                                                                       | MIT OR Apache-2.0                                                                           |
| `subtle`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           | Constant-time comparison for `rustls`                                                                                                                                                                                                                                                                                          | BSD-3-Clause                                                                                |
| `openssl-probe`, `security-framework`, `security-framework-sys`, `core-foundation`, `core-foundation-sys`, `windows-sys`, and `windows-link` 0.2                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | The platform certificate stores: the system store on Linux, the keychain on macOS, and the Windows APIs                                                                                                                                                                                                                        | MIT OR Apache-2.0                                                                           |
| `schannel`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         | The Windows certificate APIs; Cargo records it but the three CI targets do not build it                                                                                                                                                                                                                                        | MIT                                                                                         |
| `bumpalo`, `combine`, `jni`, `jni-macros`, `jni-sys`, `jni-sys-macros`, `js-sys`, `rustc_version`, `rustls-platform-verifier-android`, `rustversion`, `same-file`, `semver`, `simd_cesu8`, `simdutf8`, `thiserror` 2, `thiserror-impl` 2, `walkdir`, `wasm-bindgen`, `wasm-bindgen-futures`, `wasm-bindgen-macro`, `wasm-bindgen-macro-support`, `wasm-bindgen-shared`, `web-sys`, `winapi-util`, `windows-targets`, `windows_aarch64_gnullvm`, `windows_aarch64_msvc`, `windows_i686_gnu`, `windows_i686_gnullvm`, `windows_i686_msvc`, `windows_x86_64_gnu`, `windows_x86_64_gnullvm`, and `windows_x86_64_msvc` | Platform verifier and `getrandom` code for Android, WebAssembly, and other Windows targets, which the lockfile records and none of the three CI targets builds                                                                                                                                                                 | MIT OR Apache-2.0 (`same-file`, `walkdir`, and `winapi-util` are Unlicense OR MIT)          |
| `wasi`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             | A WebAssembly dependency of `getrandom`, not built on the CI targets                                                                                                                                                                                                                                                           | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT                                         |
| `webpki-root-certs`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                | Bundled roots the platform verifier uses on WebAssembly, not built on the CI targets                                                                                                                                                                                                                                           | CDLA-Permissive-2.0                                                                         |
| `interprocess`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     | The Windows named pipes behind the single-instance lock and the control channel, with default features off; it builds on Windows only                                                                                                                                                                                          | 0BSD OR Apache-2.0                                                                          |
| `recvmsg`, `doctest-file`, and `widestring`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        | Dependencies of `interprocess`: a receive helper, a procedural macro that includes example files in documentation, and wide-string types for Windows paths                                                                                                                                                                     | 0BSD for `recvmsg` and `doctest-file`, MIT OR Apache-2.0 for `widestring`                   |
| `winsafe`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          | The current user's security identifier, with only the `advapi` feature; it builds on Windows only                                                                                                                                                                                                                              | MIT                                                                                         |
| `win32job`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         | The Job Object that ends each job container's `docker run` client with `start`; it builds on Windows only (see [Containment](#containment))                                                                                                                                                                                    | MIT OR Apache-2.0                                                                           |
| `windows`, `windows-core`, `windows-collections`, `windows-future`, `windows-numerics`, `windows-threading`, `windows-result`, `windows-strings`, and `windows-link` 0.1                                                                                                                                                                                                                                                                                                                                                                                                                                           | The Windows API bindings `win32job` uses; they build on Windows only                                                                                                                                                                                                                                                           | MIT OR Apache-2.0                                                                           |
| `windows-implement`, `windows-interface`, `thiserror` 1, `thiserror-impl` 1, and `syn` 2                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           | The derives of the `windows` crates and `win32job`, with the older `syn` they use; these run at build time, on Windows only                                                                                                                                                                                                    | MIT OR Apache-2.0                                                                           |

`aho-corasick` and `memchr` are in the lockfile because `regex` lists them as
optional, but the CLI's feature set (`std` and `unicode-perl`, no `perf`) does not
build them. They are accepted under their MIT alternative, so the Unlicense is not
on the allowlist and need not be.

The JSON parser is in the crate, not a dependency. Parity needs the behaviours of
`ConvertFrom-Json` listed above: order-preserving objects, the last-position
duplicate rule, case-conflict errors, and number literals kept as text. The crate
owns that parser, and the fixtures pin it.

The reader implements serde's `Deserializer`, so a type that derives `Deserialize`
can be read from the parsed tree. A failed read is an error, never a panic, and
reading a struct from a node that is not an object is an error. Number literals
reach the tree's own value type as text; any other target sees an `i64`, `u64`,
`i128`, or `f64`, so `1e400` reads as infinity there. Field names match ignoring
ASCII case only when the caller asks, and only for struct fields. The validator
reads each configuration object (root, entry, volume, and budget) into a derived
type of wrapped fields, and keeps lists and nested objects raw so a wrong container
type is reported with its path. Nothing implements or derives `Serialize`: the
validator writes no JSON, and the registration body is written by hand (see
[Registration](#registration)).

Allowed licences are `MIT`, `Apache-2.0`, `Unicode-3.0`, `ISC`, `BSD-3-Clause`, and `0BSD`; a
crate whose licence is an `OR` expression is accepted when one alternative is on that
list. `Unicode-3.0` is on the list because `unicode-ident`, which `syn` uses, is
`(MIT OR Apache-2.0) AND Unicode-3.0`, and an `AND` needs both sides allowed; the
maintainer approved it at plan revision 10. `ISC` and `BSD-3-Clause` are on the list
because the HTTPS client's TLS stack needs them: `rustls-webpki` and `untrusted` are
`ISC`, `ring` is `Apache-2.0 AND ISC`, and `subtle` is `BSD-3-Clause`; the maintainer
approved both in the Phase 2 plan (issue #46, revision 1). `0BSD` is on the list
because `interprocess` needs it for the Windows pipes: `recvmsg` and `doctest-file` are
`0BSD`, and `interprocess` itself is `0BSD OR Apache-2.0`; the same plan approved it.
Adding another licence requires the maintainer's review of it.

`cargo deny` checks the three CI targets only (`deny.toml` sets `graph.targets`). The
lockfile also records crates that none of them builds, including an older
`windows-sys` and a newer `thiserror` beside the versions they build. Two older
versions are built beside the current one, on Windows only, and `deny.toml` lists each
as a skip with its reason: `syn` 2, which the `win32job` derives use while every other
derive uses `syn` 3, and `windows-link` 0.1, which the `windows` 0.61 crates use while
`windows-sys` uses 0.2. The Phase 2 plan approved both; another duplicate needs the
maintainer's review. One of the unbuilt crates,
`webpki-root-certs`, is `CDLA-Permissive-2.0`, which is not allowed. If a change makes
a CI target build it, `deny:rust` fails and the change needs the maintainer's review of
the licence.

`serde_derive`, `syn`, `quote`, `proc-macro2`, `displaydoc`, `yoke-derive`,
`zerofrom-derive`, `zerovec-derive`, `tokio-macros`, `doctest-file`, `thiserror-impl`,
`windows-implement`, and `windows-interface` run at build time, with
`synstructure` as the helper of the `yoke` and `zerofrom` derives. A new
procedural-macro crate needs the maintainer's review; see
[Security](../conventions/security.md).

## Renaming the CLI

The CLI's name is `marshal`, and it is fixed. It was provisionally `runners`, the
repository's name, until it was renamed before any release. The per-host names
derive from it:

- the instance lock, `\\.\pipe\marshal-instance` on Windows and
  `/Users/Shared/.marshal-instance.lock` on macOS;
- the control channel, `\\.\pipe\marshal-control-<user SID>` on Windows and a
  `control.sock` under `~/Library/Application Support/marshal/` on macOS;
- the log directory and file names, `%LOCALAPPDATA%\marshal\logs\` and
  `~/Library/Logs/marshal/`, with the `marshal-` prefix.

A later rename would be a breaking change for every installed host. The renamed
binary would not see a running instance's lock, so a second instance could start
beside it; its clients could not reach that instance's control channel; and its
logs would land in a new directory, away from the earlier ones.

The `runners` names still in the repository are the repository's name, not the
binary's: the image labels `com.github.axross.runners.*`, the container labels
`runners.diagnostic-lifecycle` and `runners.cpu-probe`, the `.cache/runners`
directory, the CI image tag `runners-docker-test:ci`, the login-profile markers in
`.agents/setup`, the `repository=runners` telemetry attribute in
`.claude/settings.json`, and the Amp continuation marker. The PowerShell host
under `hosts/` keeps its own `runners` names, such as its diagnostics mutex,
until Phase 4c retires it.
