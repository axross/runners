# CLI

The Rust command line tool under [`cli/`](../../cli): what `validate` accepts and
prints, the rules it applies, the shared fixtures its tests read, the TOML
configuration format with its conversion from JSON and its comment-preserving write
path, the runtime layer that runs a host's pools (the container backend, JIT registration, CPU placement,
and the slot loop), the instance layer that keeps
`start` to one instance per machine (the instance state, the single-instance lock,
and the control channel), the image contract every runner image declares and the
check of it, running a host with `build`, `start`, `stop`, `restart`, `status`, and
`diagnostics`, the diagnostic containers a diagnostics-enabled pool runs and their
export, the toolchain and the gates that cover it, how a release is built,
signed, and installed, and why its name is fixed. The binary is `marshal`, and
axpc runs its pools with it, as [Windows Runner Host](./windows-runner-host.md)
describes, including how to roll back to the previous release.

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
(the 1 MiB limit fits about 11,900) give more. The 1 MiB limit bounds both. Real
configurations are expected to have about ten entries.

Known limitation: when `validate` cannot write its output, for example a closed
pipe, it exits 1, the same code as a rejected configuration. The behaviour is
unchanged on purpose; a caller that must tell the two apart reads the output.

## Profiles

A profile names a host. The format, order, and rules under Validation Rules apply
to both, and the shared fixtures pin the message text. The profile decides the
fields, the platform labels, the routing label, the path form, and the extra
checks.

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
checkout is found, a note on standard error says the rule was not applied, and the
summary on standard output is the same with or without a checkout. The rule applies on
both profiles but can only bite where the profile's path form matches the
operating system: axpc on Windows, axmc on Linux and macOS.

Known limitation: the axmc comparison is case-sensitive, while macOS's default APFS
volume is case-insensitive. A `diagnosticDirectory` that differs from the checkout
only in case therefore passes the rule even though it names the same directory. The
validator cannot know a volume's case sensitivity, so it does not guess.

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
lines as its LF copy. A syntax error names its line and column when the parser gives a
position. One the parser cannot place, such as a dotted key or a table header nested
past its limit, names none, because a position the parser did not give would be
false.

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

- the routing label: the profile's for JSON, and the file's `routing_label` for TOML;
- whether a pool may omit `os` and `volumes`, and whether a Windows pool has an `os`;
- whether the owner and the repository are one key or two;
- whether the token-file rules apply: they do for JSON, and TOML has no token file;
- the path form of a pool, which a plan entry also records as its source path;
- which keys the format leaves undefined for the host, and where the file holds a key;
- how a problem reads.

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
  stays. A removed value takes only its own line, and a comment above it stays.
- A removed pool or volume takes its keys, a pool's volumes, and the comment lines
  directly above its header. When a blank line or the end of the file follows it, the
  blank line above it goes too. A comment set apart from the header by a blank line
  stays.
- An added entry goes after the last entry of its array, with a blank line before it.
  A comment that ends the file stays at its end.
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
  that does not exist. It is a mistake of the caller, so it carries no file text, only
  the edit and the reason. A file that already holds the text the write path uses to
  mark a removal is refused the same way.

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

| Module                                              | Holds                                                                                  |
| --------------------------------------------------- | -------------------------------------------------------------------------------------- |
| `affinity`                                          | The CPU offsets `validate` and the backend share, and the selection of a slot's CPUs   |
| `runtime::backend`                                  | The backend seam                                                                       |
| `runtime::select`                                   | The runtime a pool's operating system selects on its host                              |
| `runtime::docker`, `runtime::docker_args`           | The container backend and the exact `docker` argument lists                            |
| `runtime::registration`, `token`, `scrub`, `secret` | JIT registration, the token file, error text, and the type that hides a secret         |
| `runtime::clock`, `runtime::log`                    | The time source and the sink for the runtime's messages, each a seam the tests replace |
| `runtime::name`                                     | Runner and container names, and the test for a pool's own                              |
| `runtime::pool`                                     | Pools from a plan, and their preparation                                               |
| `runtime::worker`                                   | The slot loop and its backoff                                                          |
| `runtime::retained`                                 | The diagnostic containers the backend retains, their records, and who finalises each   |
| `runtime::diagnostic`                               | Finalising diagnostic containers at job end, in the container steps, and at startup    |
| `diagnostics`, `diagnostics::exporter`              | The exporter child and the lines and exit codes it answers with                        |
| `diagnostics::transport`                            | The transport seam through which the exporter reads a container's evidence             |
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
`cli/src/image/contract.rs` fails when `smoke-docker-commands.ps1` no longer holds the
variant's argument list and launcher as the CLI writes them. The identity and copy
commands are held equal to `smoke-diagnostic-export.ps1` by a test in
`cli/src/runtime/docker_args.rs`, and the completion marker by one in
`cli/src/diagnostics/exporter.rs`.

