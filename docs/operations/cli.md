# CLI

The Rust command line tool under [`cli/`](../../cli): what `validate` accepts and
prints, the rules it applies, the shared fixtures its tests read, the TOML
configuration format with its conversion from JSON and its comment-preserving write
path, the runtime layer that runs a host's pools (the container backend and the Tart
VM backend, JIT registration, CPU placement, and the slot loop, and on the Mac the
host prerequisites and the sleep and network signals), the instance layer that keeps
`start` to one instance per machine (the instance state, the single-instance lock,
and the control channel), the resident agent that takes the instance's place and its
protocol version 2 (the state it keeps, the token store, the messages, and the
snapshot), the image contract every runner image declares and the
check of it, running a host with `build` (the runner image on axpc, the golden VM on
axmc), `start`, `stop`, `restart`, `status`, and
`diagnostics`, the diagnostic containers and VMs a diagnostics-enabled pool runs and their
export, the toolchain and the gates that cover it, the app's workspace that builds on
the library and the gates it adds, how a release is built, signed,
and installed, and why its name is fixed. The binary is `marshal`. axpc runs its pools
with it, as [Windows Runner Host](./windows-runner-host.md) describes, including how
to roll back to the previous release, and axmc runs its Linux pools with it in Tart
VMs, as [macOS Runner Host](./macos-runner-host.md) describes.

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

| Exit | Meaning                                                                                                                                                                                                        |
| ---- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 0    | The configuration is valid. The plan summary is on standard output, with `\n` line endings.                                                                                                                    |
| 1    | The file is missing or unreadable, is not valid JSON, or breaks a rule. Every error is on standard error, in the format below and the order under Validation Rules, and nothing is printed to standard output. |
| 2    | A usage error, including no `--profile` on an operating system that has no default.                                                                                                                            |

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
path from being read whole. It is a rule of the CLI, pinned by
`a_file_over_the_size_limit_is_rejected_without_being_parsed` in
`cli/tests/command.rs`, which also checks a file of exactly 1 MiB is read.

The uniqueness checks compare every pair of entries, so their time grows with the
square of the number of entries, and it grows with the labels per entry too. On a
release build in a 4-CPU cloud container, 8,000 entries (about 0.8 MB) in distinct
repositories take about 1 s when nothing collides, and 8,000 entries in one
repository with a distinct label each take about 1.1 s. With many labels per entry it takes
longer: 1,500 entries in one repository with 50 labels each take about 3 s. Colliding
entries grow the error output with the square too, because every error is reported:
8,000 entries in one repository with the default labels print about 32 million error
lines and use about 8.4 GB of memory, and several collisions per pair or shorter entries
(the 1 MiB limit fits about 11,900) give more. The 1 MiB limit bounds both. A TOML
pool is shorter than a JSON entry, so the limit admits more of them; TOML Problems
gives the figures. Real configurations are expected to have about ten entries.

Known limitation: when `validate` cannot write its output, for example a closed
pipe, it exits 1, the same code as a rejected configuration. The behaviour is
unchanged on purpose; a caller that must tell the two apart reads the output.

## Profiles

A profile names a host. The format, order, and rules under Validation Rules apply
to both, and the shared fixtures pin the message text. The profile decides the
fields, the platform labels, the routing label, the path form, and the extra
checks.

| Aspect                  | `axpc` (Windows, Docker Desktop)                                         | `axmc` (macOS host)                                                                                     |
| ----------------------- | ------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------- |
| Platform labels         | `self-hosted, linux, x64`                                                | `self-hosted, linux, ARM64` for `os: linux`; `self-hosted, macOS, ARM64` for `macos`                    |
| Routing label           | `axpc`, the default when `labels` is absent or empty; a list replaces it | `axmc`, with the same default and replacement                                                           |
| Paths                   | Drive-letter or UNC                                                      | Absolute POSIX: start with `/`, no control characters, no empty, `.`, or `..` parts                     |
| Root fields             | `imageName` (required), `repositories`                                   | `budget` (required), `repositories`, `imageName` (optional; it names the runner image in the golden VM) |
| Entry fields            | The existing schema, with `volumes`                                      | The same without `volumes`, plus `os` and `diskGb`, both required                                       |
| Foreign platform labels | Accepted and listed, such as `ARM64`                                     | Rejected: another host's or OS's platform label, such as `macOS` on a Linux pool or `x64`               |

