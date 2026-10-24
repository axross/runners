# CLI

The Rust command line tool under [`cli/`](../../cli): what `validate` accepts and
prints, how it stays equal to the PowerShell validator, the shared fixtures both
read, the runtime layer that will run a host's pools (the container backend, JIT
registration, CPU placement, and the slot loop), the toolchain and the gates that
cover it, and what to touch when its provisional name changes. The binary is
`runners`. It has one command so far, `validate`; nothing that runs jobs uses it
yet, and the PowerShell supervisor under
[`hosts/windows-docker-desktop/`](../../hosts/windows-docker-desktop) still runs the
axpc host. The code that will run the pools is already in the library, reachable
from tests only; see The Runtime Layer.

## Running It

```bash
runners validate --config <path> [--profile axpc|axmc]
```

`validate` reads the configuration file and the marker files above it, and writes
nothing. It does not check that a token file exists or who can read it; `start`
will, in a later phase.

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

The library holds the code that will run a host's pools: the container backend,
JIT registration, and the loop of one slot. No command reaches it yet, because
`start` arrives in a later pull request of the same phase, so the tests drive it
through its seams. It reproduces the PowerShell supervisor, and where it differs
the difference is listed here.

| Module                                              | Holds                                                                                |
| --------------------------------------------------- | ------------------------------------------------------------------------------------ |
| `affinity`                                          | The CPU offsets `validate` and the backend share, and the selection of a slot's CPUs |
| `runtime::backend`                                  | The backend seam                                                                     |
| `runtime::docker`, `runtime::docker_args`           | The container backend and the exact `docker` argument lists                          |
| `runtime::registration`, `token`, `scrub`, `secret` | JIT registration, the token file, error text, and the type that hides a secret       |
| `runtime::name`                                     | Runner and container names, and the test for a pool's own                            |
| `runtime::pool`                                     | Pools from a plan, and their preparation                                             |
| `runtime::worker`                                   | The slot loop and its backoff                                                        |
| `runtime::fake`                                     | In-memory stand-ins for the seams, used by tests                                     |

### Backend

The runtime asks a backend to wait until it is available; list a pool's
containers and remove containers; prepare volumes; choose a slot's CPUs; run a
container under the run contract and report its exit code; name the containers it
has started and not seen end; and stop a set of containers within a budget. The JIT
configuration reaches the backend as a `Secret`, whose `Debug` form is redacted and
which has no `Display`. The container backend passes it as an environment variable
by name; a later backend that runs the containers in a virtual machine can pass it
another way. A fake backend implements the same seam for the tests of the code
above it.

The container backend runs `docker` with the PowerShell supervisor's argument
lists, byte for byte, except for the stop call below. A test reads the launcher
guard and the probe's reader script out of `docker-commands.ps1` and compares
them; the tests compare every other list with one written out in the test:

| Purpose                    | Command                                                                                                                                                                                                           |
| -------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Wait for Docker            | `docker info`, again every 5 s with a warning while it fails or cannot start                                                                                                                                      |
| List a pool                | `docker ps -a --filter name=<pool>- --format {{.Names}}`, then the same with `--filter label=runners.diagnostic-lifecycle=1`                                                                                      |
| Remove                     | `docker rm -f <names>`                                                                                                                                                                                            |
| Volumes                    | `docker volume create <name>` for each, then `docker run --rm --pull never --user root --mount type=volume,source=<name>,target=<path>... <image> chown runner:docker <paths>`                                    |
| Probe a slot's CPUs        | `docker run --rm --pull never --network none --name <pool>-cpu-probe-<slot> --label runners.cpu-probe=1 <image> bash -c <reader>`                                                                                 |
| Run a job container        | `docker run --rm --pull never --name <n> --cpus <q> --cpuset-cpus <set> --memory <G>g --memory-swap <G>g <--mount ...> -e ACTIONS_RUNNER_INPUT_JITCONFIG <image> bash -c <launcher> -- <set> /home/runner/run.sh` |
| Stop, for the later `stop` | `docker stop --time <seconds> <names>`, one call for all names                                                                                                                                                    |