Every client starts in its own process group: `process_group(0)` on Unix, and
`CREATE_NEW_PROCESS_GROUP` on Windows. A Ctrl+C in the terminal then reaches the CLI
only, which can stop the containers in order. A test reads the group of every client
the fake `docker` sees on Linux and macOS; no safe API reads a Windows process's
creation flags, so the Windows flag is checked by review. A client is killed when
the future that waits for it is dropped.

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

| Host | Pool               | Runtime                                                          |
| ---- | ------------------ | ---------------------------------------------------------------- |
| axpc | every pool (Linux) | The Docker Desktop container runtime, the backend above          |
| axmc | `os: "linux"`      | The VM runtime, a Tart VM per job; selected, but not implemented |
| axmc | `os: "macos"`      | None                                                             |

The backend above is the Docker Desktop runtime. The runtime core drives the
containers through the backend seam, apart from two values it takes from the Docker
modules: the engine poll interval from `runtime::docker` and the run command from
`runtime::docker_args`. `start` and the shim construct the Docker backend
directly. No code implements the VM runtime yet, so a
configuration that selects it, or selects none for a pool, is validated and then
refused:

- `start --profile axmc` exits 1 after taking the lock and validating, before it
  opens a log file or calls an engine. It prints:

  ```text
  error: the configuration is valid, but this version of marshal cannot run axmc's VM pools yet
  ```

- `build --profile axmc` exits 1 after the same lock and validation, and prints:

  ```text
  error: the configuration is valid, but this version of marshal cannot build axmc's golden VM yet
  ```

Both refusals print on standard error, as every `start` and `build` error before
the log opens. axpc is unaffected: its commands, messages, exit codes, and the run
contract keep their text.