On `axmc`, `os` is `linux` or `macos`, `diskGb` is the per-VM disk cap, and
`budget` holds `cpus`, `memoryGb`, and `diskGb`. A VM has whole vCPUs, so `cpus`
is an integer from 1 to 64 on `axmc`, while axpc keeps its fractional CPU counts.
`volumes` is rejected on an entry even as `null`, because VM pools keep no volumes.
A change to what these fields accept MUST come with a recorded migration, which
[Migrations](#migrations) holds.

### New Rules

These rules exist on `axmc` only. Their messages follow the `<path>: <problem>`
form. A fixture under
[`tests/host-configuration/rust/`](../../tests/host-configuration/rust) pins each
rule. A change to the text MUST come with a recorded migration.

| Rule                                                  | Path and message                                                                                                                                                                    |
| ----------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `volumes` present                                     | `repositories[i].volumes: not supported on axmc, because VM pools keep no volumes`                                                                                                  |
| `os` missing, or not `linux` or `macos`               | `repositories[i].os: required field is missing`, or `...os: must be "linux" or "macos"`                                                                                             |
| `diskGb` missing or out of range                      | `repositories[i].diskGb: required field is missing`, or `...diskGb: must be an integer from 1 to 16384`                                                                             |
| `cpus` or `budget.cpus` not a whole vCPU count        | `repositories[i].cpus: must be an integer from 1 to 64`, or `budget.cpus: must be an integer from 1 to 64`                                                                          |
| `diskGb` below 20 on a Linux pool                     | `repositories[i].diskGb: must be at least 20 on a Linux pool, the golden VM's disk size`                                                                                            |
| `budget` missing, not an object, or a member unusable | `budget: required field is missing` or `budget: must be an object`; `budget.cpus` is a whole vCPU count from 1 to 64; `budget.memoryGb` is 1 to 1024; `budget.diskGb` is 1 to 16384 |
| A platform label the pool cannot provide              | `repositories[i].labels: '<label>' is a platform label that a <os> pool on axmc cannot provide, so list only non-platform labels`                                                   |
| More than two macOS VMs                               | `budget: macOS pools total N VMs (the sum of their slots), above the limit of 2`                                                                                                    |
| CPU, memory, or disk use above the budget             | `budget: pools total N CPUs (the sum of slots times cpus), above the budget of M`, and the same for `GB of memory` and `GB of disk`; see below for the disk reserve                 |

The budget errors come after every per-entry and cross-entry error. They sum
`slots` times the limit over the entries that join the cross-entry checks. The
macOS VM limit is fixed by the host, so it is checked even when `budget` itself is
unusable.

When any Linux pool is configured, the disk total also counts a fixed 40 GB for the
Linux golden VM and Tart's image cache, and the disk error says so:
`budget: pools total N GB of disk (the sum of slots times diskGb, plus 40 GB for the golden VM and Tart's image cache), above the budget of M GB`.
The summary's disk line includes the reserve and ends `, including a 40 GB reserve`.
Without a Linux pool the total and the message are the sum of slots times `diskGb`
alone.

### Migrations

A change to what `axmc` accepts is recorded here with each old and new rule, its
message, and the fixture that shows it. A configuration that validated before the
change may be rejected now; the last column says how to change it.

| Rule                          | Old                                                                                          | New                                                                                                                                                               | Fixture                                                                                                                                                                                                                                                              | To migrate                                                              |
| ----------------------------- | -------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------- |
| `cpus` of a pool              | A number above 0 and at most 64, fractions allowed                                           | An integer from 1 to 64: `repositories[i].cpus: must be an integer from 1 to 64`                                                                                  | [`axmc-fractional-cpus.json`](../../tests/host-configuration/rust/rejected/axmc-fractional-cpus.json), accepted before and rejected now                                                                                                                              | Round each fractional `cpus` to a whole vCPU count, at least 1          |
| `budget.cpus`                 | A number above 0 and at most 64, fractions allowed                                           | An integer from 1 to 64: `budget.cpus: must be an integer from 1 to 64`                                                                                           | [`axmc-budget-members.json`](../../tests/host-configuration/rust/rejected/axmc-budget-members.json), message changed; `axmc-fractional-cpus.json`                                                                                                                    | Round `budget.cpus` to a whole number that covers the pools             |
| `diskGb` of a Linux pool      | Any integer from 1 to 16384                                                                  | At least 20: `repositories[i].diskGb: must be at least 20 on a Linux pool, the golden VM's disk size`                                                             | [`axmc-linux-disk-too-small.json`](../../tests/host-configuration/rust/rejected/axmc-linux-disk-too-small.json), accepted before and rejected now, with a macOS pool at the same `diskGb` left unflagged                                                             | Raise a Linux pool's `diskGb` to 20 or more; a macOS pool is unchanged  |
| `imageName` of axmc           | Optional and validated as on axpc, with its use undecided                                    | Optional and validated as before; an absent name is `actions-runner:local`, and the name is the image that `build` makes inside the golden VM and every pool runs | [`axmc-image-name.json`](../../tests/host-configuration/rust/accepted/axmc-image-name.json), accepted before and now; nothing is accepted or rejected differently                                                                                                    | None; a configuration that validated still validates                    |
| Disk budget with a Linux pool | `budget: pools total N GB of disk (the sum of slots times diskGb), above the budget of M GB` | `budget: pools total N GB of disk (the sum of slots times diskGb, plus 40 GB for the golden VM and Tart's image cache), above the budget of M GB`                 | [`axmc-disk-over-budget.json`](../../tests/host-configuration/rust/rejected/axmc-disk-over-budget.json), message and total; [`axmc-budget-at-limit.json`](../../tests/host-configuration/rust/accepted/axmc-budget-at-limit.json), whose budget rose from 150 to 190 | Raise `budget.diskGb` by 40 GB, or lower the pools' `diskGb` or `slots` |

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
checkout is found, a note on standard error says the rule was not applied, and the
summary on standard output is the same with or without a checkout. The rule applies on
both profiles but can only bite where the profile's path form matches the
operating system: axpc on Windows, axmc on Linux and macOS.

For the agent the checkout is found from the sink itself. The agent's configuration
lives outside any checkout, so the checkout above the file would never exist: the
agent compares each `diagnosticDirectory` with the first directory at or above that
directory that holds both `.git` and `cli/Cargo.toml`, found the same way, lexically
and without resolving a link, and with the same per-profile case rule. Because it looks
from the sink, which directories hold `.git` follows the volume: on a volume that
ignores case, such as macOS's default one, a case variant of the checkout's path finds
the checkout and is refused, while on Linux it is another directory. It also applies
the checkout above its configuration file where one exists. A sink at or under either
is a configuration problem, `repositories[n].diagnosticDirectory: must be outside the
checkout`, in the snapshot, and the agent stops for `invalid_configuration`. The agent
looks from a sink only when the path is one of the host it runs on, so a Windows path
read on another host is not compared. Because the sink's own place was examined, the
agent writes no note that the rule was skipped. `start`, `validate` and `build` are
unchanged: their checkout stays the one above the configuration file.
`tests/checkout_rule_agent.rs` covers a sink inside a checkout, outside one, a sibling
prefix, a case variant (accepted on Linux, refused on macOS), and the unchanged rule of
`start`.

Known limitation: the axmc comparison is case-sensitive, while macOS's default APFS
volume is case-insensitive. For `start` and `validate`, whose checkout is the one above
the configuration file, a `diagnosticDirectory` that differs from the checkout only in
case therefore passes the rule even though it names the same directory. The validator
cannot know a volume's case sensitivity, so it does not guess. The agent is not
affected on such a volume: its lookup starts at the sink, and the volume itself answers
for the case variant.

## Validation Rules

The CLI and its fixtures alone define the rules. A change to a rule changes the
fixtures and their expectations in the same change; [Fixtures and
Tests](#fixtures-and-tests) says how they are read. `validate` is a pure function
from the file's bytes and a profile to a plan or an ordered list of errors, and it
records every error it finds rather than stopping at the first.

These rules read a JSON file. [The TOML Configuration](#the-toml-configuration)
applies the same rules to a second format and says what differs.

### Error Order

A missing file prints `Host configuration file not found: <path>`, another file
that cannot be read prints `Host configuration cannot be read: <path>: <problem>`,
and one that is not valid JSON prints
`Host configuration is not valid JSON: <problem>`; each is the only error.
Otherwise the errors come in the order the checks find them:

1. The root's unknown fields, in document order, each
   `<path>.<name>: unknown field`. Every object, entry and `budget` included,
   starts with its own unknown fields in the same way.
2. `imageName`, then on axmc `budget` and its members (`cpus`, `memoryGb`,
   `diskGb`), then `repositories` itself: missing, not a list, or empty.
3. Each entry in file order, all of one entry's errors before the next entry's. An
   entry's own errors follow its unknown fields in this order: `owner`,
   `repository`, `tokenPath`, on axmc `os`, then `labels`, `volumes`, `slots`,
   `name`, `cpus`, `memoryGb`, on axmc `diskGb`, then `diagnostics` and
   `diagnosticRawRecords` when they are not booleans, `diagnosticDirectory`, and last
   `diagnosticRawRecords: requires diagnostics`.
4. The cross-entry checks, among the entries that join them, as the Cross-entry
   checks bullet under
   [Behaviour That Is Easy to Miss](#behaviour-that-is-easy-to-miss) states. For
   each entry after the first, in file order, against each earlier entry in file
   order: a colliding `name`, then for the same repository a different token file
   or an ambiguous routing label, or for another repository a reused token file.
5. On axmc, the budget checks, over the same entries as step 4.

The exit code is 1 and standard output is empty whenever there is an error. A valid
configuration prints its summary on standard output, with lines ending in `\n`:

- **axpc.** The header `Host configuration is valid: <n> entries, image <name>`, a
  CPU policy line that only the axpc summary has, and for each entry its
  repository, slots, labels, token file, name, container name pattern, CPU and
  memory limits, planned affinity count and position, diagnostic settings, and
  volumes.
- **axmc.** The header `Host configuration is valid: <n> entries, profile axmc`,
  followed by `, image <name>` when the configuration names an image, and no CPU
  policy line. Each entry has its repository, `os`, slots, labels, token file,
  name, CPU and memory limits, `disk`, and diagnostic settings, but no container
  name pattern, affinity, or volume lines. A `Budget use` block ends the summary,
  with the macOS VMs, CPUs, memory, and disk used against the limit.

### Behaviour That Is Easy to Miss

- **Required and direct fields.** A field read as a required value reports `null`
  as `required field is missing`: `repositories`, `imageName` on axpc, and per entry
  `owner`, `repository`, `tokenPath`, `slots`, `name`, `volumes`,
  `diagnosticDirectory`, a volume's `suffix` and `mountPath`, and on axmc `budget`,
  its members, `os`, and `diskGb`. A field read directly reports `null` as its own
  type error: `cpus`, `memoryGb`, `labels`, `diagnostics`, and
  `diagnosticRawRecords`.
- **Names.** Known fields are found case-insensitively; the unknown-field check is
  case-sensitive, so `"Owner"` gives only `Owner: unknown field`.
- **Patterns.** A pattern matches the whole value, so a trailing newline does not
  pass.
- **Numbers.** Integer fields accept only an integer literal: `8.0` and `1e1` are
  rejected. Decimal fields accept integers, decimals, and exponents; on axmc `cpus`
  is an integer field instead. An axpc CPU count is rounded half away from zero at
  nine decimals on its 15-significant-digit form, and a value that rounds to zero is
  rejected. An out-of-range literal such as
  `1e400` gets the field's range error, not a parse error.
- **Cross-entry checks.** An entry joins them only when its owner, repository,
  token path, labels, volumes, slots, name, `cpus`, and `memoryGb` are all usable
  (on axmc: the same without volumes, with `os` and `diskGb`). A bad `cpus`,
  `memoryGb`, or `labels` excludes it. An unknown field, or a bad `diagnostics`,
  `diagnosticRawRecords`, or `diagnosticDirectory`, does not.
- **Strings.** Owner, repository, label, and name comparisons are case-insensitive
  over ASCII, which their patterns allow. Token paths compare under Unicode simple
  case folding. Every string in a `labels` list joins the duplicate set, including
  one that fails its own pattern.
- **Input.** The file is UTF-8 strict JSON. A leading byte-order mark is ignored.
  An exact duplicate key keeps its last value at the position of its last
  occurrence. Two keys that differ only in case, and an empty key, are errors. A
  value more than 128 levels below the root is an error, and so is a file over 1
  MiB (see Running It).

### Inputs the Fixtures Pin

Each input below is a rule of the CLI. Each is either a fixture under
[`tests/host-configuration/rust/raw/`](../../tests/host-configuration/rust/raw),
whose bytes MUST NOT be reformatted, or a test where the input is not a file.
`comment.json`, `trailing-comma.json`, `leading-zero-integer.json`,
`single-quoted-string.json`, `unquoted-property-name.json`, and
`lone-surrogate-escape.json` pin the strict-JSON row, `invalid-utf8.json` the UTF-8
row, and `root-array.json` the root row. The other inputs in the strict-JSON row have
no fixture. The case-folding rows are not files:
`fold::tests::simple_folding_equates_the_declared_pairs` in `cli/src/fold.rs` pins
them. The depth and size rows are tests, not fixtures:
`the_depth_limit_is_128_levels_below_the_root` in `cli/src/json.rs` and the size
test named under Running It.

| Input                                                                                                                                                                                                                                                                       | Rule                                                               |
| --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------ |
| A missing file, truncated or otherwise invalid JSON, two keys that differ only in case, an empty key                                                                                                                                                                        | Rejected with the CLI's own wording                                |
| A value more than 128 levels below the root, a file over 1 MiB                                                                                                                                                                                                              | Rejected                                                           |
| Invalid UTF-8                                                                                                                                                                                                                                                               | Rejected                                                           |
| A non-object root                                                                                                                                                                                                                                                           | Rejected, keeping the path in the header; a root array is rejected |
| Anything outside RFC 8259 (strict JSON): comments, trailing commas, single-quoted strings, unquoted property names, leading zeros (`01`), `.5`, `0x2`, `NaN`, no-break space or form-feed whitespace, a trailing NUL, U+2028 after the document, and lone surrogate escapes | Rejected with the CLI's own wording                                |
| Token paths that differ in a way simple case folding does not equate, such as canonically equivalent forms or an ignorable character                                                                                                                                        | Different paths                                                    |
| A `null` entry in `repositories`                                                                                                                                                                                                                                            | Rejected: `repositories[0]: must be an object`                     |
| The checkout rule                                                                                                                                                                                                                                                           | Applied on both profiles                                           |
| U+017F against `s` or `S`, U+0345 against U+03B9 or U+0399, U+1E9B against U+1E61 or U+1E60                                                                                                                                                                                 | Equal under simple case folding                                    |
| The Kelvin sign U+212A against `k` or `K`                                                                                                                                                                                                                                   | Equal under simple case folding                                    |

## The TOML Configuration

A host configuration can also be written as TOML, schema version 1. The library
reads that format with the same rules as JSON, converts a JSON configuration to it,
and changes an existing file while keeping its comments. No command reads, converts,
or writes TOML yet: `validate`, `build`, `start`, and the rest take JSON through
`--config`, as Running It describes, and only the tests call the functions below.
The agent's use of the format, which includes the migration that calls the
conversion, lands in the second pull request of issue
[#77](https://github.com/axross/runners/issues/77) and is documented with it. The
roadmap issue [#72](https://github.com/axross/runners/issues/72) names the later
consumers of the schema: W3 and M4 for `[lifecycle]`, and K3, X1, X2, and M2 for the
rest.

### Schema Version 1

The file is UTF-8 TOML. Keys are lower-case snake_case and case-sensitive. A key the
schema does not define for the host, a duplicated key, and a value of the wrong type
are problems at every level. A date or time is a wrong type, because no key holds
one. Any TOML form that gives the same tree reads the same: dotted keys, inline
tables, an inline array of tables, and hexadecimal integers.

| Top-level key    | Type                                    | Rule                                                                                                                                                                                                                                                  |
| ---------------- | --------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `schema_version` | integer                                 | Required, and MUST be `1`.                                                                                                                                                                                                                            |
| `routing_label`  | string                                  | Required, with no default. A pool that lists no labels carries it beside the platform labels, and a `labels` list replaces it. It follows the label pattern and is none of the platform labels of any host, and Labels applies to it as to a default. |
| `image_name`     | string                                  | Windows: required. macOS: optional, as `imageName` is on axmc.                                                                                                                                                                                        |
| `[budget]`       | table of `cpus`, `memory_gb`, `disk_gb` | macOS: required, under today's `budget` rules. Windows: an undefined key.                                                                                                                                                                             |
| `[lifecycle]`    | table                                   | Optional. Four optional booleans, below.                                                                                                                                                                                                              |
| `[[pool]]`       | array of tables                         | At least one. Each table is one entry of today's `repositories`.                                                                                                                                                                                      |

| `[[pool]]` key           | Type                                | Renames                          | Rule                                                                                                                                                            |
| ------------------------ | ----------------------------------- | -------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `name`                   | string                              | `name`                           | Required.                                                                                                                                                       |
| `repository`             | string                              | `owner` and `repository`         | Required, `"owner/name"`. Anything that is not one owner, a slash, and one name is `must be "owner/name"`; each part then follows the owner or repository rule. |
| `runners`                | integer                             | `slots`                          | Required.                                                                                                                                                       |
| `labels`                 | array of strings                    | `labels`                         | Optional. Absent or empty gives the platform labels plus `routing_label`.                                                                                       |
| `os`                     | `"linux"`, `"macos"`                | `os`                             | Optional on both hosts, and a pool without it is a Linux pool. Windows accepts `"linux"`, which changes nothing, and rejects `"macos"`.                         |
| `cpus`                   | number                              | `cpus`                           | Optional, with the JSON key's default and range.                                                                                                                |
| `memory_gb`              | integer                             | `memoryGb`                       | Optional, with the JSON key's default and range.                                                                                                                |
| `disk_gb`                | integer                             | `diskGb`                         | macOS: required, as `diskGb` is. Windows: an undefined key.                                                                                                     |
| `diagnostics`            | boolean                             | `diagnostics`                    | Optional, false by default.                                                                                                                                     |
| `diagnostic_directory`   | string                              | `diagnosticDirectory`            | Required with `diagnostics`, invalid without it, under the checkout rule.                                                                                       |
| `diagnostic_raw_records` | boolean                             | `diagnosticRawRecords`           | Optional, false by default, invalid when true without `diagnostics`.                                                                                            |
| `[[pool.volumes]]`       | tables of `suffix` and `mount_path` | `volumes`, `suffix`, `mountPath` | Windows: optional, and a pool without them has none. macOS: rejected.                                                                                           |

Every key follows the rule of the JSON key it renames on the host's profile:
whether it is required, its default, and its range. The shared rule set applies
those rules to both formats, so a later change to a profile's rules reaches TOML
too. [`hosts/windows-docker-desktop/README.md`](../../hosts/windows-docker-desktop/README.md)
describes what each Windows field means. The table lists only what TOML adds or
changes:

- **`os` and `volumes` may be omitted.** JSON requires `os` on axmc and `volumes` on
  axpc. A TOML pool without them is a Linux pool without volumes.
- **One `repository` key.** The owner is part of it, so a missing owner is a problem
  on `repository`.
- **No token key.** The file holds no token and no token path, so `token_path` is an
  undefined key. A pool's token is not a configuration setting, and the token-file
  rules of the JSON profiles do not apply to TOML.
- **`[lifecycle]`** holds four optional booleans, `start_at_login`,
  `start_runners_on_launch`, `keep_awake`, and `resume_after_stop`. Any other key in
  it is a problem, and an absent key means "not set". The rule set checks and keeps
  the values, and nothing acts on them yet: W3 on Windows and M4 on macOS decide
  what "not set" means and act on the rest.
- **Encoding and size.** A leading byte-order mark is ignored and moves no position.
  A file larger than 1 MiB (1048576 bytes) is refused before it is parsed, and a file
  that is not UTF-8 is refused before TOML sees it. The file may be exactly 1 MiB.

### Extending the Schema

Adding an optional key, or making a required key optional, keeps
`schema_version = 1`. An older program reports such a key as undefined, or its
absence as missing. Who may make a change without an answer from K1:

- W3 and M4 add keys to `[lifecycle]`. X1, X2, and K3 add them to a pool or at the
  top level. Each does it in its own pull request, and adds the key to this chapter.
- M1 and M3 change a profile's rules under their approved plans: a range, a default,
  whether a key is required, or an allowed value. Through the shared rule set the
  change applies to both formats and keeps version 1. Any other change of that kind
  needs the maintainer's approval, as a deviation request on the roadmap.

Removing a key, changing its meaning, or adding a required key raises
`schema_version`, and needs the maintainer's approval as a deviation request on the
roadmap. The agent that introduces the new version migrates an older file when it
starts. A program that finds a higher `schema_version` than it knows reports that a
newer Marshal wrote the file, and no other problem.

### TOML Problems

`validate_toml` records every problem it finds, in the order below, except when the
bytes are not a TOML document: a file over the size limit, one that is not UTF-8, and
a syntax error are each the only problem. Their forms are:

| Problem                         | Form                                                                       |
| ------------------------------- | -------------------------------------------------------------------------- |
| Any rule                        | `line <n>: <key path>: <problem>`                                          |
| A syntax error                  | `line <n>, column <c>: syntax error: <the parser's fixed message>`         |
| A syntax error with no position | `syntax error: <the parser's fixed message>`                               |
| A file that is not UTF-8        | `line <n>, column <c>: the file is not valid UTF-8`, at the first bad byte |
| A file over 1 MiB               | `line 1: the file is larger than 1048576 bytes (1 MiB)`                    |

Columns count characters from 1, and a CRLF file gives the same problems at the same
lines and columns as its LF copy, a syntax error at the end of a line included. A
syntax error names its line and column. `toml_edit` gives no position for a key of
more than 80 dotted parts, in a table header or before a value, so the reader finds
the first such key with `toml_parser`'s events and names where that key starts. A
syntax error that is still without a position names none, because a false position
would be worse.

A TOML pool can be shorter than a JSON entry, so the 1 MiB limit admits more of them.
About 20,200 pools of only `name`, `repository`, and `runners`, with the shortest
distinct values, fit beside the three header keys, and `validate_toml` takes about
4.8 s on them on a release build in a 4-CPU cloud container when none collide.
Colliding pools cost far more, because every problem is reported: 6,000 pools in one
repository (about 0.29 MB) take about 11 s and 4.4 GB of memory, and about 21,600 of
them, the most that fit, abort with a failed memory allocation under an 8 GiB
address-space limit. As for JSON, the 1 MiB limit is the only bound.

**Order.** The order is the one under Error Order, with the TOML key names:

1. `schema_version`. A version above 1 is `this file was written by a newer Marshal`
   and is the only problem. A missing one is `required key is missing`, and any other
   value is `must be the integer 1`.
2. The root's unknown keys, in document order. Every table starts with its own
   unknown keys the same way.
3. `routing_label`, `image_name`, and on macOS `budget` and its members.
4. `[lifecycle]`.
5. `pool` itself: missing, not an array of tables, or empty.
6. Each pool in file order, all of one pool's problems before the next pool's, in the
   order of the JSON keys they rename, after the pool's own unknown keys:
   `repository`, `os`, `labels`, `volumes`, `runners`, `name`, `cpus`, `memory_gb`,
   on macOS `disk_gb`, then `diagnostics` and `diagnostic_raw_records` when they are
   not booleans, `diagnostic_directory`, and last `diagnostic_raw_records` when it
   requires `diagnostics`.
7. The checks across pools, for each pool after the first against each pool before
   it: a colliding `name`, then an ambiguous routing label. Then on macOS the budget
   totals.

**Key paths and lines.** A key path is in TOML form, such as `pool[1].runners` or
`pool[0].volumes[2].mount_path`, with pools and volumes counted from 0, as the JSON
paths are. A problem names the line where the file holds its value. Three kinds have
no value to point at:

- A missing key names the line where its table starts: a `[[pool]]` or
  `[[pool.volumes]]` header, `[budget]`, or line 1 for the top level.
- A clash between pools, a name or a routing label, names the line where the later
  pool starts.
- A budget total names the line where `[budget]` starts, which is its first dotted key
  when the table is written that way.

**Wording.** A problem keeps the meaning it has under JSON and is worded in the
file's own names: TOML keys such as `runners`, `memory_gb`, `disk_gb`, and `pool`,
and Windows or macOS where JSON names a profile. A list is an array of tables or
labels, an object is a table, a field is a key, and `pool[0]` replaces
`repositories[0]`:

| JSON                                                                | TOML                                                                          |
| ------------------------------------------------------------------- | ----------------------------------------------------------------------------- |
| `repositories[0].slots: required field is missing`                  | `line 5: pool[0].runners: required key is missing`                            |
| `repositories[0].Owner: unknown field`                              | `line 7: pool[0].Owner: unknown key`                                          |
| `repositories[0].volumes: not supported on axmc, because …`         | `line 15: pool[0].volumes: not supported on macOS, because …`                 |
| `budget: macOS pools total 3 VMs (the sum of their slots), above …` | `line 4: budget: macOS pools total 3 VMs (the sum of their runners), above …` |

JSON problems, their text, and their fixtures are unchanged. The TOML problems are
new and are pinned by their own fixtures, under Fixtures and Tests.

**Syntax and type problems quote nothing.** A syntax error and a wrong-type problem carry only the
key path, the line, the column, and fixed text, so a token pasted into the file by
mistake is not repeated. The syntax error is built from the parser's message, which
is fixed text, and its byte span when it has one, from which the line and the column
are counted. It never uses the parser's own display text, which quotes the source line. Two kinds of
problem still show something the file holds: an unknown key is named as it is
written, which is how it is found, and a rule that compares a value names it, such as
a duplicate label, a platform label, a name that collides, or a suffix. A token in a
value of the wrong type, or on a line of its own, appears in no problem, and
`a_token_pasted_anywhere_in_a_document_is_echoed_only_as_an_unknown_key` in
`cli/tests/toml_fixtures.rs` pins that for a token inserted at every position of a
valid document.

### One Rule Set, Two Readers

A JSON reader and a TOML reader produce the same tree of values, and one rule set in
`cli/src/validate/` checks both:

```text
JSON bytes --JSON reader--+
                          +--> value tree --one rule set--> plan or problems
TOML bytes --TOML reader--+
```

The rule set records a problem as data: its key path as segments (a key, or a
position in a list), a kind with its values, and the location the reader gave the
key. Each reader renders the problem. The JSON reader renders today's path form and
words, byte for byte, which the shared fixtures pin. The TOML reader renders the form
and words above. What differs by format is what the reader supplies, through the
`Reader` trait:

- its `Format`, which fixes the rules that change together by format. `Format::Json`
  carries the profile's routing label: the file carries no settings, and a pool names
  its owner and repository as two keys and its token file, whose rules apply, and
  leaves out neither `os` nor `volumes` where its host has them. `Format::Toml` reads
  `schema_version`, `routing_label` and `[lifecycle]` from the file: a pool names
  `owner/name` in one key, has no token file, and may omit `os` and `volumes`, which
  gives a Linux pool without volumes, so a Windows pool also has an `os`;
- the path form of a pool, which a plan entry also records as its source path;
- which keys the format leaves undefined for the host, and where the file holds a key;
- how a problem reads. A rule that reads the same in both formats, apart from the
  host's name, the key names and a pool's path, is worded once in `rule_text`, which
  hands each reader the rest to word itself as a value it matches in full.

`validate` is the JSON entry point and `validate_toml(bytes, host, checkout)` the TOML
one. Both return a `Validation` with the same two outcomes: a `Plan`, or a `Failure`
that is either `Unreadable`, one line for bytes that are not a document, or `Invalid`,
the ordered problems. A plan from TOML carries `settings`: the schema version, the
routing label, and the four lifecycle values, each `Some` when the file sets it. Its
entries have no token path. A plan from JSON has no `settings`, and its entries keep
theirs. The `host` is a profile, and it selects the rules: Windows applies axpc's, and
macOS applies axmc's.

### Converting a JSON Configuration

`convert_to_toml(json, host, source, checkout, clock)` in `cli/src/convert.rs` turns
the bytes of a JSON configuration into the text of an equivalent TOML document, and
checks that text before it returns it. It reads and writes no file: the caller
writes the text. The agent's migration, which does, lands in the second pull request
of issue #77.

1. It validates the JSON under the host's JSON rules, which still require `tokenPath`.
   A rejected file is `SourceRejected`.
2. It writes the document, by structure. It sets `schema_version = 1`, and
   `routing_label` to the host's former label, `axpc` or `axmc`. It renames the keys,
   joins `owner` and `repository`, and leaves out `tokenPath`. It writes each key the
   JSON file set and no other, so a pool without `labels` still gets the routing label
   by default. It writes no `[lifecycle]`.
3. It validates the document with the full TOML rules under the same checkout. A
   document that fails is `DocumentRejected`.
4. It compares the document's plan with the JSON file's, and a difference is
   `NotEquivalent`, naming the key or the pool.

It returns the text only when both checks pass. A failure carries the reasons and no
document, so a document that failed its check cannot be written. The comparison,
`first_difference(original, converted)`, accepts a difference only in the source
path, in `tokenPath`, and in an `os` that TOML defaults to Linux where an axpc file
had none. It also requires the converted settings to be version 1, the host's routing
label, and an empty lifecycle.

The document opens with `# Converted from <file name> by Marshal at <time>.`, where
the name is the last part of `source` under either separator, never its path, with a
control character replaced by `?`, and the time is the UTC time of the `clock` the
caller passes. Lines end in CRLF for axpc and LF for axmc, whatever machine runs the
conversion.

### Changing the File

`cli/src/edit/` changes an existing `config.toml` and keeps what the change leaves
alone. Nothing calls it yet: W3 and K3 will, and the tests exercise it.

`edit_config(path, host, checkout, edits)` reads the file, applies the edits in
order, each against the document the ones before it left, and replaces the file with
the result. `ConfigFile::read` and `ConfigFile::edit` are the same in two steps, and
`rewrite` does the work on bytes without touching a file. An edit is one of:

- `Set`, a value at a key path: a boolean, an integer, a float, a string, or an array
  of strings such as `labels`. It replaces a value, or adds the key to a table that
  exists.
- `Remove`, a value. One that is not there stays absent.
- `CreateTable`, a table such as `[lifecycle]`. One that exists stays as it is.
- `AddEntry`, an entry of an array of tables such as `pool` or a pool's `volumes`,
  with its values. The array is created when it is missing.
- `RemoveEntry`, an entry of such an array.

A key path names keys from the root and a position for an entry, so `pool[1].labels`
is `Repositories`, `Index(1)`, `Labels`.

**What it keeps.** Everything the edits do not change survives: comments, key order,
blank lines, and how the other values are written, including dotted keys and inline
tables and arrays.

- A changed value takes over its old value's decoration, so a comment on its line
  stays. A removed value takes only its own lines, with any comment inside a value
  written over several lines, and a comment above it stays.
- A removed pool or volume takes its keys, a pool's volumes, and the comment lines
  directly above its header. When a blank line or the end of the file follows it, the
  blank lines above it go too. A comment set apart from the header by a blank line
  stays. In a list written inline, a removed entry takes the comment after it on its
  line and the comment lines directly above it; the other entries keep theirs, and
  so does a comment set apart from it by a blank line.
- An added entry goes after the last entry of its array, with a blank line before it.
  A comment that ends the file stays at its end. In a list written inline over
  several lines, an added entry goes on a line of its own after the last entry, with
  that entry's indentation, and the comment on the last entry's line stays there.
  An entry added to an empty inline list goes straight after its `[`.
- A new root table goes before the first `[[pool]]`, so a new `[lifecycle]` reads
  before the pools. When the file writes its pools inline, it goes at the end of the
  file.
- The parser reads LF and drops a byte-order mark, so the write path notes the file's
  mark and line ending before it parses, and writes the whole result with them. A file
  with mixed endings is written with its more common one, LF when the two are equally
  common. A last line that was not ended stays so. Lines it adds use the file's
  ending.

**Checks.** Before it writes, `rewrite` checks the result with `validate_toml` for the
host. Then, just before the replacement, the write path reads the file again and
compares it with the bytes it started from. This narrows, but cannot close, the
window in which an editor's save could be overwritten. An edit that changes nothing
writes nothing, so the file keeps its identity and its bytes, even when it had mixed
line endings.

**Errors.** `EditError` says which check stopped the write, and the file keeps its
bytes in each case:

- `Changed`: the file no longer holds the bytes the edit was made from, or is gone.
- `Invalid`: the result breaks the TOML rules, with the problems. A source that is
  not a TOML document, is not UTF-8, or is over 1 MiB is reported the same way, with
  the one problem `validate_toml` gives it, and so is a source that is valid TOML but
  breaks the rules and stays broken. Removing every pool is invalid.
- `Io`: the file could not be read, the new one could not be written, or it could not
  be put in place, for example because an editor holds the file open on Windows. It
  carries the path, which of the three failed, and the kind of error, and no file text.
- `Inapplicable`: the edit names a place the document lacks or of another kind, such as
  a table on the way that is missing, a `Set` on a key that holds a table, or an entry
  that does not exist, which is a mistake of the caller. It carries no file text, only
  the edit and the reason. Two refusals come from the file instead, and only the user
  can change it: a file that already holds the text the write path uses to mark a
  removal, and an edit inside an inline table written over several lines, which TOML
  1.1 allows, since its members' comments sit where the edit would move them. A caller
  shows these as a file to rewrite, for example that table on one line or as a
  `[table]`, not as its own error. Removing such a table from its list, or adding one,
  is not refused.

**One step.** The write path creates `config.toml.<32 lower-case hexadecimal digits>.tmp`
in the same folder, with a name that does not exist yet and, on Unix, mode 0600. It
writes the result and flushes the file to disk, compares the original again, and
renames the temporary file over `config.toml`. A reader therefore sees the old file or
the new one, never part of one.

- A temporary file that is not renamed is removed, whatever stopped the write. If the
  operating system refuses the rename, nothing else changes.
- On Unix the new file takes the old file's mode.
- The rename replaces a symlinked `config.toml` with a regular file, and leaves the
  link's target as it was. The new file holds the target's contents and mode.
- The write path does not flush the folder after the rename. The data is flushed
  before it, so a crash right after a write can leave the old file in place, but not
  a partial one.
- On Windows no mode is copied: the new file has the access its folder gives a new
  file.

`remove_leftover_temporary_files(folder)` removes what an interrupted write left. It
removes only regular files named exactly `config.toml.<32 lower-case hexadecimal
digits>.tmp`, so `config.toml.old.tmp`, a link, and a folder stay. It cannot tell a
leftover from the temporary file of a write in progress, so a process calls it once,
when it starts and before anything writes. Nothing calls it yet either.

## The Runtime Layer

The library holds the code that runs a host's pools: the container backend, JIT
registration, and the loop of one slot. `start` reaches it through the instance
layer, and the tests also drive it through its seams.

| Module                                                   | Holds                                                                                                        |
| -------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------ |
| `affinity`                                               | The CPU offsets `validate` and the backend share, and the selection of a slot's CPUs                         |
| `runtime::backend`                                       | The backend seam                                                                                             |
| `runtime::select`                                        | The runtime a pool's operating system selects on its host                                                    |
| `runtime::docker`, `runtime::docker_args`                | The container backend and the exact `docker` argument lists                                                  |
| `runtime::tart`, `tart::args`, `tart::client`            | The `tart` client: the exact argument lists, and the bounded process wrapper                                 |
| `runtime::tart::list`, `tart::names`                     | The reader of `tart list --format json`, and the VM names, parsers and matchers                              |
| `runtime::tart::backend`                                 | The Tart backend: the slot cycle's host steps, the probes, the job container, and the release                |
| `tart::golden_check`, `tart::guest`                      | The golden VM check from VM names alone, and the commands run in a booted VM through the guest agent         |
| `tart::boot`                                             | The wait for a booted VM's guest agent and the boot failure text, shared by a job VM and the build VM        |
| `tart::hold`, `tart::teardown`, `vm_shim`                | The holder of a VM's `tart run`, the stop-and-delete within a budget, and the VM shim                        |
| `golden`, `golden::pins`, `golden::key`, `golden::steps` | The golden VM's pins, the check of Docker's signing key, the launcher and the guest calls that provision it  |
| `macos::version`                                         | The tested Tart and Softnet versions, the readers of their `--version`, and the sorting                      |
| `macos::softnet`                                         | Finding Softnet on `PATH` as Tart does, the judgment of its file, and its version read                       |
| `macos::prerequisites`                                   | The host checks: Tart's version, then Softnet's file and version, each a pass, warning or refusal            |
| `runtime::registration`, `token`, `scrub`, `secret`      | JIT registration, the token file, error text, and the type that hides a secret                               |
| `runtime::clock`, `runtime::log`                         | The time source and the sink for the runtime's messages, each a seam the tests replace                       |
| `runtime::name`                                          | Runner and container names, and the test for a pool's own                                                    |
| `runtime::pool`                                          | Pools from a plan, and their preparation                                                                     |
| `runtime::worker`                                        | The slot loop and its backoff                                                                                |
| `runtime::job_marks`                                     | What a runner's output says about its job, idle, busy or unknown, which a drain reads                        |
| `runtime::retained`                                      | The diagnostic containers the backend retains, their records, and who finalises each                         |
| `runtime::diagnostic`                                    | Finalising diagnostic containers at job end, in the container steps, and at startup                          |
| `diagnostics`, `diagnostics::exporter`                   | The exporter child and the lines and exit codes it answers with                                              |
| `diagnostics::transport`                                 | The transport seam through which the exporter reads a container's evidence                                   |
| `diagnostics::sink`, `archive`, `identity`               | The sink checks and admission, the strict archive reader, and the identity readback                          |
| `runtime::board`                                         | Each pool's and each slot's state, which `status` reports                                                    |
| `macos::signals`, `macos::mac`                           | The signal-source seam and its real macOS source                                                             |
| `macos::sleep`, `macos::network`, `macos::watch`         | The sleep judgment, the `route` and `arp` readers, and the primary network's judgment                        |
| `macos::keep_awake`, `macos::probe`, `macos::reason`     | The `caffeinate` hold, the bounded system-program call, and the stop-reason texts                            |
| `macos::monitor`, `macos::gate`                          | The instance's signals on axmc: the watch, the keep-awake hold, and the gate that holds back the first clone |
| `macos::fake`, `macos::scripted`                         | The in-process signal source for tests, and the one a debug build reads from a directory                     |
| `runtime::fake`                                          | In-memory stand-ins for the seams, used by tests; a debug or test build only                                 |

### Backend

The runtime asks a backend to wait until it is available; check the configured
image, as [The Image Contract](#the-image-contract) describes; list a pool's
containers and remove containers; prepare volumes; prepare and release a job host,
as below; choose a slot's CPUs; run a
container under the run contract and report its exit code; name the containers it
has started and not seen end; stop, end, or remove a set of containers within a
budget; and, for a diagnostics-enabled pool, retain each diagnostic container it
starts with the settings it ran with, export one by a deadline, and count a pool's
bundles in its sink, as [Diagnostics](#diagnostics) describes. The runtime's own
finalisation, the same for every backend, decides whether to export and removes
the container after it. The JIT
configuration reaches the backend as a `Secret`, whose `Debug` form is redacted and
which has no `Display`. The container backend passes it as an environment variable
by name; the [Tart Backend](#tart-backend), which runs the containers in a virtual
machine, passes it on the standard input of the launcher in the VM. A fake backend implements the same seam for the tests of the code
above it.

The backend also provides two values the runtime core would otherwise take from the
container backend's modules: the command a job container runs after the launcher
verifies its CPUs, and the wait between checks while the engine does not answer.
The container backend gives `/home/runner/run.sh` and 5 s, so nothing on axpc
changes.

**Job hosts.** A job host is whatever one job runs on. A container on axpc runs
directly on Docker Desktop, so the container backend's host is nothing; a backend
that gives each job a virtual machine makes and deletes that machine as the host.
Two steps, per job and per slot, carry this:

- `prepare_job_host` runs after the image check and before any CPU probe or
  registration. It is given the slot, its vCPUs, memory and disk, and answers one of
  three outcomes. **Ready**: the host can take the job. **Failed**: the host could
  not be made, and the slot backs off like any other failed attempt. **Held**: a
  host prerequisite refuses this boot, with a reason. A held slot boots nothing,
  warns in the log, shows `held: <reason>` in `status`, and tries again on the same
  5 s doubling to 300 s schedule, and a job running on another slot is not
  affected. The reason `status` shows is scrubbed, joined onto one line and cut to
  500 characters followed by `...`, as a backing-off runner's is; the log line keeps
  the whole reason. A held slot is not a failure of the runtime core, and it counts
  for no restart. An engine that does not answer is not an outcome of this step: the
  backend waits for it in `wait_until_available`, or reports the image check as
  unavailable, as Docker does today.
- `release_job_host` is given a budget and gets rid of the host. It runs once after
  every preparation, whatever the preparation answered, and on every path from there
  on, before the slot backs off: after a failed or held preparation, a failed CPU
  probe, a failed or refused registration, a run that ended or failed, and a stop at
  any of those points. A diagnostic container is exported first, so the host is
  released after the export. A release that fails is a warning and changes nothing
  else.

The backend counts a host as prepared from the start of its preparation until its
release and lists the hosts it still counts, so the stop sequence can release the
ones whose worker could not: a release does nothing for a host that is not
prepared, and may run twice at once for one host. A backend also names the budget
the stop sequence reserves for its releases; the container backend has no hosts, so
its answer is zero.

The container backend prepares every host as ready without running a command,
releases nothing, and lists no host. Its argument lists, messages, and exit codes
are unchanged.

The container backend runs `docker` with the run contract's argument lists. The
image smoke tests run the same container commands through the helpers
`smoke-docker-commands.ps1` and `smoke-diagnostic-export.ps1` in
[`images/actions-runner/tests/`](../../images/actions-runner/tests). A test reads the
launcher guard and the probe's reader script out of `smoke-docker-commands.ps1`
and compares them with the backend's, so a change to either side alone fails the
CLI's tests; the tests compare every other list with one written out in the test:

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

The stop call always passes `--time <seconds>`, the grace its caller chose, so no
caller inherits Docker's default of 10 s.

The launcher is the guard script followed by `exec "$@"`. It reads
`Cpus_allowed_list` from `/proc/self/status` and exits 125 unless it equals the set
the container was given. The Docker runtime's job container has no network option,
`--privileged`, socket mount, or `--user`, and an ordinary one has no label. The VM
runtime's has the same list and one more option, a bind mount of the job VM's own
Docker socket, as [Tart Backend](#tart-backend) describes. The JIT variable is
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
`cli/src/image/contract.rs` fails when `smoke-docker-commands.ps1` no longer holds the
variant's argument list and launcher as the CLI writes them. The identity and copy
commands are held equal to `smoke-diagnostic-export.ps1` by a test in
`cli/src/runtime/docker_args.rs`, and the completion marker by one in
`cli/src/diagnostics/exporter.rs`.

Every client starts in its own process group: `process_group(0)` on Unix, and
`CREATE_NEW_PROCESS_GROUP` on Windows. A Ctrl+C in the terminal then reaches the CLI
only, which can stop the containers in order. A test reads the group of every client
the fake `docker` sees on Linux and macOS; no safe API reads a Windows process's
creation flags back. On Windows every client and the exporter child also get
`CREATE_NO_WINDOW`, so a resident agent that has no console, as the app's agent has
none, does not open a console window for each `docker` call. This applies to the
foreground commands too, since they share the builders. Their process group is
unchanged. Most clients have piped or closed standard streams, but on Windows two
children inherit the CLI's own: in terminal mode `build`'s `docker build` inherits its standard output
and standard error, and the exporter child inherits its standard error. How a terminal
run behaves on Windows with the flag, including `start`, Ctrl+C and whether a terminal
`build` still streams its output, is not established here: the Windows CI tests only
record the flags or run the CLI with piped output, so only the axpc run shows it. The one function that applies the flags is called by the client builder and
by the exporter's command builder, and a test build records what each applied, so
two unit tests assert both flags at the two builders, which every client and exporter
call site goes through. Whether Windows shows a window is not observable here and is
listed with the axpc checks. A client is killed when the future that waits for it is
dropped.

Every client other than a job container is bounded. A listing, a volume command,
and an info call are bounded at 120 s. A removal and a stop take the budget their caller passes; stale cleanup removes
within 120 s. The CPU probe and the removal of a leftover probe have the budgets
under CPU Placement. A `docker info` that fails, hangs, or cannot start (the
program is missing) means Docker is not available yet. Output from a job container
is forwarded to the log line by line, and the log masks every secret in it. A line
longer than 64 KiB goes in pieces, each cut where it splits no masked secret, the
bytes after the cut waiting for the next piece; see [The Log](#the-log).
The output of any other client is read within its budget, and when it becomes error
text it is joined onto one line and cut to 500 characters followed by `...`, as a
registration error is.

### Runtime Seam

A pool's operating system, on the host that runs it, selects its runtime. The
selection is `Runtime::of_pool` in `runtime::select`:

| Host | Pool               | Runtime                                                 |
| ---- | ------------------ | ------------------------------------------------------- |
| axpc | every pool (Linux) | The Docker Desktop container runtime, the backend above |
| axmc | `os: "linux"`      | The VM runtime, a Tart VM per job, run by `start`       |
| axmc | `os: "macos"`      | None: `start` skips the pool                            |

The backend above is the Docker Desktop runtime. The runtime core takes nothing from
the Docker modules: it drives the containers, and the per-job host steps under
[Backend](#backend), through the backend seam, and the run command and the engine
poll interval are values the backend provides. `start` constructs the backend the
host's profile selects: the Docker backend on axpc, and on axmc the
[Tart Backend](#tart-backend). The shim constructs the Docker backend directly.

The core runs a runtime that gives each job a virtual machine. A plan of axmc
Linux pools becomes pools: each slot's host has the pool's whole `cpus` as
vCPUs, the pool's `memoryGb` and the pool's `diskGb`, and the job container's CPU
quota and affinity cover all of the host's vCPUs, 0 to `cpus` - 1, with its memory
limit equal to the host's memory. The image defaults to `actions-runner:local` when
`imageName` is absent. `start` and the core apply that default, and `validate` and
its summary do not. The macOS pools of a plan are not made into pools. The VM
runtime is the [Tart Backend](#tart-backend), which `start` runs, and
`build --profile axmc` makes its golden VM:

- `start --profile axmc` runs the Linux pools on the Tart backend. Each macOS pool
  is skipped: the core logs `Skipping pool <pool>: macOS VM pools are not supported yet`
  once for it at each run, and `status` shows the pool as
  `skipped: macOS VM pools are not supported yet`. When every pool is skipped,
  nothing can start, and `start` exits 1 with
  `No pool can start, because macOS VM pools are not supported yet`, without
  checking Tart or Softnet. A restart or a resume that finds only macOS pools fails
  the core the same way, as any later run without a startable pool does. On a
  Windows host, `start --profile axmc` exits 1 first, before taking the lock or
  calling any engine, with `error: the axmc profile runs only on macOS`; a Linux
  host is refused as unsupported, as under [Start](#start).
- `build --profile axmc` makes the golden VM, as
  [The Golden VM Build](#the-golden-vm-build) describes. On a Windows host it exits 1
  first, before taking the lock, with `error: the axmc profile runs only on macOS`.

A refusal prints on standard error, as every `start` and `build` error before
the log opens. axpc is unaffected: its commands, messages, exit codes, and the run
contract keep their text.

The diagnostics export reads a retained container's evidence through the
`EvidenceTransport` seam in `diagnostics::transport`: the allowlisted identity
text, and the evidence archive, which the strict archive reader writes into the
bundle. The exporter child's admission, bundle files, limits, exit codes, and
deadline do not depend on the transport. `DockerCopy` runs `docker inspect` and
`docker cp` as [Backend](#backend) lists them. `VmCopy` runs the same two commands
inside a job VM as `tart exec <vm> docker ...`, with the same identity cap and
archive limits. The exporter child picks the transport from its arguments: with
`--vm <name>` it reads that VM through the Tart program it is given, and without it
it reads the container with `DockerCopy`, so axpc's exporter command line is
unchanged. Both backends start the child, take its admission lock, and read its
`admitted` and `bundles` lines through `runtime::exports`.

A unit test in `runtime::select` pins the table. `axmc_start.rs` runs a
configuration of Linux and macOS pools, and one of macOS pools alone, through
`start` as processes, unit tests in `profile` and `build` pin the Windows refusal,
and `golden_build.rs` runs the golden VM build. Unit tests in `diagnostics::exporter` write a complete bundle, a
gap, and the failed bundles of an unreadable identity and a failed archive through a
scripted transport, and the
diagnostics tests listed under [Diagnostics](#diagnostics) run the exporter through
`DockerCopy` with unchanged expectations. `tart_diagnostics.rs` and
`axmc_diagnostics.rs` run it through `VmCopy` against the fake `tart` and the fake
`docker` in its VMs.

### Tart Backend

`runtime::tart::backend::TartBackend` is the VM runtime. It gives each job a VM
cloned from the golden VM, runs the run contract's Docker commands inside it, and
deletes it. `start` constructs it for the axmc profile, with the VM shim as its
launcher; `build` does not use it. The in-process tests build it through
`cli/tests/support/rig.rs` against the fake `tart`, `docker` and `softnet`, and
`cli/tests/axmc_start.rs` runs it through `start`. The `tart` client library under it is documented where its
parts are: [Host Prerequisites](#host-prerequisites),
[Golden VM Provisioning](#golden-vm-provisioning), and the argument lists in
`tart::args`.

**Availability.** `wait_until_available` runs `tart list` and, while it fails, warns
`Tart is not responding yet (not installed, or not answering) - waiting.` and tries
again every 5 s. A Tart that does not answer is therefore never a failure of the
runtime core. A missing `tart` is refused at `start` before this wait is reached, as
[Host Prerequisites](#host-prerequisites) maps it. `check_image` runs the same listing within the image check's budget and
reads the golden VM from the VM names alone ([The Image Contract](#the-image-contract)
describes the check's outcomes). A listing that fails or overruns is
`Unavailable`, which registers nothing and goes into the backoff.

**The golden VM check.** `tart::golden_check` reads `tart list --format json` and no
VM's contents. A golden VM's name,
`marshal-golden-linux-g<golden VM version>-c<image contract version>-r<runner version>-<yyyyMMddHHmmss>`,
records what the image's labels record, so nothing boots. The newest golden VM by its
build second decides, whatever its versions. It matches when its golden VM version
and image contract version are this CLI's, and a runner version that differs is not a
mismatch, as for an image. It feeds the image check's outcomes, so the texts and
refusals are today's, naming the golden VM instead of an image: no golden VM is
missing, and one built for another golden VM version or contract version is
mismatched and asks for a rebuild. A unit test in `image::check` pins the case of a
golden VM whose old runner asks for a rebuild.

**A slot's cycle.** `prepare_job_host` takes these steps and answers `Ready`,
`Failed`, or `Held`:

1. A host this slot could not release earlier is released first, within 30 s, so a
   slot never owns two VMs. Its failure fails the preparation.
2. When the instance's signals have recorded no primary network, the preparation
   waits at the network gate first; a stop ends the wait. Then the prerequisites run:
   Tart's version, then Softnet's file and version, as
   [Host Prerequisites](#host-prerequisites) maps their causes, so a boot after a long
   wait uses a check made after it. A Tart that is
   missing or does not answer warns `Tart is not available (<message>) - waiting.`
   and tries again every 5 s; a refusal of Tart's version or of Softnet is `Held`
   with the refusal's text. The versions in use are logged once, as
   `Using Tart <v> and Softnet <v>.`, and again when they change; each warning is
   logged once until it changes.
3. The newest golden VM that runs here is listed and cloned to
   `marshal-job-<pool>-<slot>-<yyyyMMddHHmmssfff>`. The name is recorded before the
   clone runs, so a clone that fails halfway is still deleted.
4. The clone is sized with `tart set --cpu <vcpus> --memory <MB> --disk-size <GB>`
   from the pool's `cpus`, `memoryGb` and `diskGb`.
5. `tart run` (headless, with Softnet) is started by the [VM shim](#the-vm-shim),
   and the backend asks the guest agent whether it answers, every second with each
   question bounded at 5 s, until it does, `tart run` ends, or the 120 s boot
   deadline passes.

A boot that fails is `Failed`, with the text
`boot failure: <tart run for the job VM <vm> ended with exit code N (<Tart's own error>) | the guest agent of the job VM <vm> did not answer by the boot deadline (<last error>)>; a locked login keychain is the documented cause, see the "The Signed-In Session" section of the macOS Runner Host document`.
The section is [The Signed-In Session](./macos-runner-host.md#the-signed-in-session).

**Probes.** `select_cpuset` finds the slot's VM from the probe's name and waits up to
60 s for the Docker Engine in it (`docker info` every 2 s, each call bounded at
10 s). The wait ends early, with `CPU discovery: the job VM was released.`, once the
VM's release has begun. A stop ends it this way: the slot does not watch for the
stop here, and the release that the stop sequence runs deletes the VM. Then it runs the CPU probe's `docker run` through the guest agent within the 30 s the
probe has. The output is read as the container backend reads it, capped at 64 KiB,
ASCII only, and selected with the same function, so a VM's affinity is chosen from the
VM's own CPUs. A probe that fails registers nothing, and the cycle releases the VM. The
VM has the pool's whole `cpus` as vCPUs, so the quota and the affinity of the job
container cover all of them and its memory limit equals the VM's.

**The job container.** `run_container` runs
`tart exec -i <vm> /usr/local/bin/marshal-launch <docker run arguments>` where the
arguments are `docker_args::vm_job_run_arguments`, the container backend's own
`docker run` list with the VM's Docker socket added: `--pull never`, the
name, quota, affinity and memory options,
`--mount type=bind,source=/var/run/docker.sock,target=/var/run/docker.sock`, and
`-e ACTIONS_RUNNER_INPUT_JITCONFIG` with no value. The JIT configuration is written to the client's standard input and
the pipe is closed. It is in no argument list, in no environment variable of `tart`,
and in no log line. The launcher reads it from standard input, exports it, and
`exec`s `docker`. Standard output and error are forwarded to the log line by line
and masked, as for the container backend. A volume is refused in a VM
(`volumes are not supported in virtual machines`), and `validate` already refuses
one on axmc. The text a guest command returns to the backend is cut at 64 KiB; the
one exception is the diagnostics archive, which the Diagnostics paragraph below
streams through the strict archive reader and the bundle limits instead.

**The VM-local socket.** From image contract 2, the runner container mounts the Docker
socket of the VM it runs in, so a job step can run `docker` against that VM's own
Docker Engine. The socket belongs to a VM that one job uses and that is deleted
afterwards, and never to a host. The Docker runtime's list, `job_run_arguments`,
mounts none, and `vm_job_run_arguments` adds the mount to it and to nothing else:
the CPU probe and the volume commands still carry no socket mount. Tests in
`docker_backend.rs`, `tart_backend.rs` and `runtime::docker_args` hold both sides.
The socket's group is the VM's `docker` group, which the golden VM creates with
`golden::pins::DOCKER_GROUP_ID` and which the runner image's `docker` group matches;
[Golden VM Provisioning](#golden-vm-provisioning) says where each build checks it,
and [Security](../conventions/security.md#the-vm-local-docker-socket) says why the
match needs no `--group-add`.

**Which user runs it.** `tart exec` reaches the guest agent,
[cirruslabs/tart-guest-agent](https://github.com/cirruslabs/tart-guest-agent) at
commit `6d4bd9c`. Its systemd unit, `packaging/tart-guest-agent.service`, sets
`User=admin`, and `internal/rpc/exec.go` starts the command as the agent's own user
with no user switch, in a process group of its own that it kills when the call is
cancelled. A command therefore runs as `admin`. The golden VM adds `admin` to the
`docker` group, which applies from the agent's start, so `docker` runs without
`sudo` and the backend never calls it.

**Stop and release.** `stop_containers` and `kill_containers` run one `docker stop
--time <n>` or `docker kill` per VM through the guest agent, all in parallel, and
`remove_containers` runs `docker rm -f` in the container's VM. A name that is a job
VM, which only a leftover from an earlier run is, is stopped forcibly and deleted.
`release_job_host` gets rid of the slot's VM within the budget it is given:

- with a VM that is running, it asks for a graceful stop, stops it forcibly when that
  fails, deletes it, and, when the deletion fails, lists the VMs to see whether it
  is already gone. The budget is split in tenths: 5 for the graceful stop (its
  `--timeout` is a second less, so Tart ends its own wait first), 2 for the forced
  stop (`stop --timeout 0`), 2 for the deletion and 1 for the listing. A VM whose
  holder has ended is deleted without a graceful stop, and a deletion that fails
  then is retried once after a forced stop, because a killed VM shim leaves its
  `tart run` running and Tart refuses to delete a running VM;
- it is idempotent and safe to run twice at once: the second waits for the first
  and finds the host released;
- a release that succeeds forgets the host, its containers, and the holder of its
  `tart run`. A release that fails or runs out of its budget keeps all of them, so
  the next preparation of the slot or the next release deletes the VM.

**The release budget split.** The stop sequence of [Stopping](#stopping) takes the
backend's reserve out of its container steps. The Tart backend reserves 10 s
(`release_budget`), so the container stop is ended at 12 s with a grace of 7 s, the
removal takes 6 s more, and the release the remaining 10 s, which ends at 28 s, the
exit deadline; a diagnostic container's stop, export and removal end 6 s earlier for the
same reason. The container backend reserves nothing, so its steps are unchanged.
The 10 s is the CLI's: 5 s graceful, 2 s forced, 2 s deletion and 1 s listing. A VM
the CLI could not delete within it is deleted by its [VM shim](#the-vm-shim), which
has its own 15 s once the CLI has dropped the liveness pipe, even after the CLI has
exited. The worst path is 12 s + 6 s + 10 s = 28 s, with no slack, so a double fault,
a `tart` call that hangs to its bound, can leave a VM for its shim, which deletes it
within 15 s after `start` exits, or for the next `start`. A VM that neither deleted
is a leftover, and the next `start` deletes it.

**Leftovers.** At the start of a pool's preparation, `list_pool_containers` finds
the local VMs named `marshal-job-<pool>-<slot>-<17 digits>` for the configured
pools, and the runtime removes them, each by a forced stop and a deletion
(`deletes_leftovers_unexported`). Nothing is exported from a leftover VM. The log
says `Deleting job VM '<name>' left over from an earlier run.`; for a
diagnostics-enabled pool, it also says that its diagnostic evidence is gone, in
today's gap wording. A leftover that cannot be deleted skips its pool, as a labelled
container that remains does on the container backend: the warning says the pool "is
not started and gets no slots" and names the VMs, and the other pools start. Only the
pool's own VMs are matched, so another pool's, a golden VM and a build VM are left
alone.

**Diagnostics.** A diagnostic container is recorded as retained when it starts, and
the runtime finalises it, exporting before it removes the container, and releases
the host only after that, as the [worker's order](#names-and-the-slot-loop) has it.
For a VM, the export runs the exporter child with `--vm <name>`, which reads the
evidence through `VmCopy` (`tart exec <vm> docker inspect` and `docker cp`), with
the identity cap, the strict archive reader, and the bundle limits of
[Diagnostics](#diagnostics), and `count_bundles` counts the pool's bundles through
the same child. The VM is the retained container: it stays alive after its job
until the export has run, and the removal of a retained container is the deletion
of its VM, by the release above, within the removal's 24 s clock, where a container
backend would run `docker rm -f`. A failed deletion keeps the container retained
for `diagnostics`, as a failed removal does. The worker does not release a stopping
slot's diagnostic host itself (`host_awaits_finalisation`), because the stop
sequence exports it first and then releases it; without that, the slot's own
release would delete the VM while the export reads it. The warnings and gaps are
today's.

**Evidence a VM loses.** A diagnostic VM is deleted without an export in three
cases: a second SIGINT, which skips the export and deletes the VM where a container
would be killed and kept; a `start` that ends while it still holds the VM, whose
[VM shim](#the-vm-shim) deletes it when the liveness pipe closes; and the next
`start`, which deletes a leftover. The shim also deletes a VM that the CLI's release
could not delete before `start` exited; for a diagnostic VM the export was attempted
in the stop sequence before that release, so this is not a fourth case, but the shim
cannot tell that the export succeeded, so its gap line can name evidence that stands
in the sink. Each reports the loss with today's gap wording,
`Diagnostic gap in pool <pool>, job VM <vm>: <text>`, or, for a forced stop,
`container <name>` in place of the VM. A forced stop keeps the container names in
its result, so `start` exits 1 as for a kept container and its last line says that
the job VMs were deleted without an export. A container whose export had succeeded
is not among them: when a release deletes its VM, the backend forgets it, so that
line names only evidence that was lost. The forced stop's warning says the VM
"will be deleted" when the hosts are released, because the deletion can still fail.

Every VM of a diagnostics-enabled pool that is deleted in these cases is reported as
a diagnostic gap, whether or not a job ran in it, because the CLI cannot tell a VM
that holds evidence from an idle one without booting it. The next `start` reports a
leftover before it deletes it (`delete_leftover_host`), so a leftover whose deletion
then fails is still reported, and the VM shim reports a VM only once it has deleted
it, and none that was already gone.

### The VM Shim

The VM shim is the CLI run again as the hidden `marshal vm-shim --name <vm> -- <tart>`
on Unix. It starts `tart run` for the VM, with the arguments of `tart::args`, in a
process group of its own. Its standard input is the read end of a liveness pipe whose
write end only the CLI holds. It holds no lock: the instance lock is never passed to
it, and a test shows the lock free while a shim lives.

- When `tart run` ends while the pipe is open, the shim relays what Tart printed, its
  standard error and then its standard output, to its own standard output, and exits
  with Tart's code, or 128 plus the signal number when a signal ended it. It deletes
  nothing, because the CLI is still there and deletes the VM itself: a job VM when its
  slot releases it, and a build VM when the build fails (one that stopped after a
  successful build is renamed to the golden name instead). A VM the CLI could not delete is left to the next
  `start`, for a job VM, or the next `build`, for a build VM.
- When the pipe closes, the shim stops the VM (`stop --timeout 5` within 8 s, then
  `stop --timeout 0` within 3 s when that fails), deletes it within 4 s, and exits 0.
  It exits 1 when the deletion fails or the 15 s run out. A shim started for a
  diagnostics-enabled pool's VM (`--diagnostic-pool <pool>`) also writes
  `WARNING: Diagnostic gap in pool <pool>, job VM <vm>: <text>` to its standard
  error, which it shares with the CLI, once the VM is deleted. It cannot tell a VM
  holding evidence from an idle one, so it reports each VM it deletes, and none that
  was already gone. None of its `tart` calls
  prunes, which `TART_NO_AUTO_PRUNE=1` sets.

The shim takes any VM name, so the build command can hold a build VM the same way.
`HeldVm` is the CLI's end: it starts the shim, keeps the write end of the pipe, and
closes it when it is dropped.

### Host Prerequisites

The axmc runtime runs two programs it does not ship: `tart`, and `softnet`, which
Tart runs. `macos::prerequisites` checks both and returns a `Check`: a pass, a
warning, or a refusal that carries a `Cause` and its message. The
[Tart Backend](#tart-backend) calls it before each boot, `start`, `restart` and
the resume of a stopped instance call it before anything is started, and
[`build --profile axmc`](#the-golden-vm-build) calls it before it creates any VM. The tested versions are constants in `macos::version`, Tart 2.41.1 and Softnet
0.25.0, and the host document records them in
[Installing Tart and Softnet](./macos-runner-host.md#installing-tart-and-softnet);
a change of either is a change of the constant, the document, and the fake programs'
default output.

| Check             | How                                                                                                                                                                                                              |
| ----------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Tart's version    | `tart --version` through the Tart client, within 5 s (2 s at a restart or a resume). The output is exactly three dot-separated numbers and one optional final newline                                            |
| Softnet's file    | The first `softnet` on the `PATH` the Tart client carries, found as Tart finds it (see below); a regular file owned by root, with the SUID bit set and no write permission for its group or others               |
| Softnet's version | `softnet --version`, run on that file, within 5 s (2 s at a restart or a resume). The output is `softnet <three numbers>-<short commit>` and one optional final newline: clap's form for Softnet's release build |

Each version sorts into the tested version, which runs; a newer patch of the tested
minor version, which runs with a warning that names the tested version; or a refusal
that names the tested version and the host document's install section. An older
patch, another minor or major version, a fourth number, a suffix other than
Softnet's commit, and output with no version are refused. Each number has no sign and
no leading zero, and the commit is 7 to 40 lowercase hexadecimal digits. Softnet
0.25.0 is a patch of 0, so an older patch of its minor version cannot exist. The
version reads and the Tart client share one bounded process wrapper: their own
process group, killed when the call is dropped, standard input null, output cut at
64 KiB, and the JIT variable cleared. A `softnet` read gets the same `PATH`.

Tart takes no path for Softnet. It runs the first `softnet` on its own `PATH`, found
by `resolveBinaryPath` in Tart 2.41.1's `Sources/tart/Utils.swift`: each entry in
order, empty entries skipped, the first where the file exists with links followed.
It tests existence alone, so a directory named `softnet` matches, and a link that
leads nowhere does not. `macos::softnet::find_on_path` does the same and the file
check reads that file's type, owner and mode with the standard library's metadata,
which follows links. The judgment is a pure function over the file's type, owner and
mode (`setup_problem`); the first failing requirement in the order type, owner, SUID
bit, group write, other write gives the message. The file is judged before it runs,
so a file that fails never starts. The reader of owner and mode is Unix code; on
another platform it returns an error, which is a refusal.

A refusal carries a cause, so a caller can map it. A missing program or one that
cannot start is `Missing`, a program that does not answer within the bound is
`NotAnswering`, a version that does not run is `Version`, and a file that is not set
up is `Setup`. A missing `tart` or `softnet` at `start` or `build` is refused naming
the install section. A `tart` that does not answer, or goes missing after `start`, is
Tart unavailable and its slots wait. A `softnet` that does not answer counts as a
refusal.

**At `start`, `restart` and a resume.** The instance asks the core to check the
prerequisites once, through the backend's `check_prerequisites`, when the core runs
a plan with a Linux pool on axmc; a plan of macOS pools alone, and every axpc plan,
pass without a check. Every refusal of Tart's check but `NotAnswering` is a
refusal, and so is every refusal of Softnet's, a missing or unanswering `softnet`
included; a `tart` that does not answer is not, because the slots wait for it. Then:

- At a fresh `start` a refusal ends the instance before the runtime core runs: the
  log warns `The instance ends: <the refusal's message>`, which names the host
  document's section, and `start` exits 1. Nothing but `tart --version` and
  `softnet --version` has run, so no VM was cloned, set, or booted.
- At a `restart` or a resume, including a `start` that resumes a stopped instance, a
  refusal leaves the instance stopped with the refusal's message as its reason,
  exactly as an image that cannot run jobs does, and the client exits 1. The check
  runs before the image check. A restart has by then stopped every running job.
- The check at a first `start` gives each version read 5 s. At a `restart` or a resume
  a client is waiting, so each read gets 2 s and the two run together, which keeps the
  reload inside the client's wait, as [The Image Check](#the-image-check) counts. A
  `softnet` that takes longer than 2 s to answer is a refusal there, as one that takes
  longer than 5 s is at `start`.
- The warnings of a passing check, a newer patch of Tart or Softnet, are logged once
  and shown by `status` as `warning: <message>` lines, listed on every call, after
  the image lines, as of the latest check that passed.

After `start`, the Tart backend maps the causes this way before each boot: a Tart that is
`Missing` or `NotAnswering` makes the slot wait, a Tart `Version` refusal holds the
slot, and every refusal of Softnet holds it, a missing or unanswering `softnet`
included.

A debug build replaces the owner and mode read from Softnet's file with
`MARSHAL_TEST_SOFTNET_OWNER_MODE=<user ID>:<octal mode>`, such as `0:4755`, for the
process tests, because CI cannot create a root-owned file. The
file's type is still read, links followed. A release build ignores the variable, and
a debug build refuses a malformed value. `Check`'s tests are in
`cli/tests/host_prerequisites.rs`, against the fake `tart` and `fake-softnet`
(`cli/helpers/fake_softnet.rs`, documented in its header, which prints Softnet's real
form), and the parsers and the file judgment have unit tests in `macos`.

### Golden VM Provisioning

The `golden` module holds what `build --profile axmc` uses to provision the
golden VM; [The Golden VM Build](#the-golden-vm-build) sequences it. `golden::pins` records the base image by its
digest (`ghcr.io/cirruslabs/ubuntu`, tag `24.04`, arm64), the list of retired base
pins, Docker's apt repository and signing-key fingerprint, the four Docker Engine
packages at their pinned versions (Compose is not installed), the id of the `docker`
group (`DOCKER_GROUP_ID`, 123), and the launcher's path in the VM,
`/usr/local/bin/marshal-launch`. A change to the base digest, the
provisioning scripts, the launcher or the Docker Engine versions raises
`GOLDEN_VM_VERSION`, and a change of the base digest also adds the old one to the
retired list. A test asserts the current base is a digest reference and is not on
that list.

`golden::steps` builds each guest call as a `tart exec` command and the standard
input it takes, from scripts embedded in the binary. The calls run in this order:

1. upgrade the Ubuntu packages and install `ca-certificates`, `curl` and `gnupg`;
2. download Docker's key to `/etc/apt/keyrings/docker.asc`;
3. list the key with `gpg --show-keys --with-colons`;
4. install the apt source, create the `docker` group with the id `DOCKER_GROUP_ID` and
   refuse a group that has another, install the four packages at their pinned versions, hold
   them, and add `admin` to the group (`install-docker.sh`);
5. install the launcher, owned by root, mode 0755;
6. lock the `admin` password and mask `ssh.service` and `ssh.socket`.

`golden::key` compares the listing from step 3 with the recorded fingerprint. It
accepts only one primary key, listed with its fingerprint record, and refuses a
mismatch, a second primary key and anything it does not recognise. Its result is
the argument the call for step 4 requires, so nothing from Docker's repository is
installed before the comparison passes.

The launcher reads the JIT configuration from standard input to its end, sets
`ACTIONS_RUNNER_INPUT_JITCONFIG` in the environment of the `docker` client it
then `exec`s with its own arguments, and refuses an empty input or one over 64 KiB
with exit code 64, without printing the input. The Tart backend calls it as
`tart exec -i <vm> /usr/local/bin/marshal-launch <docker run arguments>`.

The runner image's Dockerfile installs `docker-ce-cli` at the version pinned here,
from the same repository, after the same key's fingerprint matches, and asserts that
its `docker` group has the id `DOCKER_GROUP_ID`; a unit test in `image::contract` holds the
Dockerfile and the smoke test to `golden::pins`: the `docker-ce-cli` pin, the key's
fingerprint and URL, the repository's URI and suite, and the group id. So the image's
client, key and group id cannot differ from the golden VM's `docker-ce-cli` pin, key and
group id.

`golden::steps::build_vm_docker_command` is the prefix the build uses to run `docker`
in the build VM, `sudo -n docker`, because the `docker` group applies only from the
guest agent's next start; see [The Golden VM Build](#the-golden-vm-build).

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
range or order, so a list with several defects is refused as invalid. A test pins
the golden case: the inventory `2,5-6,11,14-15` with five slots of two CPUs gives
`2,5`, `6,11`, `14-15`, `2,5`, and `2,15`, and
`the_rejected_inventories_are_rejected` in `cli/src/affinity.rs` lists the
inventories the selection refuses.

### Registration

A registration is `POST /repos/<owner>/<repo>/actions/runners/generate-jitconfig`
to `https://api.github.com`, with these headers
(`Authorization: Bearer <token>`, `Accept: application/vnd.github+json`,
`X-GitHub-Api-Version: 2022-11-28`, `Content-Type: application/json`) and this body
(`name`, `runner_group_id` 1, and the pool's labels). It reads `encoded_jit_config`;
a response without one is a failure. The client also:

- it sends `User-Agent: marshal/<version>`;
- it follows no redirect, so a redirect is a failure;
- it fails a connection after 10 s and a whole request after 30 s;
- it fails a response larger than 1 MiB, whether the size is declared or streamed.

Each failure goes into the slot's backoff. The error text has the token replaced by
`<token>`, whitespace collapsed, and is cut to 500 characters followed by `...`.
The token is read again from its file at every
registration, trimmed, and held in a `Secret`; the header that carries it is marked
sensitive. The HTTPS client is built without reqwest's `json` feature: the body is
three validated fields written by a small writer with the escapes RFC 8259
requires, and the response is read with the crate's own parser. The client keeps
reqwest's handling of the `HTTPS_PROXY` and `NO_PROXY` environment variables, and
does not read the Windows system proxy; no test covers the proxy variables.

TLS uses rustls with the `ring` provider, installed explicitly by
`install_tls_provider` before the client is built, and the platform's certificate
verifier. Installing twice is fine. A failure to install it or to build the client
is a setup error for the caller to report, never a panic. A test builds the client
on every operating system in CI; a handshake with GitHub, and a real registration,
are verified on axpc (maintainer's report,
[#56](https://github.com/axross/runners/issues/56#issuecomment-6511344317)).

The base address can be overridden only by a debug build, and only to
`http://127.0.0.1:<port>`, for the loopback tests; a debug build refuses any other
value. A release build ignores the request and always uses GitHub, so a release
binary cannot send a token anywhere else. `tests/base_address.rs` shows both halves,
and CI runs it again in the release profile.

### Names and the Slot Loop

A runner is named `<pool>-<slot>-<yyyyMMddHHmmssfff>` in UTC, and its container has
the same name. A name is recognised as a pool's own only when it is the
pool's name, a hyphen, a slot number, a hyphen, and exactly 17 ASCII digits.

A slot loops until it is asked to stop. Each attempt checks the image, then prepares
the job host, then probes the CPUs, then registers, then runs the container, then,
for a diagnostics-enabled pool, exports it, and finally releases the host. An image
that cannot run jobs, or a check Docker does not answer, registers nothing and goes
into the backoff. A failed or held preparation, a failed probe or registration, or a
non-zero exit releases the host and then waits
5 s, then 10 s, doubling to 300 s; an exit of 0 resets the delay and starts the
replacement at once. A wait ends early when a stop is asked for, and a stop that
arrives as a registration completes starts no container. Time and sleeping
come through a `Clock` seam, so the tests do not wait.

Preparing the pools puts the image check between the wait for Docker and the
pools' cleanup. A pool whose token file is
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
client and no argument, that no client is given a socket mount, the process groups, the probe limits, the stale cleanup and
the wait for Docker. `registration.rs` runs the client against a loopback stub for
the request, the error text, a redirect, a stalled response and an oversize
response. `slot_worker.rs`, `pool_preparation.rs` and `cpu_affinity.rs` use the
fake seams. `job_host.rs` scripts a failure at each step of the slot cycle on the
fake backend and shows the host released exactly once before the backoff, a failed
probe registering nothing, a held slot on the board retrying on the backoff
schedule, and the cycle's order; `pool_preparation.rs` also shows an axmc Linux
pool's quota, affinity, memory, disk and default image, and
`instance_state.rs` and `core_restart.rs` the release at a stop and at a core
failure. `image_check.rs` drives the image check through the fake backend,
the instance, and the real backend against `fake-docker`. The diagnostics tests are
listed under [Diagnostics](#diagnostics). `tart_client.rs` runs the `tart` client
against the fake `tart`: the exact argument lists, a run's options and Softnet rules,
the environment and the standard output every call gets, the JIT configuration
reaching the guest on standard input only, the 64 KiB cut, the budgets, and the VM
names and the listing reader. `host_prerequisites.rs` runs the prerequisite checks
against the fake `tart` and `fake-softnet`. `golden_provisioning.rs` runs the golden
VM's guest calls against the fake `tart` in their order, with a key of another
fingerprint stopping them before anything is installed, and `golden_launcher.rs` runs
the launcher as a shell script against a stand-in `docker` (Unix only).
`golden_install_docker.rs` runs the install-docker script as a real shell script
against stand-in system tools (Unix only): a missing `docker` group is created with
`DOCKER_GROUP_ID`, a group with that id is accepted with or without members, and a group
with another id is refused before apt runs.
`tart_backend.rs` runs the
[Tart Backend](#tart-backend) against the fake `tart`, which forwards a guest
command to `fake-docker` as the launcher would, and `fake-softnet`, through the rig
in `tests/support/rig.rs`: availability and the golden VM check, the cleanup of
leftovers, each step of a slot's cycle and its failures, the boot failure text, the
probes, the JIT configuration reaching the guest on standard input alone, the run
arguments and the VM's one socket mount, bounded and masked output, the stop and release within their budgets,
and a release run twice at once. `tart_diagnostics.rs` and `axmc_diagnostics.rs` run the
diagnostic VM cases listed under [Diagnostics](#diagnostics). `vm_shim.rs` runs the VM shim as a real process,
for a job VM and for a build VM, and `tart_slot.rs` runs the slot loop on the
backend with the fake programs, for the order of a cycle and what each failure does
to the VM and the registration. Both are Unix only. `real_docker.rs` runs a container and the
probe through a real Docker daemon on Linux, and exports a container's small
`metrics.txt` through the exporter child from a real `docker cp` stream; it is
`#[ignore]`d for a local `cargo test`, and CI runs it in its own step, where it
fails when Docker or the image is missing. Not verified by these tests: a real
Docker daemon outside that CI step, a real Tart, Softnet and guest agent (the fakes
stand in for them, and the guest user comes from the agent's source, not from a
run), the TLS handshake with GitHub, and Windows and macOS runtime behaviour beyond
what the hosted runners show. The handshake and a
real registration are verified on axpc instead, on the maintainer's report, as the
TLS paragraph above says.

### Signal Source

The axmc host stops every runner when the Mac sleeps or its primary network
changes, and holds off idle sleep while it runs. `macos` holds what reads those
signals, and on axmc the instance polls it
([Wiring](#wiring-the-signals-into-the-instance)). The seam is `SignalSource` in
`macos::signals`, which the instance polls for five things:

| Call                                                       | Answers                                                                   |
| ---------------------------------------------------------- | ------------------------------------------------------------------------- |
| `sleep_detected`, every 1 s                                | Whether the Mac slept, or its wall clock stepped, since the sample before |
| `rearm_sleep`, at each start of a run                      | Forgets the clocks sampled so far, so the next sample sets a new baseline |
| `read_network`, every 5 s                                  | A `NetworkReading`: the primary network, or `NoDefaultRoute`              |
| `hold_keep_awake`, `poll_keep_awake`, `release_keep_awake` | Taking the hold, checking it and retrying a lost one, and ending it       |
| `keep_awake_warning`                                       | The warning text while the hold is wanted and not held                    |

`MacSignals` is the real source. `FakeSignals` in `macos::fake` is the in-process
source the tests of the code above it use, a debug or test build only.

**Sleep.** Each sample holds the wall clock and an `Instant`. On macOS the
standard library reads `Instant` from `CLOCK_UPTIME_RAW`
(`library/std/src/sys/time/unix.rs` in the pinned toolchain's source), which does
not advance while the Mac sleeps, so the wall clock moves ahead of it by the length
of the sleep. `slept_between` judges two samples: a gap between the two clocks'
movements, in either direction, of more than 3 s is a sleep, and exactly 3 s is
not. A wall-clock step of that size is judged the same way, which errs on the safe
side. A sleep shorter than the gap goes unseen. The first sample of a
`SleepDetector` only sets its baseline.

**Primary network.** `route -n get default` gives the interface and the router, and
`arp -n <router>` the router's hardware address. Each call is bounded to 2 s, runs
in its own process group with standard input null, and has its standard output
capped at 64 KiB; more than that is refused. A release build runs `/sbin/route`,
`/usr/sbin/arp` and `/usr/bin/caffeinate` by absolute path. The readers accept only
the forms macOS prints, which are the ones `print_getmsg` in `route.tproj/route.c`
and `print_entry` in `arp.tproj/arp.c` write in Apple's `network_cmds`:

- `route` prints `route to: default` first, then `interface: <name>` exactly once,
  and `gateway: <IPv4 address>` at most once. The gateway line is absent when the
  route leaves through a point-to-point interface, as a VPN's does, and the reading
  then has no router and makes no `arp` call. A gateway that is a link
  (`link#<index>` of 1 to 9 digits) or a hardware address names the interface and no
  router, and reads the same way: the interface holds the default route. A gateway
  that is neither of those nor a routable IPv4 address, a second `interface:` or `gateway:` line, an interface name that is not
  1 to 15 letters and digits starting with a letter, or a failing exit code is
  refused. A refused table, a hung or missing `route`, and a table with no IPv4
  default route (`route: writing to routing socket: not in table`) are all
  `NoDefaultRoute`.
- `arp -n <router>` prints one line per entry,
  `? (<router>) at <address> on <interface> [ifscope] [permanent] [published (proxy only)] [<type>]`.
  The address is six lowercase hexadecimal bytes of one or two digits each, as `%x`
  writes them (`0:0:5e:0:53:1`). A line of another form refuses the whole answer;
  `(incomplete)` and `-- no entry` give no address; entries on other interfaces are
  ignored; entries on the route's interface that disagree give no address. An
  address that cannot be read leaves the reading with the router and no hardware
  address.

`NetworkWatch` in `macos::watch` is the pure judgment over the readings, built from
the first reading at start or resume:

- A primary network at the first reading is recorded at once. With none, the watch
  waits, and records a network only when it has held, as the same interface and
  router, on two checks in a row.
- A reading is the same network as the record when the interface and the router are
  equal and the hardware addresses are equal, or either is unread. A hardware
  address that was unread at the record is added to it the first time it is read.
- A reading that differs from the record counts, and one that matches resets the
  count. Two differing checks in a row are a change, even if the two differ from
  each other. The change names the record's interface and the latest reading's
  interface, or `none` when the latest has no default route. A watch that has
  reported a change keeps reporting it, and a resume builds a new one.
- Joining another network, a VPN that takes the default route switched on or off,
  and losing the default route for two checks are changes. Roaming between access
  points behind the same router is not.

`macos::reason` builds the two texts `status` and the `start` terminal show:
`the Mac slept, so every runner was stopped` and
`the primary network changed (<old interface> to <new interface>), so every runner was stopped`.
They name interfaces and never network names or addresses.

**Keep-awake.** `KeepAwake` runs `caffeinate -i -w <this process's ID>` as a child
with standard input and output null, in its own process group; `-w` ends it when
this process ends, however it ends. `hold` takes it, `release` ends it, and `poll`
checks it. A `caffeinate` that cannot start, or ends early, gives one `Warned`
event with the warning text, and `warning` returns the text until the hold is back.
`poll` retries a lost hold 10 s after the failure (`RETRY_INTERVAL`), and reports
`Recovered` at the first check after a retry that finds its `caffeinate` still
running, so a `caffeinate` that keeps ending warns once, not at every retry.

**Overrides.** A debug build reads these variables, and a release build ignores
every one, which `tests/release_overrides.rs` shows in the release profile for the
programs and the directory:

| Variable                  | Replaces                                                                     |
| ------------------------- | ---------------------------------------------------------------------------- |
| `MARSHAL_TEST_CAFFEINATE` | The `caffeinate` program                                                     |
| `MARSHAL_TEST_ROUTE`      | The `route` program                                                          |
| `MARSHAL_TEST_ARP`        | The `arp` program                                                            |
| `MARSHAL_TEST_SIGNALS`    | The whole source, with a directory a test writes into while the process runs |

A scripted source also runs the instance's cadence in a fiftieth of its time, so a
process test sees a network check every 100 ms; the ratio of five samples to a check
is the real one.

The directory holds a `sleep` file, which the next sample reports once and removes;
a `network` file, which is `none` or lines `interface=`, `router=` and `hardware=`;
a `keep_awake_failure` file, whose text is the hold's failure while it exists; and
the file `keep_awake`, which the source writes as `held`, `warning` or `released`.

The tests run the real code against the `fake-macos` helper, one program that plays
`caffeinate`, `route` and `arp` by their arguments, records every call's arguments,
process group and terminals, and answers as a test scripts it, including a failure,
a hang and a flood. `macos_signals.rs` covers the pure judgments, and
`macos_processes.rs` the readers, the bounds, the hold and the scripted source. Not
verified by them: the output of a real macOS `route` and `arp`, which the samples
copy from Apple's source (a VPN's `gateway: link#<index>` form included), and a real
sleep; both wait for a run on the Mac.

#### Wiring the Signals into the Instance

`instance::lifecycle::Monitor` is the instance's view of the signals; `Instance`
takes one with `with_monitor`, and without one, as on axpc, it asks for nothing.
`start` gives axmc a `macos::monitor::SignalMonitor` built on the `Signals` source
for the process, which is the real `MacSignals` or, in a debug build, the scripted
one.

- **Keep-awake follows the state.** `set_state` takes the hold on entering running
  and resuming, keeps it through stopping, and releases it on entering stopped and
  when the instance ends. `build` neither holds it nor reacts to the signals. A
  failed or lost hold warns once in the log, shows as a `warning` line in `status`,
  and is retried every 10 s; `Keep-awake is held again.` is logged when it recovers.
  The hold is checked every sample while running (inside the watch) and while
  resuming or stopping (the monitor's `maintain`, which `serve` polls beside the
  work it waits for).
- **Each run begins afresh.** When the instance enters running, at the first run and
  at every resume or restart, it calls `begin`: the sleep baseline is set again,
  because nothing sampled the clocks while it was stopped and the time spent
  stopped is no sleep, and the gate below is closed. The watch then reads the
  primary network and builds a new `NetworkWatch`, so a record from before a stop
  is never reused.
- **The watch.** One future per run samples the clocks every second and checks the
  network every fifth sample. A sleep, or two differing checks in a row, ends it
  with a `StopReason`: `Slept`, whose text is
  `the Mac slept, so every runner was stopped`, or `NetworkChanged`, whose text names
  the old and the new interface (`none` when no interface holds the default route).
  Dropping the future, as a stop does, ends the watch. The sleep is checked before
  the network in each sample, so a wake that also changes the network stops with the
  sleep reason.
- **The stop.** The reason ends the instance's running as `stop` does: the core is
  asked to stop, and the container steps and the release of every prepared job host
  run within the stop sequence's 28 s exit deadline, with the state `stopping`. The instance then moves
  to `stopped` with the reason, which `status` shows as `reason:` and the `start`
  terminal as `The instance is stopped: <reason>.`. A `stop` that arrives during the
  sequence ends the instance instead. The restart policy counts nothing, and only
  `start` or `restart` resumes; nothing resumes on its own.
- **No primary network.** `begin` closes the `NetworkGate` in `macos::gate`, and the
  first reading opens it when a primary network is recorded. The Tart backend waits
  at the gate in `prepare_job_host`, before the host prerequisites, so the boot that
  follows a wait of any length uses a check made after it, and a slot that waits has
  cloned nothing.
  Without a default route the gate stays closed until a network has held for two
  checks, and `status` shows
  `network: waiting for a network (no IPv4 default route)`. A stop ends the wait, as
  it ends a preparation, and the gate is open for a backend that was given none.

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
| `instance::windows`                       | The current user's security identifier, the only-user descriptor, and the pipe's options    |
| `instance::steps`                         | The container steps of a stop, shared by the foreground instance and the agent              |
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
- A resume and a `restart` check the host prerequisites, on axmc, and then the
  image, after the file validates and before the instance answers. A prerequisite
  that refuses leaves the instance stopped with the refusal's message as its reason;
  the instance also checks them once before its first run, and a refusal there ends
  the instance, so `start` exits 1.
- On axmc, a sleep and its wake, or a change of the primary network on two checks
  in a row, stops every runner as `stop` does, runs the container steps and the
  release of every job host, and leaves the instance stopped with the signal's
  text as its reason: `the Mac slept, so every runner was stopped` or
  `the primary network changed (<old interface> to <new interface>), so every runner was stopped`.
  It names interfaces only. Only `start` or `restart` resumes it, and a resume
  begins the signals afresh;
  [Wiring the Signals into the Instance](#wiring-the-signals-into-the-instance) has
  the rest.
- On axmc the instance holds off idle sleep while it is running or resuming and
  while it is stopping, and releases the hold on entering stopped.
- An image that cannot run jobs, which on axmc includes a missing or mismatched
  golden VM, leaves the instance stopped on a resume or a `restart`, with a reason
  that names the image and says to run `marshal stop`, then
  `marshal build --config <path>`, then `marshal start --config <path>`, because
  `build` is refused while the instance exists. When the engine does not answer the
  check, Docker or Tart, the instance answers as before and runs the core, which
  checks again once it answers and moves the instance to stopped with that reason.

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

**Releasing job hosts.** After the container steps, the instance and the agent
release every job host the backend still counts as prepared, all at once, each
within the backend's release budget: a worker releases its own host when its run
ends, so what remains is the host of a worker that is still busy or that a panic
took. The container backend has no hosts and a release budget of zero, so the stop
sequence above is unchanged. A backend that reserves a release budget `R` shortens
the container stop to fit the same 28 s exit deadline: the stop call is ended at
28 - 6 - `R` seconds, and its grace is 5 s shorter, never longer than 15 s. With a
`R` of 10 s, the grace is 7 s and the stop call is ended at 12 s, then the removal
at 6 s later, then the release at 10 s later, so the three take 28 s at the most
and `stop` still ends within 30 s. That leaves no slack: a `tart` call that hangs
to its bound in a double fault leaves its VM to the VM shim or the next `start`, as
[The release budget split](#tart-backend) describes. The diagnostic column ends
early by the same reserve, as [Diagnostics](#diagnostics) describes, so a
diagnostics pool's release keeps its whole budget too. The release itself splits
its budget, a graceful stop of the host first and a forced stop and deletion in its
last seconds; the backend chooses that split.

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
minutes old still counts and an older one does not. A clock that moves back
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
- **Windows, the agent's client.** The library's client of protocol version 2 reads
  the process ID of the pipe's server right after it connects and before it sends
  anything, opens that process, and compares its user's SID with the current
  user's. A server of another account, a process that cannot be opened or read, or
  any failed call is a refusal, `ServerNotThisUser`: the client drops the
  connection without sending a byte, so a `token_set` never reaches a squatter of
  another account. The check names no command and no token. A squatter of the same
  user is not told apart, and a process ID reused between the connection and the
  check is a short window; both are accepted on a single-user host.
  The commands of `start`, which speak version 1, are unchanged and still reach
  whatever holds the pipe, as the cost above says.

On both systems an ending instance first drops its request queue, so a request
still waiting is answered that the instance is ending, then stops accepting and
waits up to 0.5 s for every connection it accepted to end, so an answer it has
given, such as `stop`'s, is written before the process exits. A connection still
sending its request does not hold the exit longer than that.

The agent opens this same channel with the same names, descriptor, modes and
peer checks, but serves protocol version 2 on its own runtime without a thread per
connection, so its connection limits are timers on every operating system; see
[The Agent](#the-agent). A version 1 request reaches an agent as a first line that
is not `hello` and is refused as `unsupported_protocol`.

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
`tests/ci/control-pipe-security.ps1` has the helper open the control pipe, once as
the instance's channel (`serve`) and once as the agent's (`serve-agent`), connects
to it, reads the live pipe's descriptor through .NET, and fails unless the access
control list is protected and holds one entry, allowing the current user full
access. Neither crate the CLI uses reads a live pipe's descriptor without `unsafe`,
so that check is in PowerShell. A hosted runner cannot show the Windows lock
against a second OS user or the rejection of remote clients, and both are not yet
verified on axpc
([#56](https://github.com/axross/runners/issues/56#issuecomment-6511420845)):
neither check was run there, by the maintainer's decision.
Not verified: macOS and Windows behaviour beyond what the hosted runners show.

## The Agent

The library also holds the resident agent: the instance in a form that stays up and
is told what to do, instead of being started by a terminal and stopped by it. It is
the same process model as `start`: the agent takes the same single-instance lock,
opens the same control channel, writes a log in the same directory and, on Windows,
runs inside the same kind of Job Object, so `start` and the agent never run on one
machine at once, and a second agent is refused as a second `start` is. It has no
command or flag of its own: `agent::run` is the entry that wires the container
backend, GitHub, the token store, the lock, the channel and the log, and the app's
executable runs it in its hidden agent mode, as [App](./app.md#the-executable-and-its-modes)
describes. The CLI's tests start it through `instance-helper agent`. Only a debug
build lets a test replace the agent's token store, channel limits, watched file, log
size and `docker` environment; a release build ignores each
request, as it does the roots and the GitHub address, and `tests/agent_overrides.rs`
shows both halves and runs again in the release profile in CI.

| Module              | Holds                                                                               |
| ------------------- | ----------------------------------------------------------------------------------- |
| `agent::machine`    | The agent's state, and what each request, failure and changed executable does to it |
| `agent::core`       | The pools' slots, with each token read from the store and late pools started        |
| `agent::state`      | Where the agent is, why it is stopped, and what it knows of its configuration       |
| `agent::tokens`     | The pools' token source, read from the store, and the changes that start late pools |
| `agent::view`       | The snapshot, built from the state, the board, the retained record and the tokens   |
| `agent::api`        | The methods a connection can call and the errors they answer with                   |
| `agent::wire`       | The messages of protocol version 2 and the version and size constants               |
| `agent::server`     | One connection: handshake, requests, subscription, heartbeat, `ending` and timers   |
| `agent::channel`    | The pipe or socket the agent listens on, and the public client                      |
| `agent::snapshot`   | The value of the `state` topic and its fit to the message limit                     |
| `agent::log`        | The log file that is continued in a new file at 10 MiB                              |
| `agent::executable` | The check that the file the agent started from is still the same file               |
| `agent::builder`    | The seam the agent builds the runner image through                                  |
| `agent::run`        | `run_resident` on its seams, and `run` with Docker, GitHub and the platform's store |
| `token_store`       | The token store interface, the in-memory store, and the Windows Credential Manager  |

### Agent State

The agent is `running`, `resuming`, `stopping`, `stopped`, `building`, or
`quitting`, and one task owns the state, so a request that arrives during a
transition is answered at once, never queued behind it. It starts `running` or
`stopped` as the caller asks.

| Method             | Answer                                                                                                                                                                    |
| ------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `status`           | The snapshot, in any state                                                                                                                                                |
| `start_all`        | Stopped: reads the configuration again and checks the image, then answers `resuming`, or `stopped` with the reason it could not. Running: the state it is in              |
| `stop_all`         | Running: runs the stop sequence (the same grace, budget and exit deadline of 28 s as `stop`) and answers `stopped` when it is done. Stopped: `stopped`                    |
| `build`            | Stopped: answers `building` with the start time once the build began, and the snapshot tells the rest. Running: `refused`. A configuration that cannot be used: `refused` |
| `quit`             | Runs the stop sequence if runners run, cancels a build, answers `quitting`, and ends the agent; the lock is free within a second of the answer                            |
| `token_set`        | Stores the repository's token in any state                                                                                                                                |
| `token_remove`     | Removes it in any state                                                                                                                                                   |
| `drain_for_update` | See [The Update Drain](#the-update-drain)                                                                                                                                 |

A request that arrives while the agent is `resuming`, `stopping`, `building` or
`quitting` and is not one of `status`, `token_set` and `token_remove`, a stop or a
quit, is answered `busy`, except `drain_for_update`, which is `refused` there. The
agent is `resuming` from the moment it begins to read
the configuration until the runners' pools are admitted, so a `start_all` or a
`build` in that time is `busy`, not answered as if the runners ran. One table in the
code decides all of it, for every phase; this is what it says, and while a drain runs
the table in [The Update Drain](#the-update-drain) takes its place:

| State      | `stop_all`                           | `start_all` | `build`                                          | `quit`                     |
| ---------- | ------------------------------------ | ----------- | ------------------------------------------------ | -------------------------- |
| `running`  | Stops, then `stopped` (by you)       | No change   | `refused`                                        | Stops, then the agent ends |
| `stopped`  | No change                            | `resuming`  | `building`, or `refused` without a configuration | The agent ends             |
| `resuming` | Abandons it, then `stopped` (by you) | `busy`      | `busy`                                           | Abandons it, then ends     |
| `building` | Cancels it, then `stopped` (by you)  | `busy`      | `busy`                                           | Cancels it, then ends      |
| `stopping` | No change                            | `busy`      | `busy`                                           | Ends after the stop        |

The agent stays up when something is wrong; it stops its runners and says why in the
snapshot, as the reason of the `stopped` state:

| Reason code             | When                                                                         |
| ----------------------- | ---------------------------------------------------------------------------- |
| `by_you`                | A `stop_all`, a `quit`, or an agent launched stopped                         |
| `no_configuration`      | There is no file at the configuration path                                   |
| `invalid_configuration` | The file is not valid for the axpc profile; the problems are in the snapshot |
| `image_unusable`        | The image is missing, or its contract version is not this CLI's              |
| `no_pool_started`       | No pool could start, such as every pool without a token                      |
| `restart_limit`         | The runtime core failed too often, under the same restart policy `start` has |
| `image_build_failed`    | A `build` failed; the build's message is in the snapshot                     |

The answer to a `start_all` follows the configuration read and the image check; it
is not sent first. The check is one `docker image inspect` and, when that fails, one
`docker image ls`, which share a bound of 4 s (the image check budget), so the answer
comes within about that time and the pools are admitted after it. A client that
needs a sooner answer bounds its own call and watches the `state` topic for the
rest.

After a `build` that succeeds the agent checks the image once more, as part of the
build's own outcome: the check refreshes the snapshot's `images` entry with the new
image's labels and warnings, and it does not change the `outcome`, which is the
build's. The check runs while the agent is still `building` and answering: `status`
shows the build, `start_all` and `build` are `busy`, and a `stop_all` or a `quit`
taken during the build or during the check skips or abandons the check, which leaves
the `images` entry as it was; a build that still commits after the stop stays `ready`.
A `build` that succeeds and is not cancelled leaves the agent stopped by you
(`by_you`), whatever it was stopped for before, since the build read the configuration again and the
snapshot's problems and pools are those of that read. Its outcome, `ready`, is only in `agent.build`,
and the refreshed image entry is in `images`: "stopped with the image ready", in
the state table above, is that outcome and not a reason. A build that fails
stops the agent for `image_build_failed`, and a cancelled one is stopped by you.
The app's rule to restart the agent when the last state it heard was stopped by you
is unchanged. A `start_all` answers
`resuming`, which stays true until the pools are admitted: the snapshot's `state`
becomes `running` once they are, or `stopped` with one of the reasons above when
they could not be.

The set is open: a reader keeps a code it does not know as text. The library exports
the codes as the constants of `agent::StopReason`, so a reader compares a reason with
them rather than repeating the words. A pool skipped for
want of a token does not stop the others: it is listed `skipped` with its reason,
and when a token for its repository is set, it starts without a restart. When no
pool is left running, the agent is stopped for `no_pool_started`, and each pool
keeps its reason in the snapshot, listed `stopped`.

The agent ends by itself when the file it started from is moved, renamed, replaced
or deleted, so it never goes on running a program that is no longer the installed
one. It tells the file by its size, its times and its identity on the volume (device
and inode on Unix; volume serial number and file index on Windows, read through a
handle opened for no access with every share mode, so neither the running program nor
an installer is blocked). It looks once a second and ends within 5 s, after the same stop sequence as
`quit`; the `ending` message names the reason `executable_changed`, where a `quit`
names `quit`. The agent does not handle a sign-out or a shutdown: it gets no event
for them, as `start` gets none on Windows, and the next agent removes what they left.

The configuration is `config.json` in the `marshal` folder of `%LOCALAPPDATA%` on
Windows and in `~/Library/Application Support/marshal/` on macOS, a folder that must
be a directory owned by the user with mode `0700`, the folder the socket is in. A
debug build can move it with a test root. The agent reads the file when it starts
running and at each `start_all` and `build`, with the axpc profile; it does not
read a token file, and the `tokenPath` of each entry, which the commands of `start`
still read, is checked by `validate` and ignored by the agent.

The agent's log is a file in the same private directory as `start`'s. It is
continued in a new file when it reaches 10 MiB, so no line is lost to the limit, and
the newest ten files are kept, and a warning in the file being written says when an
old one could not be removed. Every token and JIT configuration is masked in it, and
nothing goes to a terminal, since the agent has none: a failure before the log opens
goes to standard error, and everything after goes to the log. A `docker build` the
agent runs has its output read line by line into the log, each line masked and
marked `docker build:`; a longer line is cut at 8 KiB, or before a masked value the cut would split,
marked with `...`, and the rest of it dropped, so it is logged once and no part of a
masked value is kept, and the last five
lines, masked and each cut at 70 characters, end the message of a failed build, so the 500-character cut
of the message never takes the newest line. `marshal build` itself still passes the build's
output on to its terminal.

A log that cannot write or continue its file stops, and the agent goes on without
it. It tries to open a new file again when a line arrives, at most every 30 s, and
writes one line in the new file that says how many lines were not written.

### The Update Drain

W4 ([#78](https://github.com/axross/runners/issues/78)) adds `drain_for_update` to
protocol version 2, under its rules for added methods and fields. Before the app
installs an update, it asks the agent to drain: to stop registering runners and to end
once the jobs already running have finished, within 6 hours. The `version` param is
the version of the update, three numbers and an optional suffix, at most 64
characters in all; anything else is `invalid_request`.

A runner is busy once its container's output has carried a line that reads
`Running job:` after the runner's timestamp, and idle once it has carried
`Listening for Jobs` and not `Running job:`. A runner whose output has carried
neither line is waited for, as if busy. That fallback covers a runner that prints
neither line, not one that still prints `Listening for Jobs` but rewords or drops
`Running job:`: its busy runners would read as idle and be stopped mid-job, so
[Bumping the Runner](./development-workflow.md#bumping-the-runner) checks both lines.
Both streams count, a mark never clears, and
no other line, such as `Runner reconnected.`, changes it. Only the fixed text is
compared: the drain keeps no job name and puts none in the snapshot or a protocol
message; the line itself reaches the log, as all runner output does.

The drain's last steps begin when the drain ends its runners: when no runner is waited
for any more, when the 6 hours pass, or when the runtime core ends for the drain or
fails. In them the container steps run and the agent then ends for the update; the
drain can no longer be cancelled, because its runners are already stopping.

| When                                                     | Result                                                                                                                                                 |
| -------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `drain_for_update`, while `running`                      | Drains: the answer is `{state, update}`, and `agent.update` appears                                                                                    |
| `drain_for_update`, while `stopped`                      | Ends for the update at once; `stopped_by_you` is true when stopped by you                                                                              |
| `drain_for_update`, in any other state or while draining | `refused`, and nothing changes                                                                                                                         |
| `start_all` or `build`, while draining                   | `refused`, and nothing changes                                                                                                                         |
| `stop_all`, while draining                               | Answers `quitting`, also during the drain's last steps; stops every runner, so running jobs fail, then ends for the update, stopped by you             |
| `quit`, while draining                                   | Stops and ends as a `quit` does; `agent.update` is gone once the stop begins or, during the drain's last steps, at once, and the `ending` names `quit` |
| The connection that asked closes, while draining         | Before the last steps, the drain is cancelled, the agent keeps running, and the slots register again; during them, the agent still ends for the update |
| A runtime core failure, while draining                   | The container steps run, so running jobs fail; the core does not start again, and the agent ends for the update                                        |
| No runner is waited for any more                         | The container steps run, then the agent ends for the update                                                                                            |
| 6 hours pass                                             | The container steps run, so the jobs still running fail, then the agent ends                                                                           |

While the agent drains, nothing registers a runner: not a slot, not a pool prepared
after a core failure or when Docker comes back, and not a pool that waited for a token.
A slot that is registering or backing off stops without starting a container, an idle
runner's container is stopped with a stop's grace, and killed when the stop fails,
and a busy runner's container runs
until it exits; its slot then finalises it as after any job and registers nothing. A
drained slot is listed `stopped` with the reason "stopped for an update". The agent's
state stays `running`, so an older app shows it running; `agent.update.waiting` counts
the slots still running a container, an idle one until its stop ends, `deadline` is the bound's time in milliseconds since the
Unix epoch, and `pid` is the agent's process ID, which the app waits on. Before the
agent ends for an update, every connection receives `ending` with the reason `update`
and an `update` field naming the version and whether the runners were stopped by you.
A `drain_for_update` while stopped answers `{state, update}` with the state
`quitting`, so the app has the process ID it waits on. If the requesting
connection's close is seen before the core ends for the drain, the drain is cancelled
and the core starts again; if the core's end is seen first, the last steps have begun. Only the
container backend reads a runner's output today; a backend that does not, such as
axmc's Tart backend, reports every runner as waited for, so its drain ends at the
bound.

### The Token Store

`token_store` is the one place the agent keeps a repository's token. The interface
names no operating system: `get`, `set` and `remove` by a repository key, which is
`owner/name` in lower case, so `Axross/Runners` and `axross/runners` are one token.
A token is trimmed, then refused when it is empty or longer than 1280 UTF-16 code
units, which keeps it inside the Credential Manager's 2560 bytes.

- **Windows.** The Credential Manager of the current user, through
  `windows-native-keyring-store`, as one generic credential per repository named
  `app.axross.marshal:github.com/<owner>/<name>`, with the token as UTF-16 and with
  local persistence. The crate's default persistence is the enterprise one, which
  roams with the user's profile, so the store asks for the local one.
  `tests/token_store_windows.rs` sets, reads and removes a real credential, reads
  its persistence back from the store's `persistence` attribute, round-trips a token of 1280 UTF-16 units and
  a non-ASCII, non-BMP one, and sends a `token_set` through an agent whose store is
  the Credential Manager to a registration; only the Windows CI job runs it.
- **macOS and Linux.** There is no store yet. The agent runs with none, every pool is
  skipped with the reason "no token store", a `token_set` is refused as
  `store_failed`, and the agent is stopped for `no_pool_started`. A Keychain store can
  later sit behind the same interface by naming its entries, as the Windows store
  does.
- **Tests.** An in-memory store, which can be made to fail with each kind of error.

A store failure has a kind, which says what it asks of the caller, and the platform's
own code when it gave one. The kinds are `needs_user`, such as a locked keychain the
user can unlock, `access_refused`, when the item refuses this program, and `other`.
The Credential Manager's failures are all `other`, with the Windows error number in
the code. The kind and the code appear in the `store_failed` error (see Errors), in
the reason of a pool skipped because the store failed (`the token store failed: ...
(kind needs_user, code 1312)`), and in the log line of a registration that failed
for the same reason. None of them ever holds a token.

The agent reads a token from the store when a pool is admitted, again at each
registration, and at each GitHub status check, so a `token_set` takes effect at the
next registration and check, and a
`token_remove` makes the next registration fail and its slot back off with "no
token". A running job container keeps the JIT configuration it was started with.

`has_token` in the snapshot is the store's answer as of the last configuration
read, not a live reading: it refreshes only on a configuration read, a `token_set`
or a `token_remove`.

### Protocol Version 2

Protocol version 2 is served only by the agent, on the channel of the instance layer
with its access rules: only the agent's user reaches it, as [Control
Channel](#control-channel) describes. A message is one JSON object on one line. A
client message is at most 16 KiB and an agent message at most 64 KiB, line break
included. The first line a client sends MUST be `hello`; anything else, including
`start`'s version 1 requests, is answered with `unsupported_protocol` and the
connection is closed, so today's commands exit 1 against an agent and change
nothing. [Version Rules](#version-rules) says what a later agent of version 2 may add.

| Message     | Fields                                                                                                   |
| ----------- | -------------------------------------------------------------------------------------------------------- |
| `hello`     | `versions` (the versions the client speaks), `client` (`name`, `version`), which the agent ignores       |
| `welcome`   | `version` (2), `agent` (`version`), `methods`, `topics`                                                  |
| `request`   | `id` (an integer of the client's own), `method`, `params`                                                |
| `response`  | `id`, then `result` or `error`                                                                           |
| `error`     | `error`, for what answers no request: a failed handshake, `busy`, or a message that is not a request     |
| `event`     | `topic` (`state` or `github`), `seq`, `value` (the snapshot, or the GitHub status)                       |
| `heartbeat` | `seq`, the last event's                                                                                  |
| `ending`    | `reason`: `quit`, `executable_changed` or `update`; `update` (`version`, `stopped_by_you`) for an update |

A reader ignores a message type, a field and a topic it does not know, and calls only
the methods `welcome` lists. The library's `Client` does so, refuses a `welcome` that is not version 2, and
subscribes only to a topic `welcome` listed, reports a result it cannot read, and a `response` whose
envelope it cannot read but whose `id` it can, as `BadAnswer`, not as a closed connection, and refuses a request longer than 16 KiB
with `TooLong` before writing anything, so the connection stays usable. Its wire, snapshot, `Push`, `CallError` and
`ConnectError` types are `#[non_exhaustive]`, so a later slice can add a field or a
variant without breaking a program that uses them; `tests/agent_protocol.rs` shows it
against unknown methods, topics and fields.

The agent answers one connection's requests in order and several connections at once.
A connection holds at most one subscription, so a client that wants both topics
opens two connections; a `subscribe` to another topic on a connection that holds one
is answered with `invalid_request`, and one to the same topic again changes nothing
but, for `github`, counts as a renewal.

| Method             | Params                     | Result                                                             |
| ------------------ | -------------------------- | ------------------------------------------------------------------ |
| `status`           | none                       | The snapshot                                                       |
| `subscribe`        | `topic`: `state`, `github` | `{topic}`; the first event follows at once                         |
| `renew`            | `topic`: `github`          | `{topic}`; keeps the connection's `github` subscription            |
| `start_all`        | none                       | `{state}`, `resuming` while the pools are admitted                 |
| `stop_all`         | none                       | `{state}`                                                          |
| `build`            | none                       | `{state, started}`, `started` in milliseconds since the Unix epoch |
| `quit`             | none                       | `{state}`                                                          |
| `token_set`        | `repository`, `token`      | `{repository}`, the key it was stored under                        |
| `token_remove`     | `repository`               | `{repository}`                                                     |
| `drain_for_update` | `version`                  | `{state, update}`; see The Update Drain                            |

`token_set` is the one message that carries a secret: the token travels only in its
`token` param, from the client to the agent, and the agent never sends, logs or
repeats it. Its error text never quotes the params.

Errors have a stable `code`, an open set, and a `message` in plain text:

| Code                   | Meaning                                                                                                                                  |
| ---------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- |
| `unsupported_protocol` | The first line was not a `hello` that lists version 2; `versions` lists what the agent serves                                            |
| `busy`                 | The agent is in a transition that does not take the method, or has its most connections                                                  |
| `unknown_method`       | The agent has no such method                                                                                                             |
| `invalid_request`      | The params are not what the method takes, or the message is not a request; a malformed request with a readable `id` is answered under it |
| `subscription_limit`   | The agent has its most subscriptions                                                                                                     |
| `not_subscribed`       | A `renew` on a connection that holds no subscription to the topic, such as one that ended for want of renewal; subscribe again           |
| `token_invalid`        | The token is empty or too long                                                                                                           |
| `store_failed`         | The token store could not do it; `store` holds `kind` and `code`                                                                         |
| `refused`              | The agent's state does not allow the method                                                                                              |

A subscription gets the snapshot at once as an event with `seq` 1, then an event
with the whole snapshot after each change, at most one per 250 ms: changes within a
window are merged into one event, so a client never sees an intermediate state it did
not ask for. `seq` rises by one with each event of the connection. After 15 s without
an event the agent sends a `heartbeat` with the last `seq`, and the library's client
reads 45 s of silence on a subscribed connection as a lost agent. A `Client::call`
fails with `Closed` once the connection has ended but waits as long as the agent
takes on one that stays open, so a caller that needs a bound wraps it in its own
timeout. Dropping a call, as that timeout does, is safe: the client has queued the
request whole for a task that writes it to the end, and discards the answer.
Before the agent ends it sends `ending` to every connection, one still in
its handshake included, and waits up to 0.5 s for them to end; an
answer already given, such as `quit`'s, is written before the `ending`.

The limits are timers on the agent's own runtime on every operating system, so a
silent or stalled client holds no task of its own past them. A thread is held only
where the pipe crate needs one, which is measured on Linux only (the stalled readers'
test counts the runtime's worker threads there, and finds none held); the Windows
case is next to the table.

The limits:

| Limit         | Value                                                                                                                |
| ------------- | -------------------------------------------------------------------------------------------------------------------- |
| Connections   | 16; a 17th is told `busy` and closed; beyond 32 connections open or waiting for `busy`, one is closed with no answer |
| Subscriptions | 4; a 5th is told `subscription_limit` and the connection stays                                                       |
| Handshake     | 5 s from connect to `hello`                                                                                          |
| Idle          | 60 s without a message, for a connection with no subscription and no request pending                                 |
| Renewal       | 90 s without a `renew` or `subscribe` ends a `github` subscription; the connection stays                             |
| Write         | 5 s for one message, or the connection is closed                                                                     |

On Windows the pipe is created as the first instance of the name with the descriptor
`D:P(A;;GA;;;<user SID>)` and with remote clients rejected, as for `start`, but it
is served by the pipe crate's asynchronous listener on the agent's runtime, so the
limits above are the same there as on macOS. Both channels take the pipe's options
from one function, so they cannot differ. The pipe crate flushes a pipe on a blocking
thread that waits until the peer has read everything, and that wait cannot be
cancelled. So the agent flushes only the messages that end a connection (`ending`,
and the error that answers a refused or failed handshake), each in at most the write
limit, and abandons a connection's write half when the connection ends for any
reason, so the crate does not park it on a waiting thread. A client that never reads
and never closes can still keep one such thread for each of those final messages.
The agent leaves its runtime without waiting for blocking tasks, so neither the exit
nor the release of the lock waits for such a thread, and
`tests/agent_quit_stalled.rs` runs the agent as a process on every platform and
shows a `quit` ends it within a second or so while a subscribed client reads
nothing. On Unix the socket has the checks of
[Control Channel](#control-channel).

### Version Rules

These rules bind every agent and every client of protocol version 2, including the
ones other children add. The sections above describe what version 2 holds today;
this one says how it may grow without a new version.

Within version 2 an agent may add methods, topics, fields, error codes, runner
states, images, image warnings, and the reasons a state or `ending` carries, and
lists its methods and topics in `welcome`. A client ignores what it does not know,
shows an unknown runner state by the runner's reason, and calls only methods listed.
Removing one, or changing its meaning, needs version 3.

So a client written against today's agent keeps working against a later agent of
version 2, and a later client works against today's agent by calling only what
`welcome` lists. [Protocol Version 2](#protocol-version-2) says how the library's
`Client` follows these rules, and App says how the app follows them, in
[The View](./app.md#the-view) and
[The Agent and the App's Session](./app.md#the-agent-and-the-apps-session). A
change that removes or reinterprets anything above is a version 3, which is a new
framing decision for the maintainer, not an addition.

### The Snapshot

The snapshot is what `status` returns and what the `state` topic carries. It comes
from local state the agent already keeps; it reads nothing from Docker or GitHub, and
never holds a token or a JIT configuration. Every field is optional for a reader, and
a reader ignores one it does not know.

| Field                          | Holds                                                                                                 |
| ------------------------------ | ----------------------------------------------------------------------------------------------------- |
| `agent`                        | `version`, `state`, `reason` (`code`, `message`), `since`, `build`, `restart_due`, `update`           |
| `agent.update`                 | While a drain for an update runs: `version`, `waiting`, `deadline`, `pid`                             |
| `agent.build`                  | `started`, `outcome` (`building`, `ready`, `failed`, `cancelled`), `finished`, `message`              |
| `configuration`                | `path` and `problems`, at most 100                                                                    |
| `runners_up`, `runners_wanted` | Totals over the listed pools, kept when rows are left out; 0 while `restart_due` is set               |
| `repositories`                 | Each repository (`owner/name` in lower case), `has_token` and its pools                               |
| `pools`                        | `name`, `repository`, `state`, `reason`, `has_token`, `runners_up`, `slots`, `runners`, `diagnostics` |
| `pools[].runners`              | `slot`, `state`, `workload`, `reason`, `since`                                                        |
| `pools[].diagnostics`          | `limit`, `written`, `in_sink`, `containers`, `findings`, `manual_recovery`                            |
| `leftovers`                    | What a stop could not remove, `kind` (today `container`) and `name`                                   |
| `images`                       | Each image the agent tracks (today the runner image): `name`, `lines`, `warnings`                     |
| `omitted`                      | How many `runners`, `problems`, `pools`, `repositories`, `leftovers` and `images` it left out         |

When the runtime core fails, the agent starts it again after 10, 30 and 60 s for the
first, second and third failure within 10 minutes, and stops for `restart_limit` at the
fourth. While it waits, `agent.restart_due` holds the time it restarts, the agent's
state stays `running`, and `pools` is empty, so `runners_up` and `runners_wanted` read
0 for the wait, because the core's rows are not kept across a failure. A stopped agent
lists its pools again. `repositories` is still listed.

A pool is `waiting_for_docker`, `preparing`, `running`, `skipped` or `stopped`; the
first word stands for whichever engine the pool waits for. A runner is `running`,
`registering`, `backing_off`, `held` or `stopped`. The `reason` of a runner is plain
text that says more about its state, such as what a state means that the reader may
not know. A backing-off runner gives the reason it backs off: a token store
failure's kind and code, or what GitHub answered, scrubbed of any token and never a
command. A `held` runner is one whose job host a host prerequisite refuses, with
that prerequisite's reason, scrubbed the same way and cut to 500 characters; it
occurs only on axmc's Tart backend, which the agent, running axpc only today, does
not run. An image warning is data, not text: a `code` (`not_built_by_marshal`,
`packages_old`, `image_old`, `runner_old` or `runner_may_be_newer`) with `days` or
`version` where they apply, so the app words it. `leftovers` lists every container
name of a failed removal call, which can name more containers than the one that
failed. They stay listed until a start prepares the pools, which removes what is
stale, so a `start_all` that stops for a missing image or an invalid configuration
keeps them.

An agent may add a runner state, an image and an image warning within version 2, so
a reader MUST treat a state word it does not know as an unknown state and show the
runner's `reason`, MUST list an image it does not know, and MUST ignore a warning
code it does not know. The library's `RunnerState`, `RunnerInfo::kind`,
`ImageWarning::kind` and `ImageInfo::known_warnings` do so, and a unit test of the
client shows an event with a state and a warning from a later agent.

A snapshot is cut to at most 64 KiB less the message's envelope. When it is larger
it first lists no more than 100 configuration problems, then leaves out runner rows
from the end, as many as it must and no more, then pools from the end, then
leftovers, repositories, images and configuration problems from their ends, and says
how many in `omitted`. Last of all the long texts, the configuration path and the
agent's version, reason and build message, are cut to 512 characters. `runners_up`
and `runners_wanted` still count a row left out.

### GitHub Status

GitHub status is what GitHub says about this host's runners, for the window only:
whether each is offline, idle or busy, the job a busy runner runs, and the request
budget. It lives in the library's `github_status` module beside registration, and the
agent runs it and publishes it as the `github` topic. It owns the busy flag, the job
lookup, the request budget and the back-off; a repository is one kind of scope, and a
later scope, such as an organisation's, adds its own fields under the version rules
above, so no field here changes meaning.

**Collection follows the subscription.** The agent reads GitHub only while at least
one connection holds a `github` subscription that was subscribed or renewed in the
last 90 s. When the last one ends, no new request starts after 1 s; one in flight
finishes or times out and its answer is kept. The app holds that subscription on a
connection of its own while its window is shown and not minimized, and renews it every
30 s, so an app that hangs stops collection within 91 s. After collection stops the
agent keeps each repository's last pages and their `ETag`s.

**The checks.** A repository is checked while one of its pools has a runner with a
`workload` and the token store held its token at the last look; the token is read
from the store at each check by the repository's key, as a registration reads it.
A check reads the runner list in full, then, while a busy runner's job is not known,
sends up to two lookup requests: the repository's in-progress runs, newest first, then
the jobs (`filter=latest`) of those runs, continuing where it stopped at the next
check. A job is matched to its runner by name, kept for that runner's life, and
looked for in at most 20 checks. Every page is requested with `If-None-Match` and that
page's last `ETag`, so a `304` keeps it at no cost. Pages hold 100 runners, 10 runs
and 30 jobs; a read of more than 10 pages, a page over 1 MiB, or a `200` whose body
is not the list asked for reads as `error`. Only the jobs pages of runs still in
progress are kept. Requests go out one at a time across the host, in the order repositories
fall due. A repository is checked at most once per 15 s at the `normal` level and
once per 60 s at `slowed`, and never sooner than the latest `x-poll-interval` GitHub
sent for any of the repository's reads.
Registrations stay outside this queue, and their responses are not read for the
budget.

**Next pages.** For every paged read, each address in a `link` header's `next` is
compared with the read's first request. It is followed only when it has that request's
origin and its path is either that request's path or `/repositories/<id>` followed by
exactly the rest of that path after `/repos/<owner>/<repo>`, where `<id>` is one or
more ASCII digits and the same on every page of the read, and only its query differs.
GitHub names next pages in the `/repositories/<id>` form. Any other address reads as
`error` and is not requested, so no response can send the token to another origin or
another endpoint path. The agent does not check that `<id>` is the configured
repository's, which would cost another request; the address comes only from GitHub's
response over TLS, with no redirect followed.

The requests use the registration client: `https://api.github.com` only in a release
build, no redirect, 10 s to connect, 30 s a request, 1 MiB a response, the token in a
sensitive header and scrubbed error text. GitHub suggests following 301, 302 and 307;
the agent follows none, so a redirect cannot carry the token elsewhere.

**The budget** is read from the `x-ratelimit-limit`, `x-ratelimit-remaining` and
`x-ratelimit-reset` headers of each display response. Within one reset window the
lowest remaining count is kept; a response with a later reset, received after the
current window's reset has passed, starts a new window at `normal`. The level is
`normal` above 25% left, `slowed` at or below 25%, and `paused` at or below 10% or
when GitHub says the limit is reached; it only moves down within a window, and a pause
sends no display request until the reset.

**What each answer means:**

| Answer                                           | Runner list                                                                   | Lookup                                               |
| ------------------------------------------------ | ----------------------------------------------------------------------------- | ---------------------------------------------------- |
| `200`                                            | Read                                                                          | Read; `job_names` becomes `shown`                    |
| `304`                                            | Keep that page                                                                | Keep that page                                       |
| 403 or 429 with `x-ratelimit-remaining: 0`       | Pause until the reset, or, with no reset known, wait as for a secondary limit | The same                                             |
| 403 or 429 with GitHub's secondary-limit message | Wait `retry-after`, else 60 s doubling to 15 minutes                          | The same                                             |
| Another 403 or 429 with `retry-after`            | Wait `retry-after`                                                            | Wait `retry-after`                                   |
| `401`                                            | `token_rejected`                                                              | `job_names` `unreadable`, `token_rejected`           |
| Any other 403                                    | `token_rejected`                                                              | `job_names` `hidden`: the token has no Actions: read |
| `404`                                            | `not_found`                                                                   | `job_names` `unreadable`, `not_found`                |
| `3xx`                                            | `moved`                                                                       | `job_names` `unreadable`, `moved`                    |
| `5xx`, anything else, or a next page refused     | `error`                                                                       | `job_names` `unreadable`, `error`                    |
| No connection or a timeout                       | `unreachable`                                                                 | `job_names` `unreadable`, `unreachable`              |
| The store cannot give the token                  | `token_unreadable`                                                            | No request                                           |

A rate-limit wait holds every display request on the host. A `retry-after` or
`x-poll-interval` counts for at most an hour, and a reset is never taken as more than
an hour away, which is the rate limit's own window. A repository whose runner
list fails keeps its last runners and `checked_at`, and is tried again after 15 s,
doubling to 5 minutes, never sooner than the level allows, with `retry_at`. A failed
lookup leaves the runner list alone: job names become `unreadable`, and lookups wait
15 s, doubling to 5 minutes, never less than the level's interval, with
`job_names_retry_at`; `unreadable` stays until a later lookup is answered. While job
names are hidden the agent sends one in-progress-runs request again when one of the
repository's runners becomes busy, at most once a minute: a runner that becomes busy
within the minute gets it at the first check after the minute, so a token that gains
Actions: read shows the next job's name without a restart.

**The `github` value:**

| Field                          | Meaning                                                                                                                |
| ------------------------------ | ---------------------------------------------------------------------------------------------------------------------- |
| `budget`                       | `null` until a display request is answered, then `limit`, `remaining`, `resets_at`, `seen_at` and `level`              |
| `repositories`                 | Each repository with a registered runner and a stored token, by its key, with what the agent last learned of it        |
| `repositories[]`               | `repository`, `checked_at`, `status`, `retry_at`, `job_names`, `job_names_cause`, `job_names_retry_at`, `runners`      |
| `repositories[].runners`       | This host's runners GitHub lists: `name` (the snapshot's `workload`), `github` (`offline`, `idle` or `busy`) and `job` |
| `repositories[].runners[].job` | `workflow`, `name`, `run_id` and `job_id`, once a busy runner's job is known                                           |
| `omitted_runners`              | Runner entries left out from the end to keep the value within 64 KiB less the envelope                                 |

Times are milliseconds since the Unix epoch. `status` is `ok` or the cause in the table
above; `job_names` is `unknown` (no in-progress-runs request answered yet), `shown`,
`hidden` or `unreadable`, with `job_names_cause` in the words of `status`. `level`,
`status`, `job_names` and `github` are open sets: a reader treats a word it does not
know as unknown. A runner is not listed when its repository's `checked_at` is later
than the runner's `since` in the snapshot and no entry names it; until then it is
still being checked. The workflow and job names come from GitHub and are untrusted:
the agent removes control characters and Unicode bidirectional formatting characters
and cuts each to 120 code points with an ellipsis. The value never carries a token, a
URL from GitHub or a response body. A job's page is
`https://github.com/<owner>/<repo>/actions/runs/<run_id>/job/<job_id>`, built by the
reader from its own configured repository.

The agent logs each display request's method, path, status code and requests left,
and for a 403 or 404 from a lookup also GitHub's message, cleaned as names are and cut
to 200 characters, and its `X-Accepted-GitHub-Permissions` header. It logs each change of
a runner's GitHub state, of a repository's job names and of the level, each failure
and pause with when it tries again, and never a token.

### What the Agent Tests Cover

The agent tests run the agent and the test script on one runtime, behind the real
channel, on the fake backend, clock, registrar and image builder and an in-memory
token store, and talk to it with the library's client or by hand, line by line.
`tests/agent_resident.rs` covers stop, start and quit (also while a subscribed
connection reads nothing), launching stopped, the executable renamed, the resuming
state, build (ready, failed, cancelled by a stop or a quit, `busy`, `refused`, and the
image check after it, which still answers and is skipped or abandoned by a stop or a quit), and
staying up for each reason, also through `start_all`, which answers `resuming`, that
`token_set` and `token_remove` are answered while it resumes, builds and stops, and the
core's restarts: the `restart_due` window with its empty pool list, and the stop for
`restart_limit`. The restart waits are the fake clock's, with a real pause of 400 ms
each so the script can look at the window.
`tests/agent_protocol.rs` covers the handshake, `welcome`, unknown methods, topics
and fields, subscription (also by a client that arrives while the agent is idle),
coalescing, heartbeats, `ending`, the limits as timers,
a connection that stops reading, the `github` topic and `renew` (the full value on
`subscribe`, the end after 90 s without renewal, `not_subscribed` after it, and
collection only while a subscription is held). `tests/github_status.rs` runs GitHub
status on a paused clock against a fake GitHub: the intervals and levels, the pause,
`retry-after` and the secondary limit, the back-off, pages by `link` with their
`ETag`s and refused next pages, the job lookup and its 20 checks, the hidden-names
retry, and that no token reaches a value or the log; the module's unit tests cover
each answer, the budget window, the next-page rule and the cleaning of names. `tests/agent_tokens.rs` covers what `token_set` and
`token_remove` do to the next registration, a skipped pool starting, refused tokens,
and the store's kind and code in an error, a skipped pool's reason and the log.
`tests/agent_snapshot.rs` covers the snapshot, the diagnostics, the leftovers, the
64 KiB cut, that the agent ends and frees the lock within a second of `quit` with 640
runners and four subscribed connections that never read, the stalled readers' thread
count (measured on Linux only, from the runtime's worker threads) and, in
`tests/agent_quit_stalled.rs`, that the process ends within a second or so of a
`quit` with a stalled subscriber, and that no token or JIT
configuration reaches a connection or the log. `tests/agent_versions.rs` runs `stop`,
`status`, `restart` and `diagnostics` against an agent and shows they exit 1 and
change nothing. `tests/agent_channel.rs` covers the socket's mode, the peer's user
and a directory open to others on Unix. `tests/agent_channel_windows.rs` shows a pipe
of a taken name is refused and a client passes the check of the server's user. `tests/agent_process.rs` runs the agent as a process on
the fake `docker` and a GitHub stub, a second agent refused, and the log continued
with ten files kept. `tests/agent_drain.rs` covers the update drain: idle, busy and
unknown runners, the bound, the reachable rows of its state table, a cancelled drain,
a backing-off slot, a pool waiting for a token, a core failure, and diagnostic
finalisation.

The Windows CI step `tests/ci/control-pipe-security.ps1` runs its checks against
the `serve` mode of the helper and against `serve-agent`, which opens the agent's
channel, so the agent's pipe is read back with the same descriptor. The real
Credential Manager test and the connection limits on Windows and macOS run in CI only.
Not verified here: the agent on Windows and macOS beyond what the hosted runners show,
and the remote-client rejection, as for `start`.

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
the first three, so an image built from the definition another way, in CI or by hand,
carries those three only; `build` adds the last
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
a well-formed version does not compile. The version rises when the run contract
changes or the image must provide something it did not. The two runner lines the
[update drain](#the-update-drain) reads are not part of it: they come from the
runner, and a runner bump keeps the contract version, so the bump checks them
instead, as [Bumping the Runner](./development-workflow.md#bumping-the-runner)
describes.
Contract 1 is the run
contract as Phase 2 left it, plus the diagnostic variant's use of the collector's
path and output directory. Contract 2 adds what the VM runtime needs of the image: a
Docker client at the version the golden VM's Docker Engine is at, installed from
Docker's apt repository after its signing key matches a recorded fingerprint, and a
`docker` group with the id `DOCKER_GROUP_ID`, so the runner can use the VM-local socket
the VM runtime mounts. The Docker runtime's arguments are unchanged and it still mounts no
socket, but an image of contract 1 is mismatched, so axpc needs one `marshal build`
after the release that carries contract 2. The golden VM's name records the contract,
so a golden VM built for contract 1 is mismatched the same way.

Unit tests in `cli/src/image/contract.rs` keep the declaration honest:

- `APPROVED_CONTRACTS` pins a fingerprint of the run contract's argument lists, the
  launcher, the diagnostic variant's argument lists and launcher, the VM runtime's
  argument lists with the VM-local socket mount, the label keys and the inspect
  format, against the version they were approved for. Changing any of them without
  raising the contract version fails, and the failure names the row to add with the
  new version. A new version is approved by adding that row, with the fingerprint the
  failure prints.
- The Dockerfile must hold the golden VM's pins, as
  [Golden VM Provisioning](#golden-vm-provisioning) says, and the image smoke test
  must check the same client version and group id.
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
Both calls share a bound of 4 s. A resume's reload is the configuration read, then
on axmc the [host prerequisites](#host-prerequisites), whose two version reads run
together and get 2 s each, and then this check. It takes 4 s at the most on axpc and
6 s on axmc, so it fits inside `start`'s 10 s wait. It follows the container steps of
a `restart`, which take 28 s at the most, so it ends at 32 s on axpc and 34 s on axmc,
inside the 35 s wait.

| Outcome            | When                                                                         |
| ------------------ | ---------------------------------------------------------------------------- |
| Matching           | Inspect succeeds, and the contract label equals the CLI's                    |
| Mismatched         | Inspect succeeds, but the contract label is missing, malformed, or different |
| Missing            | Inspect fails, and the listing succeeds with empty output                    |
| Docker unavailable | Any other failure, or the 4 s running out                                    |

On axmc the check reads the golden VM instead, from the VM names alone, as the
[Tart Backend](#tart-backend) describes: a matching golden VM carries this CLI's
golden VM version and image contract version, a missing one is `missing`, and any
other is mismatched. Their messages are
`the golden VM <name> has golden VM version <g> and image contract <m>, but this CLI needs golden VM version <G> and image contract <n>`
and `the golden VM is missing; this CLI needs one with golden VM version <G> and image contract <n>`.
A `tart list` that fails or overruns the same 4 s is the unavailable outcome.

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
  image was replaced another way, such as by a build made by hand.
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

An image without a build-time label, such as one CI or a build by hand made, has an
unknown build age, and its warning says it was not built by
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
- on axmc, `golden-vm: <name>` once a golden VM exists, and `mismatch (golden VM
version <g> and image contract <m>, CLI golden VM version <G> and image contract
<n>)` in place of the mismatch above;
- one `warning: <text>` line per freshness warning.

`image-built` and `runner` read `unknown` before the first check Docker answers, and
when the image is missing or its labels are missing or malformed.

## Running a Host

`start` runs the host's pools in the foreground through the instance layer and the
runtime layer, and `stop`, `restart`, `status`, and `diagnostics` talk to it from another terminal
over the control channel. `build` builds the runner image, and the same clients
talk to it. axpc runs its pools with `start` and builds its image with `build`, as
[Windows Runner Host](./windows-runner-host.md) describes; axmc does the same with Tart
VMs, and `build` there makes the golden VM, as [macOS Runner Host](./macos-runner-host.md)
describes.

| Module                   | Holds                                                                                                                |
| ------------------------ | -------------------------------------------------------------------------------------------------------------------- |
| `start`                  | `start`: the lock, validation, the log file, containment, the channel, the instance, and its signals                 |
| `build`, `build::golden` | `build`: the lock, validation, the channel, its states, and its `docker` steps and their aborts; the golden VM build |
| `control`                | The clients `stop`, `restart`, `status`, and `diagnostics`                                                           |
| `shim`                   | The Unix shim between `start` and one job container's `docker run` client                                            |
| `diagnostics`            | The exporter child, listed under The Runtime Layer                                                                   |
| `instance::runtime_core` | The runtime core, listed under The Instance Layer                                                                    |
| `instance::log_file`     | The log, listed under The Instance Layer                                                                             |

### Start

A fresh `start` does these in order:

1. It takes the instance lock. When the lock is held it asks this user's instance
   to resume instead, as below.
2. It validates the configuration as `validate` does. An invalid one prints every
   error and exits 1, and nothing is written.
3. It opens its log file and logs the plan summary.
4. It contains its children, builds the HTTPS client, and opens the control
   channel. A failure of any of them, including a TLS setup failure, exits 1.
5. It runs the instance on the backend of the profile: Docker on axpc, Tart on
   axmc. On axmc, a host prerequisite that refuses ends the instance first, as
   [Host Prerequisites](#host-prerequisites) says. The runtime core waits for the
   engine every 5 s (Docker, or Tart), checks the image as
   [The Image Contract](#the-image-contract) describes, prepares the pools
   as [Names and the Slot Loop](#names-and-the-slot-loop) describes, and then runs
   every slot's loop until a stop. When the image cannot run jobs, the instance
   ends, and `start` exits 1, saying to run `marshal build --config <path>`, then
   `marshal start --config <path>`. When no pool can start, `start` exits 1. On axmc
   the instance also holds off idle sleep and watches the Mac's sleep and primary
   network, as [Signal Source](#signal-source) describes, and a slot clones no VM
   until a primary network is recorded.

On Windows, `start --profile axmc` exits 1 before step 1 with
`error: the axmc profile runs only on macOS`, because Tart exists only on macOS.
On axmc the containment of a job VM is the [VM shim](#the-vm-shim): `start` gives
the Tart backend the shim as its launcher on Unix, as it gives the Docker backend
the container shim.

On Linux a release build exits 1 at the first step, because Linux is not a
supported host, and `tests/instance_roots.rs` shows it in the release profile. A
debug build runs on a test root, for the tests. On macOS, `start --profile axpc`
runs the container backend with Unix containment, which serves the tests, not an
operated host. `MARSHAL_TEST_ROOT` moves the lock, the channel, and the log
directory, `MARSHAL_TEST_GITHUB_API` moves the GitHub base address,
`MARSHAL_TEST_DOCKER` names the `docker` client that `start` and `build` run, the
Unix shim's removal in `start` included, `MARSHAL_TEST_TART` names the `tart`
client that `start` runs on axmc, and `MARSHAL_TEST_BUILD_DEFINITION` names
a test definition that `build` builds instead of the embedded one. On axmc the
tests also set `PATH` to find the fake `softnet`, as Tart does, and
`MARSHAL_TEST_SOFTNET_OWNER_MODE` to judge its file, and `MARSHAL_TEST_SIGNALS` to
script the sleep, the primary network and the keep-awake hold. Only a debug
build honours any of them: in the release profile `tests/base_address.rs` shows
that the client stays `docker`, `tests/build_definition.rs` that `build` keeps
the embedded definition, and `tests/release_overrides.rs` that the client stays
`tart`, the `route`, `arp` and `caffeinate` programs stay the system's, the
signals are not read from a directory, and Softnet's file is judged as it is.
The tests name the fake client rather than put it first on `PATH`, because Windows
searches the program's directory and the system directories before `PATH`, so a
real `docker.exe` in `System32` would win.

A `start` that finds the lock held changes nothing. It canonicalises its
configuration path, sends `resume <profile> <path>`, and waits up to 10 s:

- A stopped instance started with the same path and profile reads its own file
  again. A valid file runs it and `start` exits 0; an invalid one leaves it stopped
  with the new reason and `start` exits 1.
- A running, stopping, resuming, or building instance refuses, naming its state,
  and so does a stopped one started with another path or profile. `start` exits 1.
- A stopped instance whose image cannot run jobs stays stopped with that reason,
  and `start` exits 1.
- A stopped axmc instance whose host prerequisite refuses stays stopped with the
  refusal as its reason, and `start` exits 1.
- A stopped axmc instance that a sleep or a network change stopped resumes like any
  other: it reads its file, checks the prerequisites and the image, records the
  primary network afresh, and `start` exits 0.
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
  `waiting for Docker` (`waiting for Tart` on axmc), `preparing`, `running`, or
  `skipped: <reason>`; one `slot`
  line per slot, which is `registering` (the preparation of its job host and the CPU
  probe included), `backing off`, `held: <reason>`, or `running container <name>`;
  and the image lines under
  [The Image Contract](#the-image-contract), then, on axmc, a
  `network: waiting for a network (no IPv4 default route)` line while no primary
  network is recorded, a `warning: keep-awake is not held (...)` line while the hold
  is wanted and not held, and one `warning: <message>` line per
  warning of the latest passing host prerequisite check. Pools and slots
  are listed while the runtime core runs; the image and warning lines are listed on
  every call, as of the latest check. A `build` answers `state`, `image`, and `elapsed: <s> s`.
- `restart` waits up to 35 s for the instance to run its container steps (28 s at the
  most), read its file again, check the host prerequisites (axmc only) and the image
  (6 s at the most on axmc, 4 s on axpc), and answer. An invalid file or an image that cannot
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
   later; alongside, each diagnostic container is exported and removed by 24 s, a
   VM's by deleting the VM.
3. At 28 s whatever still runs is dropped. A `docker` client is killed; on Unix a
   job container's shim sees its liveness pipe close, removes its container, or
   ends a diagnostic one and keeps it, within 5 s, and ends. A VM backend reserves
   10 s of the 28 s to release its job VMs, which ends the container steps earlier,
   as the [Tart Backend](#tart-backend) describes, and a VM its release could not
   delete is deleted by its VM shim within 15 s more.
4. `start` gives the answers it has already sent, such as the one to `stop`, up to
   0.5 s to reach their clients, closes the channel, and exits, which releases the
   lock.

A second SIGINT, Ctrl+C, or Ctrl+Break skips the grace period: a `docker stop` under
way is abandoned and the ordinary containers are removed at once, while each
diagnostic container is ended with `docker kill` and kept without its export.
`start` then exits 1, and its last line names each kept container. A VM backend cannot
keep a container once its VM is gone, so it deletes each diagnostic VM without an
export and reports the lost evidence, as [Diagnostics](#diagnostics) describes; `start`
still exits 1 and names the containers. Raising Ctrl+C in a
Windows test needs `unsafe`, so the Windows handling is checked by review. On axpc
one Ctrl+C stopped the instance with exit 0, and a second Ctrl+C force-stopped it
with exit 1 (maintainer's report,
[#56](https://github.com/axross/runners/issues/56#issuecomment-6511420845)). That
the second kept the diagnostic container is not yet verified on axpc
([#56](https://github.com/axross/runners/issues/56#issuecomment-6511420845)): the
report did not say. Ctrl+Break is not yet verified on axpc
([#56](https://github.com/axross/runners/issues/56#issuecomment-6511420845)): it
could not be tested.
For Ctrl+Break, a client's own process group keeps Ctrl+C from it, but whether it
also keeps Ctrl+Break from it is unverified.

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
`Diagnostic gap in pool <pool>, container <name>: <text>`, where the text names the
gap: a missing final observation, a full sink, a failed or unsafe
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
`stop` still ends within 30 s and its 35 s wait is unchanged. A backend that reserves
a release budget `R` (the [Tart Backend](#tart-backend) reserves 10 s) leaves the
release the time after the container steps, so the diagnostic column's times fall
earlier by `R` less the 4 s it leaves before the exit deadline. With an `R` of 10 s
the stop is ended at 6 s with a grace of 4 s, each export by 12 s, and each removal
by 18 s; the export and removal windows keep their sizes, and the release keeps its
10 s before 28 s, with no slack after it, so a VM that the release could not delete
in a double fault is deleted by its VM shim within 15 s after `start` exits, or by
the next `start`.

**Startup.** While a pool is prepared, the CLI looks at the labelled containers of
the pool that an earlier process left and the backend does not retain:

- **Diagnostics on.** It retains them with the pool's current settings and
  finalises them on one clock per pool, as at a stop, and reports each one:
  `Diagnostic container <name> that an earlier run left: exported, removed.`, with
  `export gap` or `not removed` in place of either word.
- **Diagnostics off.** It gives them one bounded `docker stop --time 10`, ended at
  12 s, warns for each, and refuses the pool. The reason names
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

**Diagnostic VMs.** On axmc the diagnostic container lives in a job VM, which is the
retained thing: it stays alive after the job until the exporter has read the
evidence through the guest agent, and its removal is the VM's deletion. A graceful
stop, `diagnostics`, and the job end finalise it as they do a container. A forced
stop, a hard kill of `start`, and the next `start` cannot keep the evidence, because
nothing keeps a VM alive without its holder. They delete the VM without an export and
report the loss with the same gap wording as an export gap, and a forced stop still
exits `start` with 1, naming the containers whose evidence is lost. A crash reports
the loss from the VM shim, on the standard error it shares with `start`.

**The exporter.** Each export runs in a child process: the CLI run again as the
hidden
`marshal export --pool <pool> --directory <sink> [--container <name>] [--vm <name>] [--raw-records] [--checkout <path>] --deadline-ms <ms> -- <docker or tart>`.
Without `--container` it only counts the pool's bundles. With `--vm` it reads the
evidence from that VM through `tart exec`, with the program after `--` as `tart`; the
bundle, its files, limits, and exit codes are the same. Its clients are then `tart exec`
clients that run `docker` in the VM, and the Isolation item below applies to them as
it does to the `docker` clients.

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
  deadline. Every export happens inside the one instance, which the machine-wide
  lock makes unique, so no named mutex is needed.
- **Identity and archive.** The child writes `identity.txt` from the inspect
  allowlist under [Backend](#backend), its output capped at 1 KiB and required to
  match a fixed shape. It reads the `docker cp` archive through a strict ustar
  subset. It accepts the collector's directory entry, named `./`,
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
  `complete.txt` last. A `metrics.txt` without
  `final=observed` is a gap.
- **Isolation.** The child starts in a process group of its own, as the shim does,
  so a terminal's first Ctrl+C reaches only `start`, and a job-end export goes on
  during a graceful stop. Its standard input is a liveness pipe from the instance,
  and the instance passes it the export deadline that remains. The child keeps that
  deadline itself, and ends when it passes or when its standard input closes,
  killing its clients first: on axpc its `docker` clients, and on axmc its `tart exec`
  clients, which run `docker` in the VM. Each client runs in a process group of its
  own, killed when its call is dropped, with the JIT variable cleared and its standard
  error discarded, and every file system call runs on a thread of its own, so a hung
  sink cannot keep the child from ending. At the export deadline the instance closes the liveness pipe, and kills
  the child 1 s later only if it has not ended. On Windows the child also inherits
  `start`'s Job Object. So the child outlives neither its deadline nor its
  instance, and its clients end with it whenever it ends on its own. A child killed
  1 s after its pipe closed cannot kill its clients: on Unix they run until each
  ends by itself, which is what the `tart exec` clients of axmc do, since macOS has
  no Job Object, and on Windows until `start` ends and its Job Object closes.
- **Exit codes.** 0 is a complete bundle, or a count; 3 a complete bundle with a
  gap; 4 a full sink; and 1 anything else, a refused sink, a failed copy, an archive
  outside the subset, a deadline, or a closed pipe. Containment needs no exit code
  of its own: the Job Object on Windows and the liveness pipe on Unix are in place
  before the child starts.
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

A retained VM is finalised the same way: the export reads it through the guest
agent, and its removal is the VM's deletion, so a VM whose deletion failed is
reported as `removal failed; it stays retained` and deleted by a later `diagnostics`
once Tart can, without a second bundle.

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

- `diagnostic_archive.rs` covers the archive reader's accepted and refused
  entries, and the limits at and one past each bound;
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
- `tart_diagnostics.rs` runs the Tart backend and the real exporter child against the
  fake `tart` and the fake `docker` in its VMs: the bundle exported through the guest
  agent, token-free, before the VM is deleted; a copy that hangs in the VM, which ends
  the export at its deadline and its `tart exec` client with the exporter; a
  malformed or oversized archive, a missing final observation, a full sink, a failed
  stop and a clock with no time left to begin the export, each with the container's
  gap; `diagnostics` finalising a retained VM; a VM kept for its finalisation at a
  stop; a forced stop that deletes the VM without an export and reports the gap; and
  a VM already gone;
- `axmc_diagnostics.rs` runs `start`, `stop`, and `diagnostics` as processes on an
  axmc host with a diagnostics-enabled Linux pool: a finished job exported and its VM
  deleted, a graceful stop that exports a running job first, a second SIGINT, the
  next `start` deleting a leftover, a killed `start` whose shim deletes the VM and
  reports the gap, a killed `start` whose exporter ends with the `tart exec` client of
  its copy, and `diagnostics` finalising a VM whose deletion failed;
- `sink.rs`'s unit tests refuse a sink that is missing, under a link, inside the
  checkout, also when the checkout is named through a link, or not private, and on Windows one that is or lies under a junction or
  a directory symbolic link.

Verified on axpc (maintainer's report,
[#56](https://github.com/axross/runners/issues/56#issuecomment-6511420845)): a
diagnostic job on Docker Desktop, and the Windows console-close handling below,
apart from its timing.
Not verified: the macOS behaviour of the process-group signal the SIGINT tests
send, beyond what the hosted runners show.

### Build

`build` builds the image the configuration names, from the definition embedded in
the binary, and reads nothing from a checkout or the working directory. It does
these in order:

1. It takes the instance lock. When the lock is held it exits 1 and changes
   nothing, naming the instance's state when this user's channel answers within
   5 s, and otherwise giving `start`'s message for a lock held elsewhere.
2. It validates the configuration as `validate` does; an invalid one exits 1.
   `--profile axmc` then builds the golden VM instead, as
   [The Golden VM Build](#the-golden-vm-build) describes. An image name given by
   digest is refused, because it cannot be tagged. It prints the name it builds.
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
   temporary tag, and prunes the dangling images that carry the image label. A failed tag removal is a warning naming the
   tag, which the next `build` removes, and a failed prune is a warning; both still
   exit 0.
8. A failed build or a failed retag removes the temporary tag, leaves the
   configured name as it was, and exits 1. A retag that stalls for 120 s with no
   stop asked is ended, and a `docker image inspect` of the configured name, as
   below, decides whether it committed; one that committed still prunes.

The context is a ustar archive of the embedded files at their paths in
`images/actions-runner/`, sorted by path, with mode `0644`, owner and group 0, and
time 0, so the same binary always sends the same bytes. A debug build builds the
directory named by `MARSHAL_TEST_BUILD_DEFINITION` instead, for the real-Docker
test; a release build ignores it, and `tests/build_definition.rs` shows both
halves.

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

On axmc, exit 0 means the golden VM holds the new image. Exit 1 is a refused, failed or
stopped build. It deletes its build VM and keeps the previous golden VM, with two
exceptions: at the exit deadline the build VM is left to its shim or the next build, and
when the commit is unknown the rename may have made a new golden VM. Stop and failure in
[The Golden VM Build](#the-golden-vm-build) describes both.

#### The Golden VM Build

`build --profile axmc` makes the golden VM that every job VM is cloned from
(`build::golden`). It takes the lock, validates, contains and opens the control channel
as above, and answers `status` and `stop` the same way, but it calls `tart`, not
`docker`. A debug build runs the `tart` that `MARSHAL_TEST_TART` names, and a release
build always runs `tart` from `PATH`, as the [Tart Backend](#tart-backend) does. It
does these in order:

1. **Prerequisites.** It reads Tart's version, then finds Softnet on the `PATH` the Tart
   client carries and judges its file and version, as
   [Host Prerequisites](#host-prerequisites) says. A refusal, a missing `tart` or
   `softnet` included, exits 1 before any VM exists, with a message that names the
   host document's section ([Installing Tart and Softnet](./macos-runner-host.md#installing-tart-and-softnet)
   or [Setting Up Softnet](./macos-runner-host.md#setting-up-softnet)). A warning, such
   as a newer patch version, is logged and the build runs. `build` does not wait for
   Tart, as it does not wait for Docker.
2. **Leftovers.** Every local VM whose name is `marshal-build-linux-<yyyyMMddHHmmssfff>`
   is stopped forcibly and deleted, within 15 s each. These are the build VMs of
   a CLI that died: its [VM shim](#the-vm-shim) deletes its VM when the CLI is gone,
   and this deletes any that the shim could not. A VM that cannot be deleted is a
   warning.
3. **Pull and clone.** `tart pull` of the base image by its pinned digest
   (`golden::pins::BASE_REFERENCE`), within 30 min, and `tart clone` of it to a new
   build VM, `marshal-build-linux-<yyyyMMddHHmmssfff>`. The name is recorded before
   the clone runs, so a clone that fails halfway is deleted. The build VM keeps the
   base's CPUs, memory and 20 GB disk: the build does not run `tart set`, because no
   job needs more than the image build takes, and every job VM is sized from the pool
   when it is cloned.
4. **Boot.** `tart run` under the VM shim (this program run again, as for a job VM),
   then the wait for the guest agent, every second with each question bounded at 5 s,
   until it answers, `tart run` ends, or 120 s pass. A boot that fails has the job VM
   boot failure's text, naming the build VM, and the same section,
   [The Signed-In Session](./macos-runner-host.md#the-signed-in-session). The wait is
   `tart::boot`, which the Tart backend's job VM boot shares.
5. **Provisioning.** The six guest calls of [Golden VM Provisioning](#golden-vm-provisioning),
   in order, each through `tart exec` with its own budget. Their output is read as data,
   cut at 64 KiB, and appears only in the message of a call that failed, on one line. A
   call that exits non-zero fails the build. The listing from the third call goes to
   `golden::key`; a mismatch, a second key, or an empty or oversize listing fails the
   build before the fourth call, which installs from Docker's repository.
6. **The image.** It waits up to 60 s for the Docker Engine in the VM (`docker info`
   every 2 s, each bounded at 10 s), then builds the runner image from the definition
   embedded in the binary, with the context on the standard input of
   `tart exec -i`, and follows the axpc flow as far as a new VM needs: the one
   `docker build` with exactly axpc's arguments under the temporary tag
   `<repository>:marshal-build-<yyyyMMddHHmmssfff>` in UTC, a `docker image inspect` of
   the labels that requires this CLI's image contract, a `docker tag` to the configured
   `imageName`, and the removal of the temporary tag (a failure of the removal is a
   warning). It does not remove earlier temporary tags or prune dangling images,
   because the VM is new. The build's output is forwarded to the log line by line,
   masked and cut as a container's output is.
7. **Commit.** `sync`, then `sync -f /`, as root in the build VM, within 2 min, then `tart stop` of the
   build VM with a 60 s wait, within 90 s, then the wait for `tart run` to end, then
   `tart rename` to the golden name,
   `marshal-golden-linux-g<golden VM version>-c<image contract version>-r<runner version>-<yyyyMMddHHmmss>`
   with the build time: the time taken just before the in-VM `docker build`, the same
   value as the temporary tag's and the `built` label's, not the start of the build.
   **The rename is the commit.** `tart stop` ends the VM at once, with no shutdown
   inside the guest (Tart's `stop` interrupts `tart run`, which stops the virtual
   machine forcibly), so the `sync` is what puts the files the image build wrote last
   on the golden VM's disk.
8. **After the commit.** It deletes every other golden VM, whatever its versions, and
   removes from Tart's cache each image named in `golden::pins::RETIRED_BASE_PINS`, with
   `tart delete`; the current pin is never deleted, and the list never holds it. A
   failure of any of these is a warning that the next `build` tries again, and
   `build` still exits 0, because the newest golden VM is the one used.

`imageName` is optional on axmc. When it is absent, `build` names the image
`actions-runner:local` (`runtime::pool::DEFAULT_IMAGE_NAME`), and `validate` and its
summary do not change.

**The Docker group.** The fourth guest call creates the `docker` group with the id
`DOCKER_GROUP_ID`, so the socket the runner container mounts is owned by the group that
the runner image's `runner` user is in, and adds `admin` to it. A `docker` group that exists with
another id fails the build before any Docker package is installed. The group change reaches
`tart exec` commands only when the guest agent next starts, and the build VM is not
restarted. The image build therefore runs `sudo -n docker`
(`golden::steps::build_vm_docker_command`), where it could instead have rebooted the VM,
which would add a boot and a second wait for the agent. Job VMs, which boot from the
golden VM, run `docker` as `admin` without `sudo`.

**Stop and failure.** Any failure, and `stop`, SIGINT, SIGTERM and SIGHUP at any step
before the rename, drop the call in flight, which kills its `tart` client, and delete
the build VM: a graceful stop and then a forced stop if that fails, then `tart delete`,
within 15 s as one `teardown` budget (7.5 s, 3 s, 3 s and 1.5 s for a listing). The previous golden VM is not touched.
The exit deadline of 28 s from the stop applies as it does on axpc, and at it `build`
exits 1, naming that the VM shim or the next build deletes the build VM. The rename has
the same grace as axpc's retag: when a stop arrives during it, `tart rename` gets up to
5 s from the stop, its client is then ended, and a listing, within 4 s, says whether
the golden name exists. If it does, the build has committed and `build` exits 0; if it does not, the build VM is
deleted and `build` exits 1; if the listing fails, the exit is 1 with "commit unknown"
and the golden name to look for. The build VM is still deleted by its own name, which
removes it if the rename did not happen. If it did, there is no such VM to delete, and
with the listing failing too the deletion cannot be confirmed, so `build` warns that the
next build deletes it if it is still there. The 5 s grace, the 4 s listing and the 15 s
clean-up fit inside the 28 s deadline, and a unit test holds the constants to that. After
the rename, a stop skips the rest of the clean-up, and `build` exits 0.

**A CLI that dies.** A build VM is held by a [VM shim](#the-vm-shim), so a CLI that is
killed, or crashes, leaves its build VM to the shim, which stops and deletes it within
15 s. A shim that is killed too leaves it to the next `build`, which deletes every build
VM by name form.

**Keep-awake.** `build` holds no keep-awake and ignores the sleep and network signals,
which only a running instance reacts to. A Mac that sleeps during a build stops the VM's
work, and the build fails or is stopped; the host document tells the operator to keep
the Mac awake.

**Windows.** `build --profile axmc` on Windows exits 1 before the lock, the
configuration and any engine, with `error: the axmc profile runs only on macOS`. The
code compiles on Windows and holds the VM with `VmLauncher::Direct` there, which no run
reaches.

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
  client is killed, so the next `start`'s stale cleanup removes it. The Job Object
  against Docker Desktop is not yet verified on axpc
  ([#56](https://github.com/axross/runners/issues/56#issuecomment-6511420845)): the
  hard-kill check was not run there, by the maintainer's decision.
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
- **VM shim.** A job VM's `tart run` is held by the VM shim, the CLI run again as
  `marshal vm-shim`, with a liveness pipe of its own, as [The VM Shim](#the-vm-shim)
  describes. It stops and deletes its VM within 15 s when the pipe closes, reporting
  the lost evidence of a diagnostics-enabled pool's VM on the standard error it shares
  with `start`, and holds no lock.
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
the close event. On axpc closing the window worked as described (maintainer's
report, [#56](https://github.com/axross/runners/issues/56#issuecomment-6511420845)),
but the diagnostic containers' kill within the 4 s is not yet verified on axpc
([#56](https://github.com/axross/runners/issues/56#issuecomment-6511420845)): the
report gave no timing.

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

`axmc_start.rs` runs `start` on the axmc profile as real processes, with the fake
`tart`, the fake `softnet` and the fake `docker` in the VMs: a configuration of
Linux and macOS pools that starts only the Linux pools, with one skip warning per
macOS pool, and one of macOS pools alone that exits 1; a missing `tart`, a missing
`softnet`, version refusals, Softnet setup refusals and a `softnet` that does not
answer, each exiting 1 before any VM is cloned or booted; a `tart` that does not
answer, which makes the slots wait; a newer patch, with its warnings in the log and
`status`; a restart and two `start`s that a refused prerequisite leaves stopped with
its reason, the last resuming; and a refusal after `start` that holds a slot while
another slot's job ends, and lets it boot again. `prerequisite_gate.rs` shows where
the instance asks for the check, on the fake seams.

`instance_signals.rs` wires the signals into the instance on the fake signal source,
backend, core and clock: a wake that stops and deletes every VM, a primary network
that differs on two checks (a VPN taking the default route, a lost route naming
`none`) and on one check, the same network, a hardware address that is not read, a
resume that arms the sleep baseline and records the network afresh, the gate that
opens at the second check of a first network, and the keep-awake hold held through
running and stopping, released at stopped, and retried after a failure.
`axmc_signals.rs` runs the same as real processes with the scripted source: a wake and
a network change stop the instance with their reasons, no VM is cloned and `status`
waits for a network until one is there, a stop ends that wait, and the hold's state
is read from the scripted source's file.

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
`golden_build.rs` runs the axmc build ([The Golden VM Build](#the-golden-vm-build))
against the fake `tart`, `softnet` and `docker`, which are scripted per step: the order
of the calls and the build VM's name; a failure scripted at the pull, the clone, the boot,
each provisioning call, the engine wait, the image build, the contract check, the tag, the
`sync` before the stop, the shutdown and the rename, each of which deletes the build VM
and keeps the old golden VM, with the rename only after success; a key with another fingerprint and an empty listing,
which fail before anything is installed; a stop at the pull, the clone, a provisioning
call and the image build, each within the stop sequence; a stop after the rename;
leftover build VMs deleted before the pull; the other golden VMs and only the retired
pins deleted after the commit; `TART_NO_AUTO_PRUNE` and `PATH` on every call; and, as a
real process under the VM shim, a missing `tart` or `softnet` exiting 1 before any VM
boots, the default and the configured image name, SIGINT within 30 s, and a killed CLI
whose build VM the shim deletes. `real_docker_build.rs` runs `build` through a real Docker daemon on Linux, with the
test definition in `cli/tests/build-definition/`, the runner image's digest-pinned
base and its declared labels, and fails unless the result carries all five labels,
no temporary tag remains, and the container backend's image check finds the image
matching and an absent tag missing; it is `#[ignore]`d for a local `cargo test`.
Verified on axpc (maintainer's report,
[#56](https://github.com/axross/runners/issues/56#issuecomment-6511420845) and
[#56](https://github.com/axross/runners/issues/56#issuecomment-6511344317)): Ctrl+C
and, apart from its timing, the console close on Windows, and a `build` of the full
image on Docker Desktop. Ctrl+Break is not yet verified on axpc
([#56](https://github.com/axross/runners/issues/56#issuecomment-6511420845)): it
could not be tested. Not verified: macOS and Windows behaviour beyond what the
hosted runners show.

## Fixtures and Tests

[`tests/host-configuration/`](../../tests/host-configuration) holds every fixture
and one [`expectations.json`](../../tests/host-configuration/expectations.json),
which the CLI's tests read:

- `shared/accepted/` and `shared/rejected/` hold the axpc rule fixtures.
- `shared/pwsh7/` holds three more axpc fixtures: `1e400` as a `cpus` literal and
  two duplicate-key cases. The directory's name is historical and stays until the
  fixtures are reshaped.
- `rust/accepted/` and `rust/rejected/` hold the axmc and the other CLI-only rule
  fixtures.
- `rust/raw/` holds the inputs under Inputs the Fixtures Pin. `.prettierignore` and
  `.gitattributes` (`-text`) keep their bytes unchanged.

Each expectation names the fixture's profile, its outcome, and `contains` and `lacks`
string arrays. The tests read the file into derived types through the crate's serde
reader, with exact key spelling, so an unknown or misspelled key fails the read.
Expected strings are ASCII, with newlines as JSON `\n` escapes. The tests fail when a
fixture has no expectation or an expectation has no fixture. The byte-order-mark
input is built at run time from the axpc example, because a formatter or editor
would strip a stored mark.

### TOML Fixtures

The TOML format has three sets of fixtures, which the tests read through
`validate_toml`. The TOML problems are new, so they have fixtures of their own,
beside the JSON ones and not in place of them. `.gitattributes` pins everything under
`tests/host-configuration/toml/` to LF (`text eol=lf`), unlike the `-text` raw JSON
fixtures, because the tests compare these files' bytes and columns and a checkout
that converts them to CRLF, as a Windows one can, would break them. The tests that
need CRLF build it from the stored text.

- **Hand-written TOML.** [`tests/host-configuration/toml/`](../../tests/host-configuration/toml)
  holds `accepted/` and `rejected/` documents and one `expectations.json`, read by
  `cli/tests/toml_fixtures.rs`. These are the cases only TOML has: syntax errors,
  `schema_version`, `routing_label`, `[lifecycle]`, letter case, duplicates, wrong
  types including a date, nesting deeper than the parser allows, in an array, an
  inline table, a dotted key and a table header, each placed on its line, the
  problem order, the lines, and a token-shaped value or key. Each expectation names the profile
  (`axpc` for Windows, `axmc` for macOS), the outcome, the problems whole, with their
  lines and in order, and `contains` and `lacks` strings checked against the problems
  and, for an accepted file, a description of its plan. An unknown or misspelled key
  fails the read, and the test fails when a fixture has no expectation or an
  expectation has no fixture. Some inputs are built at run time because a stored file
  could not hold their exact bytes: a byte-order mark, a file of exactly 1 MiB, a file
  that is not UTF-8, a control character, and a token pasted into a document at every
  position.
- **Translated JSON.** `cli/tests/toml_equivalence/` turns each shared JSON fixture
  into a TOML document at test time and checks it against the JSON outcome. The
  translator, `translate.rs`, works by structure: it renames the keys, joins `owner`
  and `repository`, drops `tokenPath`, and adds `schema_version` and the profile's
  routing label. It stores nothing, so a fixture that a later change migrates is
  translated again and not mirrored by hand. The same test converts every accepted
  fixture that translates, on each host it is valid on, and checks the document.
- **Expectations of their own.** `tests/host-configuration/toml/translated/expectations.json`
  holds the TOML outcome of the fixtures whose outcome differs, each with a `reason`.

**Same outcome** means that both readers accept the file or both reject it. When both
reject it, the problems are the same and in the same order once the paths and names
are translated. When both accept it, the plans are equal apart from the source path,
`tokenPath`, and an `os` that TOML defaults to Linux where axpc's JSON has none.

Five fixtures have an expectation of their own, because the TOML rules differ:

| Fixture                                    | TOML outcome                                                                     | Reason                                                                                                       |
| ------------------------------------------ | -------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------ |
| `rust/rejected/axmc-os-missing.json`       | Accepted                                                                         | `os` may be left out, and a pool without it is a Linux pool.                                                 |
| `shared/rejected/missing-volumes.json`     | Accepted                                                                         | `volumes` may be left out, and a pool without it has none.                                                   |
| `shared/rejected/missing-owner.json`       | `pool[0].repository: must be "owner/name"`                                       | The owner is part of `repository`, so a missing owner is reported there.                                     |
| `rust/rejected/axpc-axmc-only-fields.json` | Only `budget` and `pool[0].disk_gb` are unknown                                  | `os` is a key a Windows pool defines, and it accepts `"linux"`.                                              |
| `shared/rejected/field-name-case.json`     | The wrong-case keys are unknown, and the keys they stand for are missing as well | TOML keys match in exact case, where JSON finds a known field in any case and reports only the unknown ones. |

**The fixtures that do not map** stay JSON-only. The test lists all 42 by name with
the category that keeps each out, and fails when a fixture that cannot be translated
is not listed, when a listed one can be, or when its category is wrong. The
categories are:

- **Raw syntax that is not JSON:** bytes the JSON reader refuses as a document, such
  as comments, trailing commas, quoting, number forms, invalid UTF-8, two keys that
  differ only in case, and an empty key.
- **A root that is not an object.** TOML cannot express it, because its root is
  always a table.
- **JSON `null`.** TOML has no value for it.
- **A number TOML cannot hold,** such as the `1e400` literal, which the TOML reader
  reports as a syntax error.
- **`tokenPath` and token files.** The TOML format has neither, so a fixture whose
  problem is about one is not translated. `rust/rejected/axmc-budget-errors-come-last.json`
  is one, so the hand-written `rejected/macos-clashes-come-before-budget-totals.toml`
  pins for TOML that the clashes between pools come before the budget totals, though
  the budget's line is the earlier one.

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

Each Rust task first checks for `cargo`, `rustfmt`, and `clippy` and fails at once
with one line naming `.agents/install-rust`. A task never skips when the toolchain is
missing, because a gate that silently does not run would let a session report a
change as checked when it was not.

The `rust` job in
[`merge-checks.yaml`](../../.github/workflows/merge-checks.yaml) runs format check,
clippy, and tests on Linux, Windows, and macOS; the axpc checkout-rule cases that
need drive-letter paths run only on Windows. Four more steps run on Linux only: the
base-address, instance-roots, build-definition, release-overrides, and
agent-overrides tests again with `--release` and `MARSHAL_EXPECT_RELEASE=1`, which
makes each fail if built in the wrong profile;
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
Tests Cover says what each shows. The `fake-docker`, `fake-tart`, `fake-softnet`,
`fake-macos` and `instance-helper` helpers are
further binaries in the package (`cli/helpers/`, `test = false`) that only the tests
and those steps run, and `default-run = "marshal"` keeps `cargo run` on the CLI.
`instance-helper` does nothing in a release build. A release MUST NOT ship any
helper; [Releases](#releases) says how the release build keeps them out. Tests find fixtures relative to the
crate's manifest directory, and the checkout-rule tests build a temporary directory
with the marker files at test time. The advisory check, `cargo deny --locked check advisories`,
fetches the RustSec database, so it runs in
[`rust-advisories.yaml`](../../.github/workflows/rust-advisories.yaml) and not in
`mise run check`; see [Development Workflow](./development-workflow.md).

## The App Workspace

[`app/`](../../app) is a second Cargo workspace beside `cli/`: the Windows
notification-area app, which is also the executable that runs the resident agent.
[App](./app.md) owns what it does. This section owns what the library gives it and
the gates that cover it, so a change to the library knows what the app relies on.

**Contract.**

- The workspace depends on the `marshal-cli` library by path, through the crate's
  public items only, so the app's lockfile, `cargo deny` run and
  advisory check cover everything its executable links, the library's crates
  included. It never runs the `marshal` binary.
- No file under `cli/src` moves for the app. The library's additions for it are
  small exports: the stop reason codes as `agent::StopReason`, the handshake
  timeout `agent::HANDSHAKE_TIMEOUT`, the registration error's marker of a token
  GitHub rejects as `runtime::registration::REJECTED_MARK`, the start of a pool's reason
  when the token store failed as `agent::tokens::STORE_FAILED`, the failure that stops an
  instance as `instance::restart::STOPPING_FAILURE`, beside the already public
  `WINDOW` and `DELAYS`, whose figures the app's banners quote, and the names of the
  debug-only test variables as `command::TEST_ROOT_VARIABLE`, `TEST_DOCKER_VARIABLE`
  and `TEST_GITHUB_API_VARIABLE`. A later change to a public item the app uses
  breaks the app's build, which `mise run check` and the Windows job show, so the
  change and the app move together.
- The app reads the library for the protocol's types and client, the agent's entry
  and options, the token store's interface and refusals, the instance roots, the
  hidden `export`, `shim` and `vm-shim` commands, the `Secret` type that masks a token,
  the executable's identity (`agent::executable::Executable` and `Look`), the private
  log folder and file mode (`private::current_uid`, `ensure_private_directory` and
  `FILE_MODE`), and the log's timestamp (`runtime::name::stamp`). It adds no message to
  the protocol: a new message follows [Version Rules](#version-rules).
- `cli/helpers/fake_docker.rs` is also built as the `fake-docker` binary of the app's
  executable package, so the app's tests can run the exporter child against a fake
  engine. The source stays under `cli/`; the CLI's own tests build it as before.
- The app has its own `Cargo.lock`, [`deny.toml`](../../app/deny.toml) and
  [`rust-toolchain.toml`](../../app/rust-toolchain.toml). The toolchain file names
  the CLI's channel, and Dependabot is set to bump both files in one pull request.
  The app's lockfile resolves every crate `marshal-cli` builds with in the CLI, for
  Windows, macOS and Linux, to the version in the CLI's, so the CLI's tests cover the library
  at the versions the app links; a change to one of those versions changes both
  lockfiles. The CLI's
  `Cargo.lock`, `deny.toml`, and dependency table do not change for the app: the
  table under Dependencies lists the CLI's crates only, and
  [Security](../conventions/security.md#the-apps-dependencies) holds the policy for the
  app's own.
- Every app crate forbids `unsafe_code` and takes the CLI's lint set through
  `[lints] workspace = true`.

**Gates.** `mise run check` covers what builds without the web view's system
libraries, so the Check job on Linux needs none; the Windows job covers the rest.

| Task                             | Runs                                                                                                                                                                                                 |
| -------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `fetch:app`                      | `cargo fetch --locked` in `app/`, which `cargo deny` reads                                                                                                                                           |
| `format:app`, `format:check:app` | `cargo fmt --all`, and `cargo fmt --all --check`, in `app/`                                                                                                                                          |
| `lint:app`                       | `cargo clippy --locked --all-targets -- -D warnings` over the `marshal-modes`, `marshal-view`, `marshal-session` and `marshal-tray` packages                                                         |
| `test:app`                       | `cargo test --locked` over the same four packages                                                                                                                                                    |
| `test:app-frontend`              | `node --test` over the main window page's own tests, in `app/crates/app/tests/frontend/`                                                                                                             |
| `deny:app`                       | `cargo deny --locked check bans licenses sources` in `app/`, for `x86_64-pc-windows-msvc` and `aarch64-apple-darwin`                                                                                 |
| `check:app-pins`                 | Both toolchain files name one channel, and `app/tauri-cli.version` holds one exact version                                                                                                           |
| `test:tauri-cli-version`         | The script that reads the pin, against files that are not one exact version                                                                                                                          |
| `check:app-lock`                 | `app/Cargo.lock` resolves the crates `cli/Cargo.lock` resolves for `marshal-cli`, dev-dependencies aside, for the three targets to the same versions; crates the app's features add are not compared |

`format:check` and `lint` call the app's formatting and lint tasks, and `check` calls
the rest, so one run covers both workspaces. The Tauri crate, `marshal-app`, needs the
web view and is not in `lint:app` or `test:app`.

The `app` job in [`merge-checks.yaml`](../../.github/workflows/merge-checks.yaml) runs
on a Windows hosted runner: clippy and the tests over the whole workspace, then
`cargo tauri build --no-bundle`, failing if the build changed or left a file. An
`app-modes` job runs clippy and the tests of `marshal-modes` on a macOS hosted runner,
which covers the dispatch of both Unix shims, `shim` and `vm-shim`, there, and runs the
crate's tests again in the release profile, where the debug-only variables are
ignored; it never builds the Tauri crate. Linux runs the dispatch tests in the debug
profile in `mise run check`. The advisory check,
`cargo deny --locked check advisories` in `app/`, runs in
[`rust-advisories.yaml`](../../.github/workflows/rust-advisories.yaml) beside the CLI's
and is not part of `mise run check`; see [Development Workflow](./development-workflow.md).
[Development Workflow](./development-workflow.md) describes each job's permissions,
and [App](./app.md#building-the-app) says how the app is built and which pin its
build tool has.

## Releases

A release is a `v<version>` tag on the default branch and the GitHub release that
carries its files, such as `v0.1.0`. The maintainer starts one by dispatching
[`release.yaml`](../../.github/workflows/release.yaml) on main, and semantic-release
works out the version from the commits since the last `v*` tag. Releases are never
prereleases. The run has seven jobs, three of which sign the macOS files, check
them, and assemble the signed set, once the maintainer approves the first in the
`release` Environment, as [Signing the macOS Files](#signing-the-macos-files)
describes.
[Development Workflow](./development-workflow.md#cutting-a-release) gives the
maintainer's steps and the version rules, and
[Security](../conventions/security.md#a-release-is-published-only-from-a-dispatch-on-main)
records the controls around it.

The version is never committed. `cli/Cargo.toml` and `cli/Cargo.lock` on main hold
the placeholder `0.0.0-dev`, so a local build prints `marshal 0.0.0-dev`. Each
release build leg writes the version over the placeholder in the runner, with
[`.github/scripts/set-release-version.sh`](../../.github/scripts/set-release-version.sh),
before it compiles. The script refuses, changing neither file, unless the version
is `X.Y.Z` or `X.Y.Z-<prerelease>`, each file has exactly one version line for the
CLI, and that line holds the placeholder. It writes through a temporary file, so it
runs with the BSD tools on macOS and keeps the CRLF line endings of a Windows
checkout. `--locked` then fails if the two files disagree, and
`marshal --version` prints `marshal <version>`, from the same field.
`mise run test:set-release-version` tests the script.

Each release carries five files, and nothing else:

| File                                | Built on         | Compiler host target     |
| ----------------------------------- | ---------------- | ------------------------ |
| `marshal-<version>-windows-x64.exe` | `windows-latest` | `x86_64-pc-windows-msvc` |
| `marshal-<version>-windows-x64.msi` | `windows-latest` | -                        |
| `marshal-<version>-macos-arm64`     | `macos-latest`   | `aarch64-apple-darwin`   |
| `marshal-<version>-macos-arm64.pkg` | `macos-latest`   | -                        |
| `SHA256SUMS`                        | `ubuntu-latest`  | -                        |

The binaries are bare files, not archives. The `.msi` and the `.pkg` are installers
that hold the binary of the same target unchanged. `SHA256SUMS` is in the
`sha256sum` format, one line for each of the other four files, computed after
signing.

- **macOS.** The binary is signed with a Developer ID Application certificate, under
  the fixed identifier `app.axross.marshal`, with the hardened runtime and a secure
  timestamp. The package is signed with a Developer ID Installer certificate,
  notarized by Apple's notary service, and stapled. The `sign-macos` job does this
  on `macos-latest`, after the build and before the checksums, as
  [Signing the macOS Files](#signing-the-macos-files) describes. A release run is the
  first place the signing runs and no Mac has yet assessed a signed package, so that
  Gatekeeper accepts it is an expectation, not an observation.
- **Windows.** Neither the binary nor the MSI is code-signed. Windows signing is
  deferred until a need arises, such as Smart App Control being On, so the MSI raises
  the prompts that [Installing](#installing) describes.

The linker's ad-hoc signature on a macOS binary is replaced by the Developer ID
one. The `v0.1.0` binary predates signing and keeps the ad-hoc one.

[`release-build.yaml`](../../.github/workflows/release-build.yaml) builds them. It
is a reusable workflow with `contents: read` and no publish or attest step, and its
caller passes the version. Each target builds natively on its hosted runner, with
no cross-compilation, in one leg that:

1. fails unless `rustc`'s host target is the one in the table, so a change of
   runner image fails the build instead of shipping the wrong architecture;
2. writes the version with `set-release-version.sh`;
3. runs the base-address, instance-roots, build-definition, release-overrides, and
   agent-overrides tests with `--release` and `MARSHAL_EXPECT_RELEASE=1`, the same
   tests the Linux `rust` job runs, on the target itself;
4. builds `cargo build --locked --release --bin marshal`;
5. fails unless the binary's `--version` prints `marshal <version>`, with the
   version its caller passed;
6. copies the binary to its release name, and on Windows fails if it imports a
   `vcruntime` DLL;
7. builds the installer from the binary, as [Installers](#installers) describes,
   and runs the installer tests there;
8. on the macOS leg, runs the "Check The Signing Tools" step, which holds no secret
   and prints a `MEASURED: signing tools:` line with the path of each tool that
   `sign-macos` and `verify-macos` call. It fails the leg if one is missing, and
   runs `base64` and `shasum` against the flags those jobs use, so a tool that moves
   on a new runner image fails a pull request before it fails a release;
9. hands the SHA-256 of the binary and of the installer to the next job as job
   outputs, and uploads those two files by name.

An assemble job then downloads both legs' files and runs
[`assemble-release.sh`](../../.github/scripts/assemble-release.sh). The script fails
unless the files are exactly the four expected ones and each SHA-256 matches the
handed-over value, writes `SHA256SUMS` with a line for each in a fixed order, checks
it with `sha256sum --check --strict`, and the job uploads the five files as one
artifact, `marshal-release`. That artifact is the unsigned build set: the dry run
uploads it, and a release's `sign-macos` job starts from it. The `assemble-signed`
job of `release.yaml` calls the same script, so one implementation writes every
`SHA256SUMS`. `mise run test:assemble-release` tests it.

The Windows binary links the C runtime statically: `+crt-static` is set for
`x86_64-pc-windows-msvc` in [`cli/.cargo/config.toml`](../../cli/.cargo/config.toml),
so `marshal.exe` needs no Visual C++ redistributable on the host. It is set there,
not in a workflow, so the debug tests, the release-profile tests, and the release
build are compiled the same way. The import check runs `dumpbin /dependents`, found
through `vswhere` among the MSVC tools the build links with. It fails unless
`dumpbin` listed the binary's dependencies, `KERNEL32.dll` among them, so a run that
read nothing cannot pass.

A release never ships a helper: `fake-docker`, `fake-tart`, `fake-softnet`,
`fake-macos` or `instance-helper`. The release-profile tests build them into
`target/release`, so the build names `marshal` alone and copies only that file out,
each leg's artifact holds that binary and its installer, and the assemble job, the
attest job, and the release gate refuse a set of files that holds anything else.

### Installers

Each leg packs its binary into an installer, unchanged, and signs neither: the signing
happens later and only for the macOS files, as [Signing the macOS
Files](#signing-the-macos-files) describes. The installed file is byte-identical to
the attested bare binary, which on macOS is the signed one. An installer's own version
is the release's `X.Y.Z`, and a dry run's is `0.0.0`, while its file name keeps the
full dry-run version.
[`installer-version.sh`](../../.github/scripts/installer-version.sh) maps the version
for both legs: it keeps a release `X.Y.Z`, maps `0.0.0-dryrun.<n>` to `0.0.0`, and
refuses any other prerelease form and an `X.Y.Z` beyond a Windows Installer
`ProductVersion`'s limits (major and minor at most 255, patch at most 65535).
`mise run test:installer-version` tests it.

- The MSI is built by
  [`build-msi.ps1`](../../.github/scripts/build-msi.ps1) from
  [`marshal.wxs`](../../.github/scripts/marshal.wxs) with WiX, which
  [`fetch-wix.sh`](../../.github/scripts/fetch-wix.sh) installs from a pinned and
  checked package; [Security](../conventions/security.md#pinning-convention)
  records the pin. It is a per-user package named `marshal`, manufacturer `axross`,
  with a fixed upgrade code that never changes. It installs
  `%LOCALAPPDATA%\Programs\marshal\marshal.exe`, appends that directory to the
  user `PATH` and removes the entry on uninstall, refuses a lower version with the
  message "A newer version of marshal is already installed.", and replaces the
  installed product when a build of the same version is installed. It has no
  dialogs and no UI extension, and its only custom action is the error message
  for the downgrade, which runs no code.
- The pkg is built by [`build-pkg.sh`](../../.github/scripts/build-pkg.sh) with
  `pkgbuild`, identifier `app.axross.marshal`, `--ownership recommended`, and a
  payload of `usr/local/bin/marshal` with mode 0755. It has no scripts. The leg builds
  its package without signing it. `sign-macos` builds the release's package again from
  the signed binary with the same arguments plus the Installer identity, and
  `verify-macos` compares the two.

[`test-msi-installer.ps1`](../../.github/scripts/tests/test-msi-installer.ps1)
runs on the Windows leg with the real `msiexec`, and
[`test-pkg-installer.sh`](../../.github/scripts/tests/test-pkg-installer.sh) on the
macOS leg with the real `installer` and passwordless `sudo`. Each builds throwaway
installers at fixed test versions from two copies of the binary that differ in
trailing bytes, and judges every install by the installed file's SHA-256; those
copies are packed and hashed, never run. They check the release installer's
install, version, `PATH`, listing and byte identity, an upgrade, the extraction
layout the optional check relies on, and an uninstall that keeps the logs, and
that the installer carries no script, no code-running custom action and no
dialogs and names only `marshal`, `axross` and the repository. The one exception is
the package archive: `pkgbuild` records the building account as each entry's owner,
which on the build legs is `runner`, the account of every GitHub-hosted runner, and
the macOS test checks that `runner` is the only owner. The Windows test
also builds a same-version rebuild, a lower version over a higher one, and an
upgrade while a copy of a stock waiting console program, packed as `marshal.exe`,
runs from the install path. The macOS test also records the owner and mode of
`/usr/local/bin` before and after the install, the bill of materials' entries, a
login zsh finding `marshal` by name, and what a lower package does over a higher
one. Its uninstall runs the uninstall block of this document, extracted from it.
Each measured outcome is a log line that starts with `MEASURED:`, and the
installer logs are kept as the `installer-test-logs-<target>` artifact of the run.
A failed check fails the leg.

### The Dry Run and the Release Workflow

[`merge-checks.yaml`](../../.github/workflows/merge-checks.yaml) calls the build
workflow on every pull request and every push to the default branch, as the dry
run, with the version `0.0.0-dryrun.<run number>`: both legs and the assemble job
run, each binary must print `marshal 0.0.0-dryrun.<run number>`, the installers are
built and tested, and the `marshal-release` artifact, with all five files, is
uploaded to the run, but nothing is signed, attested or published, and no job
references an Environment. The `Install Blocks` job
then runs the documented install blocks against that artifact, as
[What the Install Tests Cover](#what-the-install-tests-cover) describes.
[`release.yaml`](../../.github/workflows/release.yaml) runs only on a
manual dispatch on main, in seven jobs on the dispatched commit. Every job but
`plan` runs only when the plan job handed out a version, and every job after `build`
needs `plan` directly:

1. `plan` runs semantic-release as a dry run. Its gate,
   [`.github/scripts/verify-release.sh`](../../.github/scripts/verify-release.sh),
   which `@semantic-release/exec` runs from
   [`release/.releaserc.json`](../../release/.releaserc.json), hands out the next
   version. With nothing to release, it hands out none, and the other jobs are
   skipped.
2. `build` is the build workflow, which builds and assembles the unsigned files at
   that version into `marshal-release`.
3. `sign-macos` signs and notarizes the macOS binary and package in the `release`
   Environment, once the maintainer approves it, as
   [Signing the macOS Files](#signing-the-macos-files) describes.
4. `verify-macos` checks the signed package on a runner that holds no signing
   identity.
5. `assemble-signed` writes `SHA256SUMS` over the files that are published: the
   Windows files as the build made them and the macOS files as `sign-macos`
   signed them. It uploads them as `marshal-release-signed`.
6. `attest` downloads `marshal-release-signed` and fails unless it holds exactly
   that version's five files, `SHA256SUMS` lists exactly the two binaries and the
   two installers, and `sha256sum --check --strict` passes. It then attests the
   build provenance of those four files with `actions/attest`. It checks nothing out
   and installs no npm package.
7. `release` downloads `marshal-release-signed` and runs semantic-release. The gate
   makes the same check for the version semantic-release worked out in that run,
   before anything is tagged. semantic-release then pushes the tag and its note, and
   `@semantic-release/github` creates a draft release with the generated notes,
   uploads the five files, and publishes it. `mise run test:verify-release` tests
   the gate.

Every signing step comes before `release`, so a signing, notarization or
verification failure stops the run before any tag is pushed. The jobs and their
`needs` chain are the contract that W1, W4, M2 and M4, other children of roadmap
[#72](https://github.com/axross/runners/issues/72), extend
([Extending the Release Workflows](#extending-the-release-workflows); [The Signing
Contracts](#the-signing-contracts) says what each of them is), and
`mise run check:release-shape` fails if they drift from it.

The runner image's definition is embedded in the binary (see [Build](#build)), so a
change to it, such as a runner bump, reaches a host only through a new release and
an update.

### Signing the macOS Files

Three jobs of `release.yaml` sit between `build` and `attest`. The maintainer's
approval of the first one is the decision to sign.

| Job               | Needs                                         | Runs on, timeout            | Permissions      | Environment | What it holds                                                                                                                                                                                                                    |
| ----------------- | --------------------------------------------- | --------------------------- | ---------------- | ----------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `sign-macos`      | `plan`, `build`                               | `macos-latest`, 60 minutes  | `contents: read` | `release`   | The six signing secrets and `MACOS_TEAM_ID`. Downloads `marshal-release`; uploads `marshal-macos-arm64-signed` and `notary-log-macos`. It checks nothing out.                                                                    |
| `verify-macos`    | `plan`, `build`, `sign-macos`                 | `macos-latest`, 20 minutes  | `contents: read` | none        | No secret and no signing identity. Checks out the repository with `persist-credentials: false`, downloads three artifacts, and runs `test-signed-pkg.sh` with the team ID and the notary submission ID that `sign-macos` output. |
| `assemble-signed` | `plan`, `build`, `sign-macos`, `verify-macos` | `ubuntu-latest`, 10 minutes | `contents: read` | none        | Checks out the repository with `persist-credentials: false`, runs `assemble-release.sh`, and uploads `marshal-release-signed`.                                                                                                   |

`attest` then needs `plan` and `assemble-signed`, and `release` needs `plan` and
`attest`. Both read `marshal-release-signed`, and `marshal-release` stays the
unsigned build set. `attest` keeps `id-token: write` and `attestations: write`
alone. The kept uploads, the signed files, the notary log and `marshal-release-signed`,
hold no secret. The workflows ask GitHub to keep these uploads for 14 days. GitHub
keeps them for the repository's artifact retention limit instead when that is
shorter. That limit cut the installer-log uploads of a pull request run to 7 days.

`sign-macos` is the one job that declares the Environment or reads a signing secret.
[Security](../conventions/security.md#a-release-is-published-only-from-a-dispatch-on-main)
owns what it may hold and run: `contents: read`, no checkout, no repository file, no
package manager, no fetch, and no action but `actions/download-artifact` and
`actions/upload-artifact`. Its steps are the job's own definition in `release.yaml`,
and its only network use is the artifact transfer and Apple's timestamp, notary and
ticket services. Each signing tool is called by its absolute system path, `/usr/bin/`
for `security`, `codesign`, `pkgbuild`, `openssl`, `plutil`, `shasum` and `xcrun`, and
`/usr/sbin/` for `pkgutil`, so nothing earlier on `PATH` can stand in for one. The
scripts read the two prefixes from `SYSTEM_BIN` and `SYSTEM_SBIN`, whose defaults are
those literal paths and which only the stand-in test overrides; the shape check
requires the defaults and rejects either name as an `env:` key anywhere. Its steps run
in this order:

1. It downloads `marshal-release`, the build's set.
2. It refuses, before importing anything, unless:
   - every secret and `MACOS_TEAM_ID` is non-empty, and `MACOS_TEAM_ID` is 10
     characters of A-Z and 0-9;
   - the version is `X.Y.Z`;
   - the downloaded files are exactly the five of the release, all regular files;
   - the macOS binary and package match their lines in that `SHA256SUMS`, so it signs
     what the build produced and checked;
   - every tool exists at its path, and `xcrun --find` finds `notarytool` and
     `stapler`.
3. It creates a temporary keychain with a random password that the log masks. It
   refuses the embedded Developer ID G2 intermediate unless its SHA-256 equals the pin
   in [Security](../conventions/security.md#pinning-convention), and that check comes
   before any `security` call. It then imports the intermediate and both `.p12` files.
   The intermediate is embedded because the runner image's trust in it is unknown and
   the job fetches no certificate. It expires on 2031-09-17, and replacing it is a change
   to the job and to the pin.
4. It lists the keychain's identities and refuses unless there is exactly one
   Developer ID Application identity and exactly one Developer ID Installer identity,
   and each must be valid. For each it prints the subject, team and expiry date of the certificate
   whose SHA-1 hash equals the selected identity's, found with
   `security find-certificate -a -Z -c <name>`. It refuses unless the name matches
   exactly one certificate, because `pkgbuild` signs by name. It refuses a certificate
   whose team differs from `MACOS_TEAM_ID` and an expired one, and it warns with a
   workflow warning annotation when a certificate expires within 90 days.
5. It signs a copy of the binary with the Application identity, selected by its SHA-1
   hash, `--options runtime`, `--timestamp` and `--identifier app.axross.marshal`. The
   signature replaces the linker's ad-hoc one. It then runs
   `codesign --verify --strict` and prints the designated requirement.
6. It builds the package from the signed binary. The staging layout and the `pkgbuild`
   arguments repeat
   [`build-pkg.sh`](../../.github/scripts/build-pkg.sh)'s, because the job runs no
   repository file, and `--sign` and `--keychain` are the only additions. `pkgbuild`
   takes the Installer identity by its common name, the one form its manual gives, and
   `codesign` takes the Application identity by hash. The name cannot be another
   identity's: step 4 has shown that the temporary keychain holds one certificate of
   that name, and `--keychain` keeps `pkgbuild` to that keychain. The shape check
   fails if the two argument lists differ, and `verify-macos` compares the signed
   package with the unsigned one.
7. It notarizes the package with
   `xcrun notarytool submit … --wait --timeout 30m --output-format json`, and reads
   the result with `plutil`. It refuses unless the status is `Accepted`, whatever
   `notarytool` exited with, and a timeout is therefore a failure. Whenever a
   submission ID exists it fetches the notary log, which Apple says to read even
   after a success, and which lists each ticketed CDHash.
8. It staples the package, runs `stapler validate`, and refuses unless
   `pkgutil --check-signature` names a Developer ID Installer certificate of the
   team.
9. It outputs the two signed files' SHA-256, the team ID and the submission ID, and
   uploads the signed files.

Whether the job passes or fails, the clean-up step deletes the keychain, the `.p12`
files and the key file, and the notary log is uploaded when one exists. These two
steps run with `if: always()`, and no other step in the job may.

No secret is printed. Secret values reach the scripts only through `env:` and files
written with mode 0600 under the runner's temporary directory, and each line of the
`.p8` is masked as well. Two exceptions to "never passed on a command
line" are recorded, the password arguments of `security` and the `--key-id` and
`--issuer` arguments of `notarytool`, and the `.p8` private key reaches `notarytool`
only as a file path.
[Security](../conventions/security.md#nothing-identifying-or-secret-is-committed)
records both and the reasons.

`verify-macos` runs
[`test-signed-pkg.sh`](../../.github/scripts/tests/test-signed-pkg.sh) on a runner
that holds no signing identity. It takes the team ID and the notary submission ID that
`sign-macos` output, the latter through `env: SUBMISSION_ID`. The script refuses unless:

- the unsigned binary and package each match their single line in the unsigned set's
  `SHA256SUMS`;
- the signed package's identifier, version and install location equal the unsigned
  one's, and the identifier is `app.axross.marshal`;
- the two bills of materials list the same paths with the same owner and mode, which
  leaves out the binary's size;
- neither package holds `Scripts`;
- the binary in the signed package is byte-identical to the signed bare binary;
- the bare binary verifies with `codesign --verify --strict`, is signed by a Developer
  ID Application certificate of the team, carries the hardened runtime and a
  timestamp, and its designated requirement is one of the two known default forms for
  the identifier and the team ([The Signing Contracts](#the-signing-contracts) names
  them), nothing else being accepted;
- the package's signature is a Developer ID Installer chain of the team to Apple's
  root, `stapler validate` passes, and `spctl --assess --type install` accepts it;
- the notary log's `jobId`, read with `plutil`, equals the submission ID `sign-macos`
  output, ignoring case, so the log is of that submission, and it lists the bare
  binary's CDHash.

It then installs the signed package with `installer -pkg`, runs `codesign --verify` and
`--version` on the installed file, and removes it. It prints `MEASURED:` lines for
which form of the designated requirement matched, the requirement itself, and
`spctl`'s answer. That is the first time the signed
package is checked, and it happens in the release run: a dry run cannot sign, and the
signed package has not been opened on a Mac.

Nothing here has run on macOS or in Actions. The scripts read the output of
`security find-identity`, `pkgutil --check-signature`, `codesign -d`, `spctl` and
`lsbom` as the manuals describe it, and none of those formats is confirmed. Nor is how
long `stapler` takes after an accepted submission, or what the signing certificate's
subject `OU` holds. The first signed release is therefore the first real run of the
job and of `verify-macos`. A defect stops that release before its tag, and a later
pull request fixes it.

### The Signing Contracts

F0 owns these. W1, W4, M2 and M4 consume them. Each is a child of roadmap
[#72](https://github.com/axross/runners/issues/72), as F0 is:

- **F0**, "Installers and release signing", is the child this section describes.
- **W1**, "Windows tray app, first release", has an installer pull request that
  replaces the release's files. Its app's bundle identifier is the identifier
  below, because the app and its agent are one code object. W1 owns that use.
- **W4**, "Automatic updates, on Windows", signs the update archive. The archive's
  signature is W4's contract. W4 makes it in W4's own update-signing job, under the
  rules in [Extending the Release Workflows](#extending-the-release-workflows). M4
  may also sign the Mac's update archive there.
- **M2**, "Mac app", signs the Mac app with the same identifier and team, and
  notarizes and staples its disk image, in F0's signing job. The commands and the
  layout are M2's. M2 also holds the Keychain store. Its Keychain items trust the
  signed Marshal executable, which is the code object that carries the identifier in
  the first contract below, and must be created by F0's stable Developer ID identity.
- **M4**, "Mac app: login, power, updates and telemetry", updates the installed Mac
  app. Its updates keep M2's Keychain tokens working only while the identifier and the
  team stay the same.

The contracts F0 owns are these:

- **The identifier** is `app.axross.marshal`, and it never changes. One code object
  carries it: the `marshal` binary until W1's installer pull request, and from M2 on
  the Mac app bundle's signature, through its main executable. F0 proposes that from
  M2 on `verify-macos` reads the designated requirement from the signed app, and M2's
  Keychain trust matches it. F0 also asks M2's plan to compare, at M2's first release, the
  app's requirement with the one recorded for `marshal`, and to record any difference.
  M2's plan accepts or answers both. The package's own identifier is the same
  string. On Windows nothing is signed, so the identifier binds only the Mac. Changing
  the identifier or the team breaks the Keychain trust that M2 relies on, so a
  change goes through a roadmap revision.
- **The designated requirement** is the default one for a Developer ID signature: this
  identifier, `anchor apple generic`, the Developer ID intermediate and leaf markers,
  and `certificate leaf[subject.OU]` equal to the team ID. Two forms of it are known,
  and `verify-macos` accepts exactly these, after it drops `/* … */` comments,
  collapses whitespace and unquotes the team ID:
  - **Form A**, the expected one, is the plain `codesign` default: the conjunction of
    `anchor apple generic`, `identifier "<id>"`,
    `certificate 1[field.1.2.840.113635.100.6.2.6]`,
    `certificate leaf[field.1.2.840.113635.100.6.1.13]` and
    `certificate leaf[subject.OU] = <team>`, in any order, compared as a set of
    clauses. It is inferred, not yet observed, because TN3127 says plain `codesign`
    applies a different default from Xcode's and does not print it.
  - **Form B** is the requirement Xcode writes for Developer ID code, from TN3127,
    compared as a whole string:
    `anchor apple generic and identifier "<id>" and (certificate leaf[field.1.2.840.113635.100.6.1.9] or certificate 1[field.1.2.840.113635.100.6.2.6] and certificate leaf[field.1.2.840.113635.100.6.1.13] and certificate leaf[subject.OU] = <team>)`.

  The script prints a `MEASURED:` line naming the form that matched, then the
  requirement, so the first signed release records which one it was. Keychain items
  that trust the requirement are expected to keep trusting later releases signed under
  the same identifier and team, including after a certificate renewal; that is
  inferred, not observed. After the first signed release, a later change records the
  team ID, the form that matched, and the exact requirement printed for the bare
  binary here, as the text that M2 consumes.

- **The team** is pinned by the Environment variable `MACOS_TEAM_ID`. `sign-macos`
  refuses a certificate of another team, and `verify-macos` refuses a designated
  requirement that is neither form A nor form B above, each built from the identifier
  and the team ID that `sign-macos` output after checking it.
- **The Environment** is named `release`. The maintainer creates it and its secrets,
  and agents never change them or read the settings:
  - deployment branches: selected branches, `main` only, no tags;
  - required reviewers: the maintainer, with "Prevent self-review" off, so the
    maintainer can approve a run they started;
  - "Allow administrators to bypass": off.
- **The secrets** are Environment secrets only, and no repository secret is added.
  Both certificates come from the Developer ID G2 intermediate.

  | Secret                         | Holds                                                             |
  | ------------------------------ | ----------------------------------------------------------------- |
  | `MACOS_APPLICATION_P12_BASE64` | The Developer ID Application identity as a `.p12`, base64-encoded |
  | `MACOS_INSTALLER_P12_BASE64`   | The Developer ID Installer identity as a `.p12`, base64-encoded   |
  | `MACOS_P12_PASSWORD`           | The password of both `.p12` files                                 |
  | `MACOS_NOTARY_KEY_P8`          | The App Store Connect team API key's `.p8` text                   |
  | `MACOS_NOTARY_KEY_ID`          | The key's ID                                                      |
  | `MACOS_NOTARY_ISSUER_ID`       | The issuer ID                                                     |

  The team ID is the Environment variable `MACOS_TEAM_ID`, not a secret.

- **A missing Environment.** A dispatch that finds no `release` Environment makes
  GitHub create one with no protection rules and no secrets. `sign-macos` then
  refuses on the empty secrets, before it imports anything.
  [Development Workflow](./development-workflow.md#before-the-first-release) has the
  maintainer create the Environment first.
- **Certificate lifetime.** The Developer ID certificates are expected to last one
  year, and the maintainer confirms the dates from the certificates. Software
  signed with a secure timestamp keeps working after its certificate expires.
  Whether an older release's package still installs after its Installer certificate
  has expired is unconfirmed, and bears on rolling back to it.
  [Development Workflow](./development-workflow.md#renewing-the-certificates) says how
  to renew.

### Reading a Signature

These commands show what a release's files carry. They are information, not steps of
the install procedure, and the answers below are what the documentation leads to
expect, not what a Mac has shown.

```bash
pkgutil --check-signature marshal-<version>-macos-arm64.pkg
spctl --assess --type install -vv marshal-<version>-macos-arm64.pkg
xcrun stapler validate marshal-<version>-macos-arm64.pkg
codesign --verify --strict -vv /usr/local/bin/marshal
codesign -d -r- /usr/local/bin/marshal
```

- `pkgutil --check-signature` is expected to show a Developer ID Installer
  certificate of the maintainer's team, chained to Apple's root. An operator who wants
  the team checked compares the team ID it shows with the one this document records
  once the first signed release has fixed it. Gatekeeper is not expected to do that
  check, and it assesses only a file that carries the download mark, which a package
  fetched with `gh` or `curl` lacks.
  [Security](../conventions/security.md#a-release-is-published-only-from-a-dispatch-on-main)
  records what it is expected to block and accept. This is expected, not yet observed
  on a Mac.
- `spctl --assess --type install -vv` is expected to print `accepted` with
  `source=Notarized Developer ID`. One unanswered report says macOS 26.3 rejected a
  notarized, stapled package; if a genuine release shows that, it is a defect to
  record.
- `xcrun stapler validate` passes for a stapled package.
- `codesign -d -r-` on the installed or the bare binary prints the designated
  requirement of the identifier contract. `spctl --type execute` does not suit a bare
  command line tool, so the binary's notarization is read from its CDHash in the
  notary log instead.

### Extending the Release Workflows

The release workflows' structure is F0's, and W1, W4, M2 and M4 extend them at
these points:

- **W1** adds its installer to the build legs and to the release set. That changes
  the five-file set, so the same change updates it across `release-build.yaml`,
  `assemble-release.sh` and `test-assemble-release.sh`, `sign-macos`'s file check,
  `assemble-signed`, `attest`, `verify-release.sh` and `test-verify-release.sh`,
  `.releaserc.json` and `test-sign-macos.sh`. The install-block suites and
  `test-msi-standard-user.ps1` name the one installer they install, so they change
  only if W1's installer replaces or renames it; `test-signed-pkg-refusals.sh` uses
  one Windows file name only as a fixture for an unlisted file, and needs no change.
  W1's installer pull request replaces the release's files. From it on, the release
  carries W1's Windows app and installer and no macOS file until M2, and
  no macOS command line is built or signed.
- **M2** adds steps to `sign-macos` and `verify-macos` that sign the Mac app with the
  same identifier and team, and notarize and staple its disk image. The commands and
  the layout are M2's.
- **W4** adds its own update-signing job, which roadmap [#72](https://github.com/axross/runners/issues/72) adopted, to sign the
  update archive. The archive's signature is W4's contract.
- **M4** may sign the Mac's update archive in W4's update-signing job.

Every extension keeps these rules:

- only signing jobs declare the Environment;
- a signing job holds `contents: read` and runs no repository code;
- checksums and attestation follow every signing step;
- no pull-request run reaches the Environment;
- F0's and M2's signing jobs use only operating-system signing tools.

W4's update-signing job is the one exception to the last rule. It runs `tauri-cli`'s
`signer sign`. A tool job without the Environment builds that binary with `--locked`
from a crate archive whose SHA-256 must equal one committed in the repository. The
signing job checks the binary it receives against the SHA-256 that the tool job
outputs, before running it. A later revision of F0's plan, before W1's installer pull
request, sets how the job sits beside F0's jobs.

An extension adds its jobs or steps to `check-release-shape.mjs` and its test in its
own pull request, under REVIEW.md's release lens and the maintainer's review. The
shape check guards against mistakes. A pull request can edit it, so it is not the
control against a hostile one: what keeps pull requests away from the Environment is
the maintainer's branch rule and that `release.yaml` runs only on a dispatch. It reads
the commands a signing script runs, but not what they are told to do, so these can
still run code it does not see: the programs of `awk` and `sed`, the operands of a
`[[ ]]` test, of which it reads only the command substitutions, an option or variable
name built at run time, such as `find "$option"`, `read -r "$name"` and
`printf -v "$name"`, an assignment through `${PATH:=...}`, and any environment variable
other than `PATH`, `BASH_ENV` and `ENV`. It also does not see a write to `SHA256SUMS`
in `release-build.yaml`'s `assemble` job other than by a redirection, `tee`, `cp` or
`mv`, or a step condition in `attest` or `release` that skips a step, such as
`if: false`.

### What the Signing Tests Cover

Each of these runs in `mise run check`, on Linux, with no signing identity and no
secret:

| Task                             | What it covers                                                                                                                                                                                                                                                                  |
| -------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `mise run check:release-shape`   | Reads the `.yaml` and `.yml` workflows and the `sign-macos` scripts and fails unless the rules listed under this table hold.                                                                                                                                                    |
| `mise run test:release-shape`    | Runs that check against the unchanged workflows, then against copies altered one change at a time, and expects each to fail.                                                                                                                                                    |
| `mise run test:sign-macos`       | Runs `sign-macos`'s `run:` scripts, extracted from `release.yaml`, against recording stand-ins of the macOS tools: each refusal, the flags, that the keychain and files are deleted after a pass and after a failure at each step, and that no secret or `.p8` line is printed. |
| `mise run test:signed-pkg`       | Runs `test-signed-pkg.sh` against stand-ins and produces each of its refusals, including an unsigned file that `SHA256SUMS` does not list and a notary log of another submission.                                                                                               |
| `mise run test:assemble-release` | Runs `assemble-release.sh` against temporary directories: the four lines in order, and a refusal for another file set or a changed file.                                                                                                                                        |

`mise run check:release-shape` enforces these rules. They apply to every `.yaml` and
`.yml` workflow, unless a rule names a file or job. Each failure names its rule, and
a workflow that cannot be read, a missing `release.yaml` or `release-build.yaml`, and a
missing `build-pkg.sh` fail it too.

- **The Environment.** Only `release.yaml`'s `sign-macos` declares an `environment`,
  and only `release`. No step declares one.
- **Triggers.** `release.yaml` is triggered by `workflow_dispatch` alone. A workflow
  that `pull_request` or `pull_request_target` triggers calls only local workflows
  (`./.github/workflows/...`), and none of them, directly or through another called
  workflow, declares an Environment.
- **The signing secrets.** No workflow but `release.yaml`'s `sign-macos` reads one,
  whether by `secrets.MACOS_*`, `secrets[...]`, `toJSON(secrets)` or
  `secrets: inherit`.
- **The `sign-macos` job.** It holds exactly `contents: read`, declares
  `environment: release`, and runs on `macos-latest`. No step sets `shell:`, and
  neither the workflow nor the job sets `defaults.run.shell`.
- **Its steps.** None checks out the repository. The only actions are
  `actions/download-artifact` and `actions/upload-artifact`, each on a major tag. The
  download takes only the inputs `name` and `path`, and the upload only `name`,
  `path`, `retention-days` and `if-no-files-found`. No `run:` contains `${{ }}`, and
  each begins with a `# sign-macos: <name>` marker line.
- **Its commands.** Each command word, a `trap` body included, is a function the
  script defines, a shell built-in or base utility on a fixed list, or a signing tool
  at `"$SYSTEM_BIN/<tool>"` or `"$SYSTEM_SBIN/<tool>"`: `security`, `codesign`,
  `pkgbuild`, `openssl`, `plutil`, `shasum` and `xcrun` under the first, `pkgutil` under
  the second. A package manager, `curl` or `wget`, an interpreter, `source`, `eval` or
  `exec`, a path into the repository, a signing tool called by its bare name, and any
  other command fail. So do a quoted or expanded command word, a bare command
  substitution where a command word would stand (one attached to a word, as in
  `x=$(...)`, is read as that word's value), a backtick, an arithmetic expansion
  `$(( ... ))` that contains `$(` or a backtick, an unreadable
  `trap` operand, a `find` option outside `-mindepth`, `-maxdepth`, `-type`, `-name`,
  `-print` and `-print0`, and a reference to `.github/`, `GITHUB_WORKSPACE` or
  `github.workspace`. The command substitutions inside a `[[ ]]` test are read as
  commands too; the rest of the test is not.
- **The tool prefixes.** A script that uses `SYSTEM_BIN` or `SYSTEM_SBIN` has the line
  `: "${SYSTEM_BIN:=/usr/bin}"` (`/usr/sbin` for `SYSTEM_SBIN`), gives it no other
  default, and neither assigns nor exports it. No `env:` of any workflow, job or step
  sets either name.
- **The scripts' environment.** No script mentions `GITHUB_PATH`. A script mentions
  `GITHUB_ENV` only in a block of one or more `echo "NAME=${variable}"` lines for
  `APPLICATION_IDENTITY` and `INSTALLER_IDENTITY_NAME`, which is accepted in any
  `sign-macos` script. No script assigns `PATH`, `BASH_ENV` or `ENV`, or starts a
  `read`, `printf -v`, `declare`, `typeset`, `local`, `export`, `readonly`, `mapfile`,
  `readarray`, `getopts`, `for` or `select` command that names one of the three. None
  of the three is an `env:` key at any level of `release.yaml`.
- **Step conditions in `sign-macos`.** A step with any `if` is the clean-up step
  (marker `# sign-macos: cleanup`) or the `notary-log-macos` upload, so a condition
  such as `if: false` or `if: success()` is refused as well as the status functions
  `always()`, `failure()` and `cancelled()`, whatever their case and inner spaces. Each
  of those two steps exists exactly once and has `if: always()`.
- **The check script.** The script marked `# sign-macos: check` lists in `bin_tools`
  and `sbin_tools` every tool the job calls by `$SYSTEM_BIN` or `$SYSTEM_SBIN`, and
  looks for `notarytool` and `stapler` with `xcrun --find`.
- **The package.** `build-pkg.sh` sets an `identifier`, and every `identifier=` in
  `sign-macos` equals it. The package script's staging lines (`mkdir -p`, `cp`,
  `chmod`) equal `build-pkg.sh`'s, and its `pkgbuild` call passes `--sign` and
  `--keychain` once each and otherwise the arguments `build-pkg.sh` passes.
- **The new jobs.** `sign-macos`, `verify-macos` and `assemble-signed` each need
  `plan` directly, carry
  `if: github.ref == 'refs/heads/main' && needs.plan.outputs.version != ''`, and set
  `timeout-minutes` to at most 60, 20 and 10. `verify-macos` runs on `macos-latest`
  and `assemble-signed` on `ubuntu-latest`, and both hold exactly `contents: read` and
  declare no environment; their checkouts set `persist-credentials: false`, and no
  step of either carries an `if`. `attest` and `release` carry exactly the same
  `if` as the new jobs.
- **The chain.** Each job needs these directly: `sign-macos` needs `plan` and
  `build`; `verify-macos` adds `sign-macos`; `assemble-signed` adds `verify-macos`;
  `attest` needs `plan` and `assemble-signed`; `release` needs `plan` and `attest`.
  `attest` holds exactly `id-token: write` and `attestations: write`. `attest` and
  `release` each download exactly one artifact, `marshal-release-signed`, and no step
  of `release` reads `needs.build`.
- **No way past a failure.** No job or step of `release.yaml` sets
  `continue-on-error`. No job-level `if` uses `always()`, `failure()` or
  `cancelled()`, and neither does a step of any job but `sign-macos`, whose step
  conditions are the rule above. A step of `verify-macos` or `assemble-signed` carries
  no `if` at all. A step condition of `attest` or `release` that skips a step, such as
  `if: false`, is not seen.
- **The dry run's tool check.** In `release-build.yaml`'s `build` job, the "Check The
  Signing Tools" step runs on the macOS leg only, fails with `exit 1` when a tool is
  missing, and has no `env:` key that contains `SECRET` or `MACOS_`. Its `bin_tools`
  and `sbin_tools` list every tool that `sign-macos`'s and `verify-macos`'s scripts
  and `test-signed-pkg.sh` call, and it looks for `notarytool` and `stapler` with
  `xcrun --find`.
- **The dry run's `assemble`.** Its checkouts set `persist-credentials: false`, a
  script calls `assemble-release.sh`, and none writes `SHA256SUMS` itself, which means a
  `>` or `>>` redirection to it, `tee`, or `cp` or `mv` with it as the destination;
  reading it, as `cat` or `sha256sum -c` does, is not a write.

The stand-ins record calls and return canned answers, so these tests show the flow
and the refusals, not that Apple accepts a signature. The signed package is checked
only in the release run, by `verify-macos`, and on the Mac, and neither has run.
[Development Workflow](./development-workflow.md#cutting-a-release) has the
maintainer check the first signed release by hand.

## Installing, Updating and Rolling Back

`marshal` is installed from a release, never built on the host. You open the
release's installer, the MSI on Windows or the pkg on macOS, and it places the
binary, puts it on `PATH`, and handles updates and removal, so nothing is copied or
added to `PATH` by hand. The pkg is signed with Developer ID and notarized, and the
MSI is not code-signed ([Releases](#releases)). So Gatekeeper is expected to accept
the pkg with no block to pass, while Windows warns about the MSI, and the steps below
say which prompts to expect on Windows and how to pass them.

Checking the downloaded installer before you open it is optional but recommended.
The [check block](#checking-the-download) tests its checksum against
`SHA256SUMS`, that the tag's commit is on the default branch, its attestation
against `release.yaml` run from main, and the version of the binary inside it. It
needs `gh` 2.102 or later, signed in (`gh auth status`). Without the check, you open
a file that nothing has checked, and the file runs before anything else can catch a
replaced one: an MSI runs as you, and a pkg runs as root once you enter the
administrator password.
[Security](../conventions/security.md#a-release-is-published-only-from-a-dispatch-on-main)
records that risk.

The first release expected to carry installers is `0.2.0`, and the check blocks
below name it. `v0.1.0`, the only release so far, has the two bare binaries and no
installer, so until a release with installers exists, install `v0.1.0` with the
[bare-binary blocks](#rolling-back-to-v010). To check another release, change the
`version` assignment line of a block, and in a check block the `file` assignment
line too. If the first release with installers gets another version, a later change
corrects the line.

CI runs these blocks against stand-ins in `mise run check` and in the `Install Blocks`
job, and the `Install Blocks` job also runs them against the dry run's real installers
([What the Install Tests Cover](#what-the-install-tests-cover)). They have not run on
axpc or the Mac. The dry run builds its pkg without signing it, so the signed pkg is
checked only in the release run, by `verify-macos`, and on the Mac, and neither has
happened. Every prompt below is described as expected, from the platform's
documentation, not as observed. The maintainer's first install of a release
with installers on each host records what the prompts showed, and this document then
states it.

The pkg is notarized, which you can see before opening it. This is information, not a
step. `spctl --assess --type install -vv <pkg>` is expected to print `accepted` with
`source=Notarized Developer ID`, and `pkgutil --check-signature <pkg>` to show a
Developer ID Installer certificate chained to Apple's root;
[Reading a Signature](#reading-a-signature) says more.

axpc runs the binary this procedure installs, and
[Windows Runner Host](./windows-runner-host.md) is its operator procedure, including
[Rolling Back marshal](./windows-runner-host.md#rolling-back-marshal), which adds
the configuration check and the image rebuild around this procedure.

| OS      | Installed by            | Install path                                  |
| ------- | ----------------------- | --------------------------------------------- |
| Windows | the MSI                 | `%LOCALAPPDATA%\Programs\marshal\marshal.exe` |
| macOS   | the pkg                 | `/usr/local/bin/marshal`                      |
| macOS   | by hand, before the pkg | `~/.local/bin/marshal`                        |
| Windows | by hand, before the MSI | `%LOCALAPPDATA%\Programs\marshal\marshal.exe` |

### Installing

`<version>` is the release's version without the `v`, such as `0.2.0`.

1. Download `marshal-<version>-windows-x64.msi` or
   `marshal-<version>-macos-arm64.pkg` from the
   [release page](https://github.com/axross/runners/releases). If the browser warns
   that the file is uncommon or could be harmful, choose to keep it. For the pkg, a
   browser prompt is not expected, but this is not yet observed.
2. Optionally, and recommended, run the [check block](#checking-the-download) on
   the downloaded file before opening it.
3. Before opening the installer, do what applies:
   - When updating, stop the running instance first with `marshal stop`.
   - If this host has a manual install from `v0.1.0`, which is
     `~/.local/bin/marshal` on macOS or a `marshal.exe` copied by hand to
     `%LOCALAPPDATA%\Programs\marshal` on Windows, run the
     [migration block](#migrating-from-a-manual-install). It stops the instance
     itself.
4. Open the downloaded file.
   - On Windows, if SmartScreen shows "Windows protected your PC", choose "More
     info", then "Run anyway". If another security warning names the file, choose
     to run it. Smart App Control, when On, may block an unsigned installer; this is
     not verified.
   - On macOS, Installer shows its usual window and asks for an administrator
     password. Gatekeeper is expected to accept the notarized package with no block
     to pass; this is expected, not yet observed on a Mac.
5. Close every terminal window, open a new one, and run `marshal --version`; it
   prints `marshal <version>`. A new tab in a window that was already open can keep
   the old environment. On macOS, `command -v marshal` prints
   `/usr/local/bin/marshal`.

If `command -v marshal` prints another path on macOS, a manual install from before
the installers is still in place. Run the
[migration block](#migrating-from-a-manual-install), then uninstall and install
again. On Windows the MSI replaces a manual copy at the install path, so step 5
prints the new version even when you skipped the block;
[Migrating From a Manual Install](#migrating-from-a-manual-install) says what is
left behind.

The MSI installs for your user only. In CI, a GitHub-hosted Windows Server runner
ran this as a standard local user without administrator rights. The user signed in
through an interactive logon and was not elevated. The user ran the check block,
which unpacks the MSI with `msiexec /a`. The same user then installed the MSI with
`msiexec /i` and no UI, ran the installed `marshal`, and ran the uninstall block.
Each step passed. For that run the test set the Windows Installer policy
`DisableMSI` to 0. A run without that change exited 1625, and its log reported
`DisableMsi` as 1. The runner has no Installer policy key, so the value is expected
to be the Windows Server default. Microsoft documents 0 as the behavior with no
policy for Windows XP, Vista and 7. It states no default for Windows 10 or 11.
Whether a Windows 11 PC with no Installer policy allows the install is not yet
measured. The follow-up after the first release measures it on axpc. What Windows
shows when you open the file was not measured.

If `msiexec` exits 1625, its verbose log says "This installation is forbidden by
system policy. Contact your system administrator." A system policy on that PC, such
as the Windows Installer policy `DisableMSI`, blocks the install for your user.
Changing that policy is for whoever manages the PC.

Of the install steps, only the pkg needs the administrator password. It is expected
to create `/usr/local/bin` when it is absent; that case was not measured. On a
GitHub-hosted runner where `/usr/local/bin` already existed, it was owned by
`runner:admin` with mode 775 before the install, and the install and the uninstall
left that owner and mode unchanged. The package lists the directory as `root:wheel`
with mode 0755, so the listing was not applied to a directory that already existed.

### Checking the Download

The block opens with two assignment lines: `version`, and `file`, the path of the
downloaded installer, for example in the browser's download folder. Edit those two
lines, then paste the block whole into one shell. It does not check the macOS
package's signature or its team; what Gatekeeper does and does not check is under
[Reading a Signature](#reading-a-signature). It downloads `SHA256SUMS` with `gh`, then
refuses unless:

1. the installer's SHA-256 matches the line of `SHA256SUMS` for the file name the
   version gives, whatever the downloaded file is called;
2. the tag's commit, with an annotated tag dereferenced, is on the default branch,
   which the compare API reports as `identical` to or `behind` it;
3. `gh attestation verify` passes, pinned to `release.yaml`, the branch
   `refs/heads/main`, and that commit, with self-hosted runners denied; and
4. the binary inside the installer prints `marshal <version>`. The attestation ties
   the installer to a commit on main, not to a version, so this step keeps an older
   genuine installer under a newer version's name from passing.

A check that fails prints its cause after `refused:` and ends the block there
without closing the shell. The block installs nothing and changes neither the
downloaded file nor its download mark. It reads the binary from a temporary copy
with the mark removed: on Windows `msiexec /a` unpacks the copy into a temporary
folder, and on macOS `pkgutil --expand-full` does. The block has no comment lines,
because an interactive zsh runs a line that starts with `#` as a command unless
`interactive_comments` is set.

On a Windows host with Smart App Control On, the block's last step is expected to be
blocked, because it runs the unsigned `marshal.exe` extracted from the installer; this
is not measured. The block then refuses with "the binary in the installer does not
run:" and the error. The macOS block's matching refusal is "the binary in the package
does not run". A refusal at this step is not evidence of a bad file, and you may skip
the optional check.

On macOS, the system is expected to ask first whether Terminal may access the
Downloads folder, which the default `file` path is in. Allow it, or the block refuses
because the file "could not be read". This is expected, not measured.

On macOS, in bash or zsh:

<!-- install-block: macos-check -->

```bash
(
  version=0.2.0
  file="$HOME/Downloads/marshal-0.2.0-macos-arm64.pkg"
  set -euo pipefail
  refuse() { echo "refused: $*" >&2; exit 1; }
  name="marshal-${version}-macos-arm64.pkg"
  work="$(mktemp -d)" || refuse "no temporary directory"
  trap 'rm -rf "$work"' EXIT

  gh release download "v${version}" --repo axross/runners --pattern SHA256SUMS --dir "$work" \
    || refuse "SHA256SUMS could not be downloaded"

  listed="$(awk -v name="$name" '$2 == name { count++; hash = $1 } END { if (count == 1) print hash }' "${work}/SHA256SUMS")"
  [ -n "$listed" ] || refuse "SHA256SUMS has no single line for ${name}"
  actual="$(shasum -a 256 "$file" | awk '{ print $1 }')" || refuse "${file} could not be read"
  [ "$actual" = "$listed" ] || refuse "${file} does not match the ${name} line of SHA256SUMS"

  ref="$(gh api "repos/axross/runners/git/ref/tags/v${version}" --jq '.object.type + " " + .object.sha')" \
    || refuse "the tag v${version} could not be read"
  read -r type sha <<<"$ref"
  if [ "$type" = tag ]; then
    ref="$(gh api "repos/axross/runners/git/tags/${sha}" --jq '.object.type + " " + .object.sha')" \
      || refuse "the annotated tag v${version} could not be read"
    read -r type sha <<<"$ref"
  fi
  [ "$type" = commit ] || refuse "v${version} does not resolve to a commit"
  commit="$sha"

  comparison="$(gh api "repos/axross/runners/compare/main...${commit}" --jq .status)" \
    || refuse "${commit} could not be compared with main"
  case "$comparison" in
    identical | behind) echo "${commit} is on main." ;;
    *) refuse "${commit} is not on main (${comparison})" ;;
  esac

  gh attestation verify "$file" --repo axross/runners \
    --signer-workflow axross/runners/.github/workflows/release.yaml \
    --source-ref refs/heads/main --source-digest "$commit" \
    --deny-self-hosted-runners \
    || refuse "the attestation does not verify"

  copy="${work}/${name}"
  cp "$file" "$copy" || refuse "${file} could not be copied"
  if xattr -p com.apple.quarantine "$copy" >/dev/null 2>&1; then
    xattr -d com.apple.quarantine "$copy" || refuse "the quarantine attribute could not be removed from the copy"
  fi
  pkgutil --expand-full "$copy" "${work}/unpacked" || refuse "the package could not be unpacked"
  binary="${work}/unpacked/Payload/usr/local/bin/marshal"
  [ -f "$binary" ] || refuse "the package holds no usr/local/bin/marshal"
  chmod 0755 "$binary" || refuse "the unpacked binary could not be made executable"
  if xattr -p com.apple.quarantine "$binary" >/dev/null 2>&1; then
    xattr -d com.apple.quarantine "$binary" || refuse "the quarantine attribute could not be removed from the unpacked binary"
  fi
  printed="$("$binary" --version)" || refuse "the binary in the package does not run"
  [ "$printed" = "marshal ${version}" ] || refuse "the binary in the package is not marshal ${version}: ${printed}"

  echo "Passed: ${file} is the v${version} package. Nothing was installed."
)
```

On Windows, in PowerShell 5.1 or 7:

<!-- install-block: windows-check -->

```powershell
& {
  $version = '0.2.0'
  $file = "$env:USERPROFILE\Downloads\marshal-0.2.0-windows-x64.msi"
  $ErrorActionPreference = 'Stop'
  function Refuse([string]$Why) { throw "refused: $Why" }
  $name = "marshal-$version-windows-x64.msi"
  try {
    $work = New-Item -ItemType Directory -Path (Join-Path ([IO.Path]::GetTempPath()) ([IO.Path]::GetRandomFileName()))
  } catch { Refuse "no temporary directory: $_" }
  try {
    gh release download "v$version" --repo axross/runners --pattern SHA256SUMS --dir $work
    if ($LASTEXITCODE -ne 0) { Refuse 'SHA256SUMS could not be downloaded' }

    try {
      $listed = @(Get-Content -LiteralPath (Join-Path $work 'SHA256SUMS') | Where-Object { ($_ -split '\s+')[1] -eq $name })
      $actual = (Get-FileHash -Algorithm SHA256 -LiteralPath $file).Hash.ToLowerInvariant()
    } catch { Refuse "$file or SHA256SUMS could not be read: $_" }
    if ($listed.Count -ne 1 -or ($listed[0] -split '\s+')[0] -ne $actual) { Refuse "$file does not match the $name line of SHA256SUMS" }

    $ref = gh api "repos/axross/runners/git/ref/tags/v$version"
    if ($LASTEXITCODE -ne 0) { Refuse "the tag v$version could not be read" }
    $object = ($ref | ConvertFrom-Json).object
    if ($object.type -eq 'tag') {
      $ref = gh api "repos/axross/runners/git/tags/$($object.sha)"
      if ($LASTEXITCODE -ne 0) { Refuse "the annotated tag v$version could not be read" }
      $object = ($ref | ConvertFrom-Json).object
    }
    if ($object.type -ne 'commit') { Refuse "v$version does not resolve to a commit" }
    $commit = $object.sha

    $comparison = gh api "repos/axross/runners/compare/main...$commit" --jq .status
    if ($LASTEXITCODE -ne 0) { Refuse "$commit could not be compared with main" }
    if ($comparison -notin 'identical', 'behind') { Refuse "$commit is not on main ($comparison)" }
    Write-Host "$commit is on main."

    gh attestation verify $file --repo axross/runners `
      --signer-workflow axross/runners/.github/workflows/release.yaml `
      --source-ref refs/heads/main --source-digest $commit `
      --deny-self-hosted-runners
    if ($LASTEXITCODE -ne 0) { Refuse 'the attestation does not verify' }

    $copy = Join-Path $work $name
    try {
      Copy-Item -LiteralPath $file -Destination $copy
      Unblock-File -LiteralPath $copy
    } catch { Refuse "the temporary copy could not be made: $_" }
    $unpacked = Join-Path $work 'unpacked'
    $unpack = Start-Process -FilePath msiexec.exe -ArgumentList @('/a', "`"$copy`"", '/qn', "TARGETDIR=`"$unpacked`"") -Wait -PassThru
    $null = $unpack.Handle
    if ($unpack.ExitCode -ne 0) { Refuse "msiexec could not unpack the installer (exit code $($unpack.ExitCode))" }
    $binaries = @(Get-ChildItem -LiteralPath $unpacked -Recurse -File -ErrorAction SilentlyContinue | Where-Object { $_.Name -eq 'marshal.exe' })
    if ($binaries.Count -ne 1) { Refuse "the installer holds $($binaries.Count) files named marshal.exe, not 1" }
    $binary = $binaries[0].FullName
    try { $printed = & $binary --version } catch { Refuse "the binary in the installer does not run: $_" }
    if ($LASTEXITCODE -ne 0 -or "$printed" -ne "marshal $version") { Refuse "the binary in the installer is not marshal ${version}: $printed" }

    Write-Host "Passed: $file is the v$version installer. Nothing was installed."
  } finally {
    Remove-Item -LiteralPath $work -Recurse -Force -ErrorAction SilentlyContinue
  }
}
```

### Migrating From a Manual Install

Before the first installer install, and again after any rollback to `v0.1.0`, which
restores the manual install, run the block for your OS. It stops the instance and
refuses if the instance does not stop, then removes the manual install:

- On Windows, the copied `marshal.exe`, its directory when that is left empty, and the
  user `PATH` entry for the directory. The block prints what it removed, or that there
  is nothing to migrate. It looks up the MSI's product by the MSI's fixed upgrade code
  and refuses if it cannot list the products. The manual copy and the MSI share one
  path, so when the product is installed the block removes the installed file too,
  says so, and leaves the product registered: uninstall it and install it again.
- On macOS, `~/.local/bin/marshal` when it is present. The block leaves `~/.local/bin`
  and any `PATH` entry for it alone.

Updating from `v0.1.0` is this block, then an install. If you skip it, the recovery
is this block, then an uninstall and an install again.

Installing the MSI over a manual copy at the same path, without the block, was run
in CI on a GitHub-hosted runner. `msiexec` exited 0 and listed one product. The
installer's binary replaced the manual copy, and `marshal --version` at the install
path printed the installer's version. The user `PATH` then listed the install
directory twice, once from the manual install and once from the MSI. Step 5 of the
install shows the new version. To get back to one `PATH` entry, run the block, then
uninstall and install again. The uninstall block after that install passed and
removed the file at the install path.

Installing the pkg beside a manual copy at `~/.local/bin/marshal`, without the block,
when `~/.local/bin` comes before `/usr/local/bin` on `PATH`, was run in CI on a
GitHub-hosted runner, with the same result under zsh and `/bin/bash`. The pkg
installed its own binary at `/usr/local/bin/marshal` and left the manual copy in
place. The shell kept finding the manual copy: `command -v marshal` printed the manual
copy's path, and `marshal --version` printed the manual copy's version. So step 5
shows the old version and the manual copy's path. The recovery is the migration block,
which removes the manual copy, then an uninstall and an install again. The uninstall
block after that install exited 0 and removed `/usr/local/bin/marshal`, and the manual
copy stayed.

On macOS, in bash or zsh:

<!-- install-block: macos-migrate -->

```bash
(
  set -euo pipefail
  refuse() { echo "refused: $*" >&2; exit 1; }
  manual="$HOME/.local/bin/marshal"
  if [ ! -e "$manual" ] && [ ! -L "$manual" ]; then
    echo "There is no manual install at ${manual}; nothing to migrate."
    exit 0
  fi

  if [ -x "$manual" ]; then
    stopped="$("$manual" stop 2>&1)" || [ "$stopped" = "No instance is running." ] \
      || refuse "the instance did not stop: ${stopped}"
  fi
  rm -f "$manual" || refuse "${manual} could not be removed"
  echo "Removed ${manual}."
)
```

On Windows, in PowerShell 5.1 or 7:

<!-- install-block: windows-migrate -->

```powershell
& {
  $ErrorActionPreference = 'Stop'
  function Refuse([string]$Why) { throw "refused: $Why" }
  $upgradeCode = '{512F67C6-97ED-4562-84F3-2584FA3BA29C}'
  $directory = Join-Path $env:LOCALAPPDATA 'Programs\marshal'
  $target = Join-Path $directory 'marshal.exe'
  $removed = New-Object System.Collections.Generic.List[string]

  try {
    $installer = New-Object -ComObject WindowsInstaller.Installer
    $products = @($installer.GetType().InvokeMember('RelatedProducts', 'GetProperty', $null, $installer, @($upgradeCode)))
  } catch { Refuse "the installed products could not be listed: $_" }

  if (Test-Path -LiteralPath $target) {
    $stopped = & {
      $ErrorActionPreference = 'Continue'
      (& $target stop 2>&1 | ForEach-Object { "$_" }) -join "`n"
    }
    if ($LASTEXITCODE -ne 0 -and $stopped.Trim() -ne 'No instance is running.') { Refuse "the instance did not stop: $stopped" }
    try { Remove-Item -LiteralPath $target -Force } catch { Refuse "$target could not be removed: $_" }
    $removed.Add($target)
  }
  if ((Test-Path -LiteralPath $directory) -and -not (Get-ChildItem -LiteralPath $directory -Force)) {
    Remove-Item -LiteralPath $directory -Force
    $removed.Add("the empty directory $directory")
  }

  try {
    $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment', $true)
    try {
      $entries = @(([string]$key.GetValue('Path', '', [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)) -split ';' | Where-Object { $_.Trim() -ne '' })
      $kept = @($entries | Where-Object { [Environment]::ExpandEnvironmentVariables($_.Trim()).TrimEnd('\') -ine $directory.TrimEnd('\') })
      if ($kept.Count -ne $entries.Count) {
        $key.SetValue('Path', ($kept -join ';'), [Microsoft.Win32.RegistryValueKind]::ExpandString)
        $removed.Add("the user PATH entry for $directory")
      }
    } finally { $key.Close() }
  } catch { Refuse "the user PATH could not be updated: $_" }
  if ($removed.Count -eq 0) {
    Write-Host "There is no manual install at $directory; nothing to migrate."
  } else {
    Write-Host "Removed $($removed -join ', ')."
  }
  if ($products.Count -gt 0 -and $removed.Count -gt 0) {
    Write-Host 'The marshal installer is installed, and what was removed was part of it too; uninstall it and install it again.'
  }
}
```

### Updating, Repairing, Uninstalling and Rolling Back

- **Update.** Install a newer version, steps 1 to 5 above. Stop the instance first,
  then rebuild the image and start again, as
  [Updating the Runner](./windows-runner-host.md#updating-the-runner) describes.
- **Upgrading while a program runs from the install path** on Windows. In CI, on a
  GitHub-hosted runner, an upgrade with no UI (`/qn`) ran while a stand-in console
  program ran from the install path as `marshal.exe`. `msiexec` exited 0. The
  upgrade's log shows Restart Manager shutting down the applications that held files
  in use. The file at the install path was then the new version, the program was no
  longer running, one product was listed at the new version, and no rename was left
  pending for a reboot. So the upgrade ends a `marshal` that runs from the install
  path and replaces the file with no reboot. Stop the instance first, as the update
  bullet says; if you did not, start it again after the upgrade. What the interactive
  installer window shows was not measured.
- **A lower pkg over a higher one** on macOS. In CI, on a GitHub-hosted runner, the
  installer exited 0, the lower version's file replaced the higher version's, and the
  receipt reported the lower version. So macOS Installer accepts the downgrade, and
  installing an older pkg is a working rollback there.
- **Repair.** Uninstall, then install.
- **Uninstall.** The block for your OS stops the instance and refuses if it does not
  stop. The logs stay, in `%LOCALAPPDATA%\marshal\logs\` on Windows and
  `~/Library/Logs/marshal/` on macOS. On Windows the block finds the installed
  product by the MSI's fixed upgrade code and removes it with `msiexec /x`, and
  Settings, Apps removes it too, after you stop the instance. macOS has no pkg
  uninstall, so when `pkgutil --pkg-info` finds the pkg's receipt the block removes
  `/usr/local/bin/marshal` and forgets the receipt with `pkgutil --forget`, which
  removes no file. When it finds no receipt the block says the package is not
  installed, runs no `sudo`, and leaves any file at `/usr/local/bin/marshal` in place,
  because the package did not install it. If the block reports such a file and you do
  not want it, remove it with `sudo rm /usr/local/bin/marshal`.
- **Roll back.** Uninstall, then install the older version, steps 1 to 5. The MSI
  refuses a lower version while a newer one is installed, with the message "A newer
  version of marshal is already installed.", and the installed version stays, so
  uninstall first. The pkg does not refuse, as the lower pkg bullet above says, so on
  macOS installing the older pkg over the newer one works too. Uninstalling first
  stays the documented route on both.
  The Developer ID certificates are expected to last one year, and whether a
  notarized pkg still installs once its Installer certificate has expired is not
  confirmed, so a rollback through an older release's pkg may fail. The bare-binary
  route to `v0.1.0` and a newer release stay available.
- **Roll back to `v0.1.0`.** Uninstall, then use the [bare-binary
  blocks](#rolling-back-to-v010) and add the install directory to the user's `PATH`
  by hand, as the manual install always needed. On Windows, add
  `%LOCALAPPDATA%\Programs\marshal` to the user `Path` under **Settings, System,
  About, Advanced system settings, Environment Variables**. On macOS, `~/.local/bin`
  is not on the default `PATH`, so add it in the shell's startup file. Updating from
  `v0.1.0` again starts with the migration block.

Immutable releases keep a published release's tag from moving, so a version you
install again is the file you installed before.

On macOS, in bash or zsh:

<!-- install-block: macos-uninstall -->

```bash
(
  set -euo pipefail
  refuse() { echo "refused: $*" >&2; exit 1; }
  installed=/usr/local/bin/marshal
  identifier=app.axross.marshal

  if ! pkgutil --pkg-info "$identifier" >/dev/null 2>&1; then
    echo "The marshal package is not installed; nothing to uninstall."
    if [ -e "$installed" ] || [ -L "$installed" ]; then
      echo "${installed} exists but the package did not install it, so it is left in place."
    fi
    exit 0
  fi

  if [ -x "$installed" ]; then
    stopped="$("$installed" stop 2>&1)" || [ "$stopped" = "No instance is running." ] \
      || refuse "the instance did not stop: ${stopped}"
  fi
  sudo rm -f "$installed" || refuse "${installed} could not be removed"
  sudo pkgutil --forget "$identifier" || refuse "the receipt for ${identifier} could not be forgotten"
  echo "Uninstalled marshal. The logs in ~/Library/Logs/marshal stay."
)
```

On Windows, in PowerShell 5.1 or 7:

<!-- install-block: windows-uninstall -->

```powershell
& {
  $ErrorActionPreference = 'Stop'
  function Refuse([string]$Why) { throw "refused: $Why" }
  $upgradeCode = '{512F67C6-97ED-4562-84F3-2584FA3BA29C}'
  $target = Join-Path $env:LOCALAPPDATA 'Programs\marshal\marshal.exe'

  try {
    $installer = New-Object -ComObject WindowsInstaller.Installer
    $products = @($installer.GetType().InvokeMember('RelatedProducts', 'GetProperty', $null, $installer, @($upgradeCode)))
  } catch { Refuse "the installed products could not be listed: $_" }
  if ($products.Count -eq 0) {
    Write-Host 'The marshal installer is not installed; nothing to uninstall.'
    return
  }

  if (Test-Path -LiteralPath $target) {
    $stopped = & {
      $ErrorActionPreference = 'Continue'
      (& $target stop 2>&1 | ForEach-Object { "$_" }) -join "`n"
    }
    if ($LASTEXITCODE -ne 0 -and $stopped.Trim() -ne 'No instance is running.') { Refuse "the instance did not stop: $stopped" }
  }
  foreach ($product in $products) {
    $removal = Start-Process -FilePath msiexec.exe -ArgumentList @('/x', $product, '/qn', '/norestart') -Wait -PassThru
    $null = $removal.Handle
    if ($removal.ExitCode -ne 0) { Refuse "msiexec could not remove $product (exit code $($removal.ExitCode))" }
  }
  Write-Host 'Uninstalled marshal. The logs in %LOCALAPPDATA%\marshal\logs stay.'
}
```

### Rolling Back to v0.1.0

`v0.1.0` has no installer, so it is installed from a bare binary. If an installer is
installed, uninstall it first with the uninstall block. On macOS, a remaining
`/usr/local/bin/marshal` comes first on `PATH` unless you add `~/.local/bin` ahead
of `/usr/local/bin`, so remove it first. The blocks do not touch `PATH`: add
`%LOCALAPPDATA%\Programs\marshal` to the user `Path` on Windows, or `~/.local/bin`
in the shell's startup file on macOS, as the
[Roll back to `v0.1.0`](#updating-repairing-uninstalling-and-rolling-back) bullet
describes. The procedure accepts a binary only when its checksum matches
`SHA256SUMS`, the tag's commit is on the default branch, and its attestation
verifies against `release.yaml` run from main, at that commit. It needs `gh` 2.102
or later, signed in. The blocks below pin `version=0.1.0` and are the only ones that
copy a binary into place.

Paste a block below whole into one shell. A check that fails prints its cause after
`refused:` and ends the block there without closing the shell, so nothing after it
runs, and a refusal before the rename in step 7 leaves the install path as it was.
The block's variables and working directory do not outlive it, so each run starts
clean.

1. Download the binary and `SHA256SUMS`.
2. Refuse unless the binary's SHA-256 matches its line in `SHA256SUMS`.
3. Resolve the commit the tag points to, dereferencing an annotated tag.
4. Refuse unless the compare API reports that commit `identical` to or `behind`
   the default branch, which means it is on it.
5. Refuse unless `gh attestation verify` passes, pinned to `release.yaml`, the
   branch `refs/heads/main`, and the commit from step 3, with self-hosted runners
   denied. An attestation made by any other workflow, branch, or commit fails here.
6. Refuse unless the downloaded binary's `--version` prints `marshal <version>`.
   The attestation ties the binary to a commit on main, not to a version, so this
   step keeps an older genuine binary under a newer version's name from replacing
   the installed one. On macOS, make the file executable and remove its quarantine
   attribute, if present, before running it.
7. Stop the instance if one is running, refusing if it does not stop, then replace
   the binary in the install directory.
8. Refuse unless the installed binary's `--version` prints `marshal <version>`.

On macOS, in bash or zsh. The block has no comment lines, because an interactive zsh
runs a line that starts with `#` as a command unless `interactive_comments` is set:

<!-- install-block: macos-bare -->

```bash
(
  set -euo pipefail
  refuse() { echo "refused: $*" >&2; exit 1; }
  version=0.1.0
  asset="marshal-${version}-macos-arm64"
  work="$(mktemp -d)" || refuse "no temporary directory"
  cd "$work"

  gh release download "v${version}" --repo axross/runners --pattern "$asset" --pattern SHA256SUMS \
    || refuse "the release files could not be downloaded"

  awk -v file="$asset" '$2 == file' SHA256SUMS >"${asset}.sha256"
  { [ -s "${asset}.sha256" ] && shasum -a 256 -c "${asset}.sha256"; } \
    || refuse "${asset} does not match SHA256SUMS"

  ref="$(gh api "repos/axross/runners/git/ref/tags/v${version}" --jq '.object.type + " " + .object.sha')" \
    || refuse "the tag v${version} could not be read"
  read -r type sha <<<"$ref"
  if [ "$type" = tag ]; then
    ref="$(gh api "repos/axross/runners/git/tags/${sha}" --jq '.object.type + " " + .object.sha')" \
      || refuse "the annotated tag v${version} could not be read"
    read -r type sha <<<"$ref"
  fi
  [ "$type" = commit ] || refuse "v${version} does not resolve to a commit"
  commit="$sha"

  comparison="$(gh api "repos/axross/runners/compare/main...${commit}" --jq .status)" \
    || refuse "${commit} could not be compared with main"
  case "$comparison" in
    identical | behind) echo "${commit} is on main." ;;
    *) refuse "${commit} is not on main (${comparison})" ;;
  esac

  gh attestation verify "$asset" --repo axross/runners \
    --signer-workflow axross/runners/.github/workflows/release.yaml \
    --source-ref refs/heads/main --source-digest "$commit" \
    --deny-self-hosted-runners \
    || refuse "the attestation does not verify"

  chmod 0755 "$asset" || refuse "${asset} could not be made executable"
  if xattr -p com.apple.quarantine "$asset" >/dev/null 2>&1; then
    xattr -d com.apple.quarantine "$asset" || refuse "the quarantine attribute could not be removed from ${asset}"
  fi
  downloaded="$(./"$asset" --version)" || refuse "the downloaded binary does not run"
  [ "$downloaded" = "marshal ${version}" ] || refuse "the downloaded binary is not marshal ${version}: ${downloaded}"

  if [ -x ~/.local/bin/marshal ]; then
    stopped="$(~/.local/bin/marshal stop 2>&1)" || [ "$stopped" = "No instance is running." ] \
      || refuse "the instance did not stop: ${stopped}"
  fi
  { mkdir -p ~/.local/bin && cp "$asset" ~/.local/bin/marshal.new \
    && chmod 0755 ~/.local/bin/marshal.new && mv -f ~/.local/bin/marshal.new ~/.local/bin/marshal; } \
    || refuse "the binary could not be installed in ~/.local/bin"

  installed="$(~/.local/bin/marshal --version)" || refuse "the installed binary does not run"
  [ "$installed" = "marshal ${version}" ] || refuse "the installed binary is not marshal ${version}: ${installed}"
  echo "Installed marshal ${version} at ~/.local/bin/marshal."
)
```

On Windows, in PowerShell:

<!-- install-block: windows-bare -->

```powershell
& {
  $ErrorActionPreference = 'Stop'
  function Refuse([string]$Why) { throw "refused: $Why" }
  $version = '0.1.0'
  $asset = "marshal-$version-windows-x64.exe"
  try {
    $work = New-Item -ItemType Directory -Path (Join-Path ([IO.Path]::GetTempPath()) ([IO.Path]::GetRandomFileName()))
  } catch { Refuse "no temporary directory: $_" }
  $file = Join-Path $work $asset
  $sums = Join-Path $work 'SHA256SUMS'

  gh release download "v$version" --repo axross/runners --pattern $asset --pattern SHA256SUMS --dir $work
  if ($LASTEXITCODE -ne 0) { Refuse 'the release files could not be downloaded' }

  try {
    $listed = @(Get-Content -LiteralPath $sums | Where-Object { ($_ -split '\s+')[1] -eq $asset })
    $actual = (Get-FileHash -Algorithm SHA256 -LiteralPath $file).Hash.ToLowerInvariant()
  } catch { Refuse "$asset or SHA256SUMS could not be read: $_" }
  if ($listed.Count -ne 1 -or ($listed[0] -split '\s+')[0] -ne $actual) { Refuse "$asset does not match SHA256SUMS" }

  $ref = gh api "repos/axross/runners/git/ref/tags/v$version"
  if ($LASTEXITCODE -ne 0) { Refuse "the tag v$version could not be read" }
  $object = ($ref | ConvertFrom-Json).object
  if ($object.type -eq 'tag') {
    $ref = gh api "repos/axross/runners/git/tags/$($object.sha)"
    if ($LASTEXITCODE -ne 0) { Refuse "the annotated tag v$version could not be read" }
    $object = ($ref | ConvertFrom-Json).object
  }
  if ($object.type -ne 'commit') { Refuse "v$version does not resolve to a commit" }
  $commit = $object.sha

  $comparison = gh api "repos/axross/runners/compare/main...$commit" --jq .status
  if ($LASTEXITCODE -ne 0) { Refuse "$commit could not be compared with main" }
  if ($comparison -notin 'identical', 'behind') { Refuse "$commit is not on main ($comparison)" }
  Write-Host "$commit is on main."

  gh attestation verify $file --repo axross/runners `
    --signer-workflow axross/runners/.github/workflows/release.yaml `
    --source-ref refs/heads/main --source-digest $commit `
    --deny-self-hosted-runners
  if ($LASTEXITCODE -ne 0) { Refuse 'the attestation does not verify' }

  try { $downloaded = & $file --version } catch { Refuse "the downloaded binary does not run: $_" }
  if ($LASTEXITCODE -ne 0 -or "$downloaded" -ne "marshal $version") { Refuse "the downloaded binary is not marshal ${version}: $downloaded" }

  $directory = Join-Path $env:LOCALAPPDATA 'Programs\marshal'
  $target = Join-Path $directory 'marshal.exe'
  if (Test-Path -LiteralPath $target) {
    $stopped = & {
      $ErrorActionPreference = 'Continue'
      (& $target stop 2>&1 | ForEach-Object { "$_" }) -join "`n"
    }
    if ($LASTEXITCODE -ne 0 -and $stopped.Trim() -ne 'No instance is running.') { Refuse "the instance did not stop: $stopped" }
  }
  try {
    New-Item -ItemType Directory -Force -Path $directory | Out-Null
    Copy-Item -LiteralPath $file -Destination "$target.new" -Force
    Move-Item -LiteralPath "$target.new" -Destination $target -Force
  } catch { Refuse "the binary could not be installed in ${directory}: $_" }

  try { $installed = & $target --version } catch { Refuse "the installed binary does not run: $_" }
  if ($LASTEXITCODE -ne 0 -or "$installed" -ne "marshal $version") { Refuse "the installed binary is not marshal ${version}: $installed" }
  Write-Host "Installed marshal $version at $target."
}
```

The stop runs with `$ErrorActionPreference` at `Continue` because Windows PowerShell 5.1
turns a native command's redirected standard error into an error, and `Stop` would end
the block at the "No instance is running." line.

Windows cannot replace an executable that is running, so step 7 also fails while an
instance still runs from the install path. The procedure's refusals on Windows are
not yet verified on axpc ([#56](https://github.com/axross/runners/issues/56)): the
refusal checks were not run there. A binary fetched with
`gh release download` is assumed to carry no Mark of the Web, and the procedure
does not clear one. On axpc the installed copy had no `Zone.Identifier`
([#56](https://github.com/axross/runners/issues/56#issuecomment-6511048211)), and
SmartScreen did not prompt, with "Check apps and files" On and Smart App Control
Off ([#56](https://github.com/axross/runners/issues/56#issuecomment-6511120441)).
So with Smart App Control Off, the unsigned binary this procedure installs, which
carries no Mark of the Web, is not blocked on axpc. A copy that carries one, such
as a browser download, or a host with Smart App Control On, could be.

### What the Install Tests Cover

Each block above has a marker comment, `<!-- install-block: <id> -->`, before its
fence, and the tests extract the block from this document, so the text you paste is
the text under test. A test replaces a block's `version` assignment line, and a check
block's `file` assignment line, and the macOS uninstall block's `installed`
assignment line, and changes nothing else.

`mise run test:install-blocks` runs the macOS blocks under bash with stand-ins first
on `PATH` for `gh`, `pkgutil`, `xattr`, `sudo`, `installer`, `cp`, and `marshal`, each
recording its calls. Every refusal is produced, and each case asserts what ran and
what did not, so a failed check is shown to install nothing. The suite includes the
bare-binary blocks' refusals, the uninstall and migration refusals when the
instance does not stop, and the uninstall refusals when `sudo rm` or
`pkgutil --forget` fails. The `gh` stand-ins, here and in the Windows suite, serve a
canned API response body and apply the block's `--jq` expression to it with `jq`, so
a wrong expression fails the suite. `mise.toml` pins `jq` for `mise run check`, and
the `Install Blocks` job runs the suites outside mise, so it uses its runner's own
`jq`; the suites stop with one line when `jq` is missing. The macOS suite finishes in
well under a minute on Linux. The `Install Blocks` job in
[`merge-checks.yaml`](../../.github/workflows/merge-checks.yaml) runs the same suite
under zsh on macOS, and the Windows suite
([`test-install-blocks.ps1`](../../.github/scripts/tests/test-install-blocks.ps1))
under Windows PowerShell 5.1 and PowerShell 7 on Windows.

The job needs the release dry run and downloads its `marshal-release` artifact, then
runs the documented blocks against the real installers with only `gh` stubbed, to
serve the artifact's real `SHA256SUMS`, a tag on main, and a passing attestation.
The dry run's pkg is built without signing, so the blocks never see a signed package,
and the check block works on it because it checks no signature; the signed package is
exercised only by `verify-macos` in a release run:

- the check block passes a genuine installer given as a renamed copy that carries a
  seeded download mark (`Zone.Identifier` on Windows, `com.apple.quarantine` on
  macOS), installs nothing, and leaves the file and its mark unchanged;
- the check block refuses a genuine installer served under another version's name,
  with a matching checksum line and a passing attestation, at the version step;
- the migration block removes a seeded manual install that differs from the
  installer's binary, after which the installer installs and `marshal --version` at
  the install path prints the build's version;
- installing over such a manual copy without migration is recorded as a `MEASURED:`
  line;
- the migration block, run while the MSI is installed, removes the installed file
  too, after which the uninstall block and a second install leave the installer's
  binary at the install path;
- the uninstall block removes the installation.

macOS runs under zsh and the system `/bin/bash`; Windows runs under Windows
PowerShell 5.1. On the Windows leg a standard local user without administrator
rights then runs the check block and installs the MSI through a secondary logon,
which is an interactive logon, and the outcome is a `MEASURED:` line. The test
sets the Windows Installer policy `DisableMSI` to 0 for that run, because the
GitHub-hosted Windows Server image blocks a non-elevated per-user install, and
restores it at the end.

## Dependencies

A new crate MUST NOT be added without an entry here, and every crate in
[`cli/Cargo.lock`](../../cli/Cargo.lock) has one. [`cli/deny.toml`](../../cli/deny.toml)
allows crates.io only, one version of each crate, and the licences below. The CLI
is unpublished, so it carries no licence field of its own; `deny.toml` ignores
private crates for that reason. A Dependabot bump that raises new clippy warnings
or adds a duplicate crate version arrives as a red pull request, and the
maintainer fixes it there. A Dependabot `cargo` update of a crate `marshal-cli` uses
must move `cli/Cargo.lock` and `app/Cargo.lock` to the same version in one change, or
`check:app-lock` fails (see [The App Workspace](#the-app-workspace)).

| Crate                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              | Why                                                                                                                                                                                                                                                                                                                                                                | Licence                                                                                     |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------- |
| `clap`, with `clap_builder`, `clap_lex`, and `anstyle`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             | Argument parsing, with default features off                                                                                                                                                                                                                                                                                                                        | MIT OR Apache-2.0                                                                           |
| `regex`, with `regex-automata` and `regex-syntax`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  | The whole-value patterns for names, labels, and image names                                                                                                                                                                                                                                                                                                        | MIT OR Apache-2.0                                                                           |
| `aho-corasick` and `memchr`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        | Optional dependencies of `regex`                                                                                                                                                                                                                                                                                                                                   | Unlicense OR MIT                                                                            |
| `serde`, with `serde_core`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         | Reading the parsed JSON tree and the shared expectations into derived types, with default features off                                                                                                                                                                                                                                                             | MIT OR Apache-2.0                                                                           |
| `serde_derive`, `syn` 3, `quote`, and `proc-macro2`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                | The `Deserialize` derive; these run at build time                                                                                                                                                                                                                                                                                                                  | MIT OR Apache-2.0                                                                           |
| `unicode-ident`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    | A dependency of `syn` and `proc-macro2`                                                                                                                                                                                                                                                                                                                            | (MIT OR Apache-2.0) AND Unicode-3.0                                                         |
| `tokio`, `tokio-macros`, `mio`, `bytes`, and `slab`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                | The async runtime for the slot loop and the container backend, on one thread, with only the `rt`, `time`, `sync`, `io-util`, `process`, `macros`, `net`, and `signal` features, and `test-util` for the tests only; `tokio-macros` supplies `select!` and `join!`, `net` serves the Unix control socket, and `signal` delivers the stop signals and console events | MIT                                                                                         |
| `socket2`, `signal-hook-registry`, `errno`, `smallvec`, `once_cell`, `cfg-if`, `log`, `itoa`, and `bitflags`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       | Operating-system and small-utility dependencies of `tokio`, `reqwest`, and their dependencies                                                                                                                                                                                                                                                                      | MIT OR Apache-2.0                                                                           |
| `libc`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             | The `O_NOFOLLOW` and `O_NONBLOCK` values for the Unix lock's open, declared directly on Unix with default features off; `tokio` and its dependencies use it too                                                                                                                                                                                                    | MIT OR Apache-2.0                                                                           |
| `pin-project-lite` and `atomic-waker`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              | Futures plumbing under `tokio` and `hyper`                                                                                                                                                                                                                                                                                                                         | Apache-2.0 OR MIT                                                                           |
| `futures-core`, `futures-task`, `futures-channel`, and `futures-util`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              | Future and channel traits `hyper` and `reqwest` use                                                                                                                                                                                                                                                                                                                | MIT OR Apache-2.0                                                                           |
| `reqwest`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          | The HTTPS requests to GitHub: the JIT registration and the agent's GitHub status reads, through one client builder, with default features off and only `rustls-no-provider`, so no JSON, cookie, proxy-crate, or compression feature is built                                                                                                                      | MIT OR Apache-2.0                                                                           |
| `hyper`, `hyper-util`, `http-body`, `http-body-util`, `tower`, `tower-http`, `tower-layer`, `tower-service`, `tracing`, `tracing-core`, `want`, and `try-lock`                                                                                                                                                                                                                                                                                                                                                                                                                                                     | The HTTP client stack under `reqwest`                                                                                                                                                                                                                                                                                                                              | MIT                                                                                         |
| `http`, `httparse`, `base64`, and `ipnet`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          | HTTP types, header parsing, header encoding, and proxy address matching under `reqwest`                                                                                                                                                                                                                                                                            | MIT OR Apache-2.0                                                                           |
| `sync_wrapper`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     | A dependency of `reqwest`                                                                                                                                                                                                                                                                                                                                          | Apache-2.0                                                                                  |
| `url`, `form_urlencoded`, `percent-encoding`, `idna`, and `idna_adapter`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           | Parsing the request URL, under `reqwest`                                                                                                                                                                                                                                                                                                                           | MIT OR Apache-2.0                                                                           |
| `utf8_iter`, `stable_deref_trait`, and `zeroize`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | Dependencies of `idna` and the TLS stack                                                                                                                                                                                                                                                                                                                           | Apache-2.0 OR MIT for `utf8_iter` and `zeroize`, MIT OR Apache-2.0 for `stable_deref_trait` |
| `icu_collections`, `icu_locale_core`, `icu_normalizer`, `icu_normalizer_data`, `icu_properties`, `icu_properties_data`, `icu_provider`, `litemap`, `potential_utf`, `tinystr`, `writeable`, `yoke`, `zerofrom`, `zerotrie`, and `zerovec`                                                                                                                                                                                                                                                                                                                                                                          | Unicode normalisation data for `idna`                                                                                                                                                                                                                                                                                                                              | Unicode-3.0                                                                                 |
| `displaydoc`, `yoke-derive`, `zerofrom-derive`, `zerovec-derive`, and `synstructure`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               | Derives and their helper for the Unicode crates above; these run at build time                                                                                                                                                                                                                                                                                     | MIT OR Apache-2.0 for `displaydoc`, MIT for `synstructure`, Unicode-3.0 for the rest        |
| `rustls`, `hyper-rustls`, and `rustls-native-certs`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                | TLS for `reqwest`, with only the `ring`, `std`, and `tls12` features                                                                                                                                                                                                                                                                                               | Apache-2.0 OR ISC OR MIT                                                                    |
| `tokio-rustls`, `rustls-pki-types`, and `rustls-platform-verifier`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 | TLS glue, certificate types, and the platform's certificate verifier                                                                                                                                                                                                                                                                                               | MIT OR Apache-2.0                                                                           |
| `rustls-webpki` and `untrusted`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    | Certificate validation for `rustls`                                                                                                                                                                                                                                                                                                                                | ISC                                                                                         |
| `ring`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             | The cryptography provider, installed explicitly                                                                                                                                                                                                                                                                                                                    | Apache-2.0 AND ISC                                                                          |
| `cc`, `shlex`, `find-msvc-tools`, and `getrandom`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  | Build-time C compiler discovery for `ring`, and its random-number source                                                                                                                                                                                                                                                                                           | MIT OR Apache-2.0                                                                           |
| `subtle`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           | Constant-time comparison for `rustls`                                                                                                                                                                                                                                                                                                                              | BSD-3-Clause                                                                                |
| `openssl-probe`, `security-framework`, `security-framework-sys`, `core-foundation`, `core-foundation-sys`, `windows-sys`, and `windows-link` 0.2                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | The platform certificate stores: the system store on Linux, the keychain on macOS, and the Windows APIs                                                                                                                                                                                                                                                            | MIT OR Apache-2.0                                                                           |
| `schannel`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         | The Windows certificate APIs; Cargo records it but the three CI targets do not build it                                                                                                                                                                                                                                                                            | MIT                                                                                         |
| `bumpalo`, `combine`, `jni`, `jni-macros`, `jni-sys`, `jni-sys-macros`, `js-sys`, `rustc_version`, `rustls-platform-verifier-android`, `rustversion`, `same-file`, `semver`, `simd_cesu8`, `simdutf8`, `thiserror` 2, `thiserror-impl` 2, `walkdir`, `wasm-bindgen`, `wasm-bindgen-futures`, `wasm-bindgen-macro`, `wasm-bindgen-macro-support`, `wasm-bindgen-shared`, `web-sys`, `winapi-util`, `windows-targets`, `windows_aarch64_gnullvm`, `windows_aarch64_msvc`, `windows_i686_gnu`, `windows_i686_gnullvm`, `windows_i686_msvc`, `windows_x86_64_gnu`, `windows_x86_64_gnullvm`, and `windows_x86_64_msvc` | Platform verifier and `getrandom` code for Android, WebAssembly, and other Windows targets, which the lockfile records and none of the three CI targets builds                                                                                                                                                                                                     | MIT OR Apache-2.0 (`same-file`, `walkdir`, and `winapi-util` are Unlicense OR MIT)          |
| `wasi`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             | A WebAssembly dependency of `getrandom`, not built on the CI targets                                                                                                                                                                                                                                                                                               | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT                                         |
| `webpki-root-certs`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                | Bundled roots the platform verifier uses on WebAssembly, not built on the CI targets                                                                                                                                                                                                                                                                               | CDLA-Permissive-2.0                                                                         |
| `interprocess`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     | The Windows named pipes behind the single-instance lock and the control channel, with default features off and, for the agent's asynchronous pipe, its `tokio` feature; it builds on Windows only                                                                                                                                                                  | 0BSD OR Apache-2.0                                                                          |
| `recvmsg`, `doctest-file`, and `widestring`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        | Dependencies of `interprocess`: a receive helper, a procedural macro that includes example files in documentation, and wide-string types for Windows paths                                                                                                                                                                                                         | 0BSD for `recvmsg` and `doctest-file`, MIT OR Apache-2.0 for `widestring`                   |
| `serde_json`, `zmij`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               | The agent's protocol messages and snapshot, in JSON, with only the `std` feature; `zmij` formats its floating-point numbers                                                                                                                                                                                                                                        | MIT OR Apache-2.0 for `serde_json`, MIT for `zmij`                                          |
| `keyring-core`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     | The interface of a credential store, which the token store adapts and a platform's store implements, with default features off                                                                                                                                                                                                                                     | MIT OR Apache-2.0                                                                           |
| `windows-native-keyring-store` and `byteorder`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     | The Windows Credential Manager behind the token store, with default features off, and the byte-order helper it uses for the UTF-16 token; they build on Windows only                                                                                                                                                                                               | MIT OR Apache-2.0 for the store, Unlicense OR MIT for `byteorder`                           |
| `winsafe`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          | The current user's security identifier, with only the `advapi` feature; it builds on Windows only                                                                                                                                                                                                                                                                  | MIT                                                                                         |
| `win32job`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         | The Job Object that ends each job container's `docker run` client with `start`; it builds on Windows only (see [Containment](#containment))                                                                                                                                                                                                                        | MIT OR Apache-2.0                                                                           |
| `windows`, `windows-core`, `windows-collections`, `windows-future`, `windows-numerics`, `windows-threading`, `windows-result`, `windows-strings`, and `windows-link` 0.1                                                                                                                                                                                                                                                                                                                                                                                                                                           | The Windows API bindings `win32job` uses; they build on Windows only                                                                                                                                                                                                                                                                                               | MIT OR Apache-2.0                                                                           |
| `windows-implement`, `windows-interface`, `thiserror` 1, `thiserror-impl` 1, and `syn` 2                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           | The derives of the `windows` crates and `win32job`, with the older `syn` they use; these run at build time, on Windows only                                                                                                                                                                                                                                        | MIT OR Apache-2.0                                                                           |
| `toml_edit`, with `toml_parser`, `toml_writer`, and `toml_datetime`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                | Reading the TOML host configuration into a document that keeps each key's position, and changing a document while keeping its comments, with default features off and only `parse` and `display`; `display` prints a document back. The reader also uses `toml_parser` directly, with only `std`, to place a key nested past `toml_edit`'s limit                   | MIT OR Apache-2.0                                                                           |
| `winnow`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           | The parser library under `toml_parser` and `toml_edit`                                                                                                                                                                                                                                                                                                             | MIT                                                                                         |
| `indexmap`, `hashbrown`, and `equivalent`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          | The order-keeping map under `toml_edit`'s tables, and its hash map and lookup trait                                                                                                                                                                                                                                                                                | Apache-2.0 OR MIT for `indexmap` and `equivalent`, MIT OR Apache-2.0 for `hashbrown`        |

`aho-corasick` and `memchr` are in the lockfile because `regex` lists them as
optional, but the CLI's feature set (`std` and `unicode-perl`, no `perf`) does not
build them. They are accepted under their MIT alternative, so the Unlicense is not
on the allowlist and need not be.
`serde_json`, `keyring-core`, `windows-native-keyring-store`, and the `tokio`
feature of `interprocess` were added with the agent (issue #73).
Their dependencies in the lockfile are `zmij` and `byteorder`, and `serde_json` also
brings `itoa` and `memchr`, which the table already lists. `byteorder` is accepted
under its MIT alternative, like `memchr`.

The validator's JSON parser is in the crate, not a dependency. The rules under
Validation Rules need order-preserving objects, the last-position duplicate rule,
case-conflict errors, and number literals kept as text. The crate owns that parser,
and the fixtures pin it. `serde_json` is for the agent's protocol only: the messages
of version 2 are serialised and parsed with it, with only its `std` feature, and the
validator never uses it.

The reader implements serde's `Deserializer`, so a type that derives `Deserialize`
can be read from the parsed tree. A failed read is an error, never a panic, and
reading a struct from a node that is not an object is an error. Number literals
reach the tree's own value type as text; any other target sees an `i64`, `u64`,
`i128`, or `f64`, so `1e400` reads as infinity there. Field names match ignoring
ASCII case only when the caller asks, and only for struct fields. The validator
reads each configuration object (root, entry, volume, and budget) into a derived
type of wrapped fields, and keeps lists and nested objects raw so a wrong container
type is reported with its path. Nothing in the validator implements or derives
`Serialize`: it writes no JSON, and the registration body is written by hand (see
[Registration](#registration)). The agent's messages derive `Serialize` and
`Deserialize` for `serde_json`.

The TOML parser is a dependency, where the JSON parser is not. The JSON rules need
behaviour no crate gives, which is why the crate owns that parser. TOML's grammar is
much larger than JSON's, and the write path needs a full document model of the file
to keep its comments, which `toml_edit` is. The project does not write a TOML reader
by hand, and it does not use the `toml` crate alone, because that crate cannot
change one value in a file and keep the file's comments. The TOML reader walks the
`toml_edit` document into the same tree of values the JSON parser makes, so the rule
set reads one kind of tree, and `toml_edit`'s `serde` feature stays off with the other
default features. For a syntax error the reader uses only the parser's fixed message
and a position, from the error's byte span or, for a key of too many dotted parts,
from `toml_parser`'s events, never its display text, which quotes the file; see
[TOML Problems](#toml-problems).

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
`.claude/settings.json`, and the Amp continuation marker.