The stop call differs from the supervisor's. `supervisor.ps1` runs
`docker stop <names>`, so Docker's default grace of 10 s applies. The backend always
passes `--time <seconds>`, the grace its caller chose, so no caller inherits a
default it did not pick.

The launcher is the guard script followed by `exec "$@"`. It reads
`Cpus_allowed_list` from `/proc/self/status` and exits 125 unless it equals the set
the container was given. A job container has no label, network option,
`--privileged`, socket mount, or `--user`. The JIT variable is passed to the one
`docker run` client that starts the container, in that client's environment, and
is removed from every other client's.

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
is forwarded to the log line by line, in pieces of at most 64 KiB; masking a
secret in forwarded output belongs to the logging that comes with `start`.

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
space, sign or leading zero. The PowerShell golden case is a test: the inventory
`2,5-6,11,14-15` with five slots of two CPUs gives `2,5`, `6,11`, `14-15`, `2,5`,
and `2,15`, and the rejected inventories are the PowerShell test's.

### Registration

A registration is `POST /repos/<owner>/<repo>/actions/runners/generate-jitconfig`
to `https://api.github.com`, with the PowerShell supervisor's headers
(`Authorization: Bearer <token>`, `Accept: application/vnd.github+json`,
`X-GitHub-Api-Version: 2022-11-28`, `Content-Type: application/json`) and body
(`name`, `runner_group_id` 1, and the pool's labels). It reads `encoded_jit_config`;
a response without one is a failure. It differs from PowerShell in four ways:

- it sends `User-Agent: runners/<version>`;
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

A slot loops until it is asked to stop. Each attempt probes the CPUs, then registers,
then runs the container. A failed probe or registration, or a non-zero exit, waits
5 s, then 10 s, doubling to 300 s; an exit of 0 resets the delay and starts the
replacement at once. A wait ends early when a stop is asked for, and a stop that arrives as a registration completes starts no container. Time and sleeping
come through a `Clock` seam, so the tests do not wait.

Preparing the pools follows the PowerShell supervisor. A pool whose token file is
missing or blank, and a pool with diagnostics on (this runtime does not support them yet), is
skipped with a warning that names the pool and the reason, and never the token.
Docker is waited for once. Each remaining pool's own stale containers are removed
and its volumes prepared; a pool that fails either is skipped. When no pool is left
there is nothing to run. Stale cleanup removes only containers that match the
pool's own pattern and do not carry the `runners.diagnostic-lifecycle=1` label. A
labelled container is never removed, and while one remains the pool is refused,
because its diagnostics are off. A failed removal is a warning only, and a failed
listing skips the pool.

### What the Tests Cover

`docker_backend.rs` runs the real backend against the `fake-docker` helper, a
program built by Cargo because Windows finds only `.exe` commands. It checks the
example configuration's argument lists, the JIT variable reaching only the `run`
client and no argument, the process groups, the probe limits, the stale cleanup and
the wait for Docker. `registration.rs` runs the client against a loopback stub for
the request, the error text, a redirect, a stalled response and an oversize
response. `slot_worker.rs`, `pool_preparation.rs` and `cpu_affinity.rs` use the
fake seams. `real_docker.rs` runs a container and the probe through a real Docker
daemon on Linux; it is `#[ignore]`d for a local `cargo test`, and CI runs it in its
own step, where it fails when Docker or the image is missing. Not verified: a real
Docker daemon outside that CI step, the TLS handshake with GitHub, and Windows and
macOS runtime behaviour beyond what the hosted runners show.

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
need drive-letter paths run only on Windows. Three more steps run on Linux only: the
base-address test again with `--release` and `RUNNERS_EXPECT_RELEASE=1`, which makes
it fail if built in the wrong profile; `docker build --target base` of
[`images/actions-runner`](../../images/actions-runner), tagged and never pushed; and
the `real_docker` test with `--ignored`, pointed at that tag by
`RUNNERS_REAL_DOCKER_IMAGE`. The Windows and macOS hosted runners have no Linux
Docker daemon, and the test fails rather than skips when one is missing. The
`fake-docker` helper is a second binary in the package (`cli/helpers/fake_docker.rs`,
`test = false`) that only the tests run, and `default-run = "runners"` keeps
`cargo run` on the CLI. A release MUST NOT ship `fake-docker`; the release phase
that packages the binary has to name `runners` alone. Tests find fixtures relative to the
crate's manifest directory, and the checkout-rule tests build a temporary directory
with the marker files at test time. The advisory check, `cargo deny --locked check advisories`,
fetches the RustSec database, so it runs in
[`rust-advisories.yaml`](../../.github/workflows/rust-advisories.yaml) and not in
`mise run check`; see [Development Workflow](./development-workflow.md).

## Dependencies

A new crate MUST NOT be added without an entry here, and every crate in
[`cli/Cargo.lock`](../../cli/Cargo.lock) has one. [`cli/deny.toml`](../../cli/deny.toml)
allows crates.io only, one version of each crate, and the licences below. The CLI
is unpublished, so it carries no licence field of its own; `deny.toml` ignores
private crates for that reason.

| Crate                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          | Why                                                                                                                                                                                                          | Licence                                                                                     |
| -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------- |
| `clap`, with `clap_builder`, `clap_lex`, and `anstyle`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         | Argument parsing, with default features off                                                                                                                                                                  | MIT OR Apache-2.0                                                                           |
| `regex`, with `regex-automata` and `regex-syntax`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              | The whole-value patterns for names, labels, and image names                                                                                                                                                  | MIT OR Apache-2.0                                                                           |
| `aho-corasick` and `memchr`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    | Optional dependencies of `regex`                                                                                                                                                                             | Unlicense OR MIT                                                                            |
| `serde`, with `serde_core`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     | Reading the parsed JSON tree and the shared expectations into derived types, with default features off                                                                                                       | MIT OR Apache-2.0                                                                           |
| `serde_derive`, `syn`, `quote`, and `proc-macro2`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              | The `Deserialize` derive; these run at build time                                                                                                                                                            | MIT OR Apache-2.0                                                                           |
| `unicode-ident`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                | A dependency of `syn` and `proc-macro2`                                                                                                                                                                      | (MIT OR Apache-2.0) AND Unicode-3.0                                                         |
| `tokio`, `tokio-macros`, `mio`, `bytes`, and `slab`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            | The async runtime for the slot loop and the container backend, on one thread, with only the `rt`, `time`, `sync`, `io-util`, `process`, and `macros` features; `tokio-macros` supplies `select!` and `join!` | MIT                                                                                         |
| `socket2`, `signal-hook-registry`, `libc`, `errno`, `smallvec`, `once_cell`, `cfg-if`, `log`, `itoa`, and `bitflags`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           | Operating-system and small-utility dependencies of `tokio`, `reqwest`, and their dependencies                                                                                                                | MIT OR Apache-2.0                                                                           |
| `pin-project-lite` and `atomic-waker`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          | Futures plumbing under `tokio` and `hyper`                                                                                                                                                                   | Apache-2.0 OR MIT                                                                           |
| `futures-core`, `futures-task`, `futures-channel`, and `futures-util`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          | Future and channel traits `hyper` and `reqwest` use                                                                                                                                                          | MIT OR Apache-2.0                                                                           |
| `reqwest`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      | The one HTTPS request to GitHub's JIT endpoint, with default features off and only `rustls-no-provider`, so no JSON, cookie, proxy-crate, or compression feature is built                                    | MIT OR Apache-2.0                                                                           |
| `hyper`, `hyper-util`, `http-body`, `http-body-util`, `tower`, `tower-http`, `tower-layer`, `tower-service`, `tracing`, `tracing-core`, `want`, and `try-lock`                                                                                                                                                                                                                                                                                                                                                                                                                                                 | The HTTP client stack under `reqwest`                                                                                                                                                                        | MIT                                                                                         |
| `http`, `httparse`, `base64`, and `ipnet`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      | HTTP types, header parsing, header encoding, and proxy address matching under `reqwest`                                                                                                                      | MIT OR Apache-2.0                                                                           |
| `sync_wrapper`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 | A dependency of `reqwest`                                                                                                                                                                                    | Apache-2.0                                                                                  |
| `url`, `form_urlencoded`, `percent-encoding`, `idna`, and `idna_adapter`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       | Parsing the request URL, under `reqwest`                                                                                                                                                                     | MIT OR Apache-2.0                                                                           |
| `utf8_iter`, `stable_deref_trait`, and `zeroize`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               | Dependencies of `idna` and the TLS stack                                                                                                                                                                     | Apache-2.0 OR MIT for `utf8_iter` and `zeroize`, MIT OR Apache-2.0 for `stable_deref_trait` |
| `icu_collections`, `icu_locale_core`, `icu_normalizer`, `icu_normalizer_data`, `icu_properties`, `icu_properties_data`, `icu_provider`, `litemap`, `potential_utf`, `tinystr`, `writeable`, `yoke`, `zerofrom`, `zerotrie`, and `zerovec`                                                                                                                                                                                                                                                                                                                                                                      | Unicode normalisation data for `idna`                                                                                                                                                                        | Unicode-3.0                                                                                 |
| `displaydoc`, `yoke-derive`, `zerofrom-derive`, `zerovec-derive`, and `synstructure`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           | Derives and their helper for the Unicode crates above; these run at build time                                                                                                                               | MIT OR Apache-2.0 for `displaydoc`, MIT for `synstructure`, Unicode-3.0 for the rest        |
| `rustls`, `hyper-rustls`, and `rustls-native-certs`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            | TLS for `reqwest`, with only the `ring`, `std`, and `tls12` features                                                                                                                                         | Apache-2.0 OR ISC OR MIT                                                                    |
| `tokio-rustls`, `rustls-pki-types`, and `rustls-platform-verifier`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             | TLS glue, certificate types, and the platform's certificate verifier                                                                                                                                         | MIT OR Apache-2.0                                                                           |
| `rustls-webpki` and `untrusted`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                | Certificate validation for `rustls`                                                                                                                                                                          | ISC                                                                                         |
| `ring`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         | The cryptography provider, installed explicitly                                                                                                                                                              | Apache-2.0 AND ISC                                                                          |
| `cc`, `shlex`, `find-msvc-tools`, and `getrandom`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              | Build-time C compiler discovery for `ring`, and its random-number source                                                                                                                                     | MIT OR Apache-2.0                                                                           |
| `subtle`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       | Constant-time comparison for `rustls`                                                                                                                                                                        | BSD-3-Clause                                                                                |
| `openssl-probe`, `security-framework`, `security-framework-sys`, `core-foundation`, `core-foundation-sys`, `windows-sys`, and `windows-link`                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | The platform certificate stores: the system store on Linux, the keychain on macOS, and the Windows APIs                                                                                                      | MIT OR Apache-2.0                                                                           |
| `schannel`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     | The Windows certificate APIs; Cargo records it but the three CI targets do not build it                                                                                                                      | MIT                                                                                         |
| `bumpalo`, `combine`, `jni`, `jni-macros`, `jni-sys`, `jni-sys-macros`, `js-sys`, `rustc_version`, `rustls-platform-verifier-android`, `rustversion`, `same-file`, `semver`, `simd_cesu8`, `simdutf8`, `thiserror`, `thiserror-impl`, `walkdir`, `wasm-bindgen`, `wasm-bindgen-futures`, `wasm-bindgen-macro`, `wasm-bindgen-macro-support`, `wasm-bindgen-shared`, `web-sys`, `winapi-util`, `windows-targets`, `windows_aarch64_gnullvm`, `windows_aarch64_msvc`, `windows_i686_gnu`, `windows_i686_gnullvm`, `windows_i686_msvc`, `windows_x86_64_gnu`, `windows_x86_64_gnullvm`, and `windows_x86_64_msvc` | Platform verifier and `getrandom` code for Android, WebAssembly, and other Windows targets, which the lockfile records and none of the three CI targets builds                                               | MIT OR Apache-2.0 (`same-file`, `walkdir`, and `winapi-util` are Unlicense OR MIT)          |
| `wasi`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         | A WebAssembly dependency of `getrandom`, not built on the CI targets                                                                                                                                         | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT                                         |
| `webpki-root-certs`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            | Bundled roots the platform verifier uses on WebAssembly, not built on the CI targets                                                                                                                         | CDLA-Permissive-2.0                                                                         |

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

Allowed licences are `MIT`, `Apache-2.0`, `Unicode-3.0`, `ISC`, and `BSD-3-Clause`; a
crate whose licence is an `OR` expression is accepted when one alternative is on that
list. `Unicode-3.0` is on the list because `unicode-ident`, which `syn` uses, is
`(MIT OR Apache-2.0) AND Unicode-3.0`, and an `AND` needs both sides allowed; the
maintainer approved it at plan revision 10. `ISC` and `BSD-3-Clause` are on the list
because the HTTPS client's TLS stack needs them: `rustls-webpki` and `untrusted` are
`ISC`, `ring` is `Apache-2.0 AND ISC`, and `subtle` is `BSD-3-Clause`; the maintainer
approved both in the Phase 2 plan (issue #46, revision 1). Adding another licence
requires the maintainer's review of it.

`cargo deny` checks the three CI targets only (`deny.toml` sets `graph.targets`). The
lockfile also records crates that none of them builds, including an older `syn` and
`windows-sys` beside the versions they build. One of the unbuilt crates,
`webpki-root-certs`, is `CDLA-Permissive-2.0`, which is not allowed. If a change makes
a CI target build it, `deny:rust` fails and the change needs the maintainer's review of
the licence.

`serde_derive`, `syn`, `quote`, `proc-macro2`, `displaydoc`, `yoke-derive`,
`zerofrom-derive`, `zerovec-derive`, and `tokio-macros` run at build time, with
`synstructure` as the helper of the `yoke` and `zerofrom` derives. A new
procedural-macro crate needs the maintainer's review; see
[Security](../conventions/security.md).

## Renaming the CLI

`runners` is provisional and is confined so a rename before it is published touches
only these places:

- the binary name in [`cli/Cargo.toml`](../../cli/Cargo.toml) (`[[bin]]`), the
  command name in `cli/src/command.rs`, and, if they change with it, the package
  and library names in `Cargo.toml` and `Cargo.lock` and the `runners_cli` imports
  in `cli/src/main.rs` and `cli/tests/`;
- `CARGO_BIN_EXE_runners` in `cli/tests/support/mod.rs`, and the default binary
  path in `tests/host-configuration/parity.ps1`;
- the `cli/target/debug/runners` path in the README Commands table, and the
  binary name and the example command in this document.

[Security](../conventions/security.md), [REVIEW.md](../../REVIEW.md),
[AGENTS.md](../../AGENTS.md), the [glossary](../glossary.md), and the README Layout
table say "the CLI", so a rename leaves them unchanged. The `runners` in the
`.cache/runners` directory, the login-profile markers in `.agents/setup`, and the
Amp continuation marker are the repository's name, not the binary's.