The diagnostics export reads a retained container's evidence through the
`EvidenceTransport` seam in `diagnostics::transport`: the allowlisted identity
text, and the evidence archive, which the strict archive reader writes into the
bundle. The exporter child's admission, bundle files, limits, exit codes, and
deadline do not depend on the transport. Its first transport, `DockerCopy`, runs
`docker inspect` and `docker cp` as [Backend](#backend) lists them.

A unit test in `runtime::select` pins the table. `start_command.rs` and
`build_command.rs` pin the two refusal texts and their exit code, and
`start_command.rs` also refuses a configuration of Linux and macOS pools and one of
macOS pools alone. Unit tests in `diagnostics::exporter` write a complete bundle, a
gap, and the failed bundles of an unreadable identity and a failed archive through a
scripted transport, and the
diagnostics tests listed under [Diagnostics](#diagnostics) run the exporter through
`DockerCopy` with unchanged expectations.

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

A slot loops until it is asked to stop. Each attempt checks the image, then probes
the CPUs, then registers, then runs the container. An image that cannot run jobs,
or a check Docker does not answer, registers nothing and goes into the backoff. A
failed probe or registration, or a non-zero exit, waits
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
fails when Docker or the image is missing. Not verified by these tests: a real
Docker daemon outside that CI step, the TLS handshake with GitHub, and Windows and
macOS runtime behaviour beyond what the hosted runners show. The handshake and a
real registration are verified on axpc instead, on the maintainer's report, as the
TLS paragraph above says.

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
so that check is in PowerShell. A hosted runner cannot show the Windows lock
against a second OS user or the rejection of remote clients, and both are not yet
verified on axpc
([#56](https://github.com/axross/runners/issues/56#issuecomment-6511420845)):
neither check was run there, by the maintainer's decision.
Not verified: macOS and Windows behaviour beyond what the hosted runners show.

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
changes or the image must provide something it did not. Contract 1 is the run
contract as Phase 2 left it, plus the diagnostic variant's use of the collector's
path and output directory.

Unit tests in `cli/src/image/contract.rs` keep the declaration honest:

- `APPROVED_CONTRACTS` pins a fingerprint of the run contract's argument lists, the
  launcher, the diagnostic variant's argument lists and launcher, the label keys
  and the inspect format, against
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
- one `warning: <text>` line per freshness warning.

`image-built` and `runner` read `unknown` before the first check Docker answers, and
when the image is missing or its labels are missing or malformed.

## Running a Host

`start` runs the host's pools in the foreground through the instance layer and the
runtime layer, and `stop`, `restart`, `status`, and `diagnostics` talk to it from another terminal
over the control channel. `build` builds the runner image, and the same clients
talk to it. axpc runs its pools with `start` and builds its image with `build`, as
[Windows Runner Host](./windows-runner-host.md) describes.

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
   configuration still exits 1, with the refusal that [Runtime Seam](#runtime-seam)
   gives.
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
`stop` still ends within 30 s and its 35 s wait is unchanged.

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
   `--profile axmc` exits 1 after validation with the refusal that
   [Runtime Seam](#runtime-seam) gives. An image name given by digest is refused,
   because it cannot be tagged. It prints the name it builds.
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
beside the JSON ones and not in place of them.

- **Hand-written TOML.** [`tests/host-configuration/toml/`](../../tests/host-configuration/toml)
  holds `accepted/` and `rejected/` documents and one `expectations.json`, read by
  `cli/tests/toml_fixtures.rs`. These are the cases only TOML has: syntax errors,
  `schema_version`, `routing_label`, `[lifecycle]`, letter case, duplicates, wrong
  types including a date, nesting deeper than the parser allows, which is placed for
  an array or an inline table and is not for a dotted key or a table header, the
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
3. runs the base-address, instance-roots, and build-definition tests with
   `--release` and `MARSHAL_EXPECT_RELEASE=1`, the same tests the Linux `rust` job
   runs, on the target itself;
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

A release never ships `fake-docker` or `instance-helper`. The release-profile tests
build both into `target/release`, so the build names `marshal` alone and copies only
that file out, each leg's artifact holds that binary and its installer, and the
assemble job, the attest job, and the release gate refuse a set of files that holds
anything else.

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
maintainer fixes it there.

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
| `toml_edit`, with `toml_parser`, `toml_writer`, and `toml_datetime`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                | Reading the TOML host configuration into a document that keeps each key's position, and changing a document while keeping its comments, with default features off and only `parse` and `display`; `display` prints a document back                                                                                             | MIT OR Apache-2.0                                                                           |
| `winnow`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           | The parser library under `toml_parser` and `toml_edit`                                                                                                                                                                                                                                                                         | MIT                                                                                         |
| `indexmap`, `hashbrown`, and `equivalent`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          | The order-keeping map under `toml_edit`'s tables, and its hash map and lookup trait                                                                                                                                                                                                                                            | Apache-2.0 OR MIT for `indexmap` and `equivalent`, MIT OR Apache-2.0 for `hashbrown`        |

`aho-corasick` and `memchr` are in the lockfile because `regex` lists them as
optional, but the CLI's feature set (`std` and `unicode-perl`, no `perf`) does not
build them. They are accepted under their MIT alternative, so the Unlicense is not
on the allowlist and need not be.

The JSON parser is in the crate, not a dependency. The rules under Validation Rules
need order-preserving objects, the last-position duplicate rule, case-conflict
errors, and number literals kept as text. The crate owns that parser, and the
fixtures pin it.

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

The TOML parser is a dependency, where the JSON parser is not. The JSON rules need
behaviour no crate gives, which is why the crate owns that parser. TOML's grammar is
much larger than JSON's, and the write path needs a full document model of the file
to keep its comments, which `toml_edit` is. The project does not write a TOML reader
by hand, and it does not use the `toml` crate alone, because that crate cannot
change one value in a file and keep the file's comments. The TOML reader walks the
`toml_edit` document into the same tree of values the JSON parser makes, so the rule
set reads one kind of tree, and `toml_edit`'s `serde` feature stays off with the other
default features. For a syntax error the reader uses only the parser's fixed message
and its byte span, never its display text, which quotes the file; see
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
