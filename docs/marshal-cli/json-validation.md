---
type: Specification
title: "Host profiles and JSON validation"
description: "Which host profile, checkout boundary and validation errors apply to a JSON configuration?"
---

# Host profiles and JSON validation

## Profiles

A profile names a host. The format, order, and rules under [Validation Rules](#validation-rules) apply
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
A macOS pool's `slots`, `cpus`, `memoryGb`, and `diskGb` are optional and have
minimums; [macOS VM Pools](./macos-pools.md#macos-vm-pools) states them.

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
When any macOS pool is configured, the disk total also counts a fixed 100 GB for the
golden macOS VM and its cached base, and the message adds
`, plus 100 GB for the golden macOS VM and its cached base` after the Linux clause,
or in its place without a Linux pool. The summary's disk line includes both
reserves and ends `, including a <n> GB reserve`. Without a Linux or macOS pool the
total and the message are the sum of slots times `diskGb` alone.

### Migrations

A change to what `axmc` accepts is recorded here with each old and new rule, its
message, and the fixture that shows it. A configuration that validated before the
change may be rejected now; the last column says how to change it.

| Rule                                 | Old                                                                                          | New                                                                                                                                                                                                                                                  | Fixture                                                                                                                                                                                                                                                                                                                                                                                                            | To migrate                                                                                       |
| ------------------------------------ | -------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ |
| `cpus` of a pool                     | A number above 0 and at most 64, fractions allowed                                           | An integer from 1 to 64: `repositories[i].cpus: must be an integer from 1 to 64`                                                                                                                                                                     | [`axmc-fractional-cpus.json`](../../tests/host-configuration/rust/rejected/axmc-fractional-cpus.json), accepted before and rejected now                                                                                                                                                                                                                                                                            | Round each fractional `cpus` to a whole vCPU count, at least 1                                   |
| `budget.cpus`                        | A number above 0 and at most 64, fractions allowed                                           | An integer from 1 to 64: `budget.cpus: must be an integer from 1 to 64`                                                                                                                                                                              | [`axmc-budget-members.json`](../../tests/host-configuration/rust/rejected/axmc-budget-members.json), message changed; `axmc-fractional-cpus.json`                                                                                                                                                                                                                                                                  | Round `budget.cpus` to a whole number that covers the pools                                      |
| `diskGb` of a Linux pool             | Any integer from 1 to 16384                                                                  | At least 20: `repositories[i].diskGb: must be at least 20 on a Linux pool, the golden VM's disk size`                                                                                                                                                | [`axmc-linux-disk-too-small.json`](../../tests/host-configuration/rust/rejected/axmc-linux-disk-too-small.json), accepted before and rejected now; its macOS pool, at 50 GB, gets no error                                                                                                                                                                                                                         | Raise a Linux pool's `diskGb` to 20 or more; a macOS pool needs at least 50, as its own row says |
| `imageName` of axmc                  | Optional and validated as on axpc, with its use undecided                                    | Optional and validated as before; an absent name is `actions-runner:local`, and the name is the image that `build` makes inside the golden VM and every pool runs                                                                                    | [`axmc-image-name.json`](../../tests/host-configuration/rust/accepted/axmc-image-name.json), accepted before and now; nothing is accepted or rejected differently                                                                                                                                                                                                                                                  | None; a configuration that validated still validates                                             |
| Disk budget with a Linux pool        | `budget: pools total N GB of disk (the sum of slots times diskGb), above the budget of M GB` | `budget: pools total N GB of disk (the sum of slots times diskGb, plus 40 GB for the golden VM and Tart's image cache), above the budget of M GB`                                                                                                    | [`axmc-disk-over-budget.json`](../../tests/host-configuration/rust/rejected/axmc-disk-over-budget.json), message and total; [`axmc-budget-at-limit.json`](../../tests/host-configuration/rust/accepted/axmc-budget-at-limit.json), whose budget rose from 150 to 190                                                                                                                                               | Raise `budget.diskGb` by 40 GB, or lower the pools' `diskGb` or `slots`                          |
| `slots` of a Linux pool              | Required: `repositories[i].slots: required field is missing`                                 | Optional, default 2; axpc still requires it                                                                                                                                                                                                          | [`axmc-linux-default-slots.json`](../../tests/host-configuration/rust/accepted/axmc-linux-default-slots.json), rejected before and accepted now                                                                                                                                                                                                                                                                    | None; a configuration that validated still validates                                             |
| Disk budget with a macOS pool        | The sum of slots times `diskGb`, with the Linux reserve when a Linux pool is configured      | The same, plus 100 GB: `…, plus 100 GB for the golden macOS VM and its cached base), above the budget of M GB`                                                                                                                                       | [`axmc-budget-errors-come-last.json`](../../tests/host-configuration/rust/rejected/axmc-budget-errors-come-last.json), message and total; [`axmc-budget-at-limit.json`](../../tests/host-configuration/rust/accepted/axmc-budget-at-limit.json), whose budget rose from 190 to 290                                                                                                                                 | Raise `budget.diskGb` by 100 GB, or lower the pools' `diskGb` or `slots`                         |
| Size of a macOS pool                 | `cpus`, `memoryGb` and `diskGb` as on a Linux pool, without the Linux `diskGb` minimum       | At least 2, 4 and 50: `repositories[i].cpus: must be at least 2 on a macOS pool`, `repositories[i].memoryGb: must be at least 4 on a macOS pool`, and `repositories[i].diskGb: must be at least 50 on a macOS pool, the golden macOS VM's disk size` | [`axmc-macos-cpus-below-minimum.json`](../../tests/host-configuration/rust/rejected/axmc-macos-cpus-below-minimum.json), [`axmc-macos-memory-below-minimum.json`](../../tests/host-configuration/rust/rejected/axmc-macos-memory-below-minimum.json) and [`axmc-macos-disk-below-minimum.json`](../../tests/host-configuration/rust/rejected/axmc-macos-disk-below-minimum.json), accepted before and rejected now | Raise each value to its minimum, or remove `cpus` or `memoryGb` to take the default              |
| `diagnostics` of a macOS pool        | Accepted                                                                                     | `true` is rejected: `repositories[i].diagnostics: not supported on a macOS pool, because macOS VM pools have no diagnostics`                                                                                                                         | [`axmc-macos-diagnostics.json`](../../tests/host-configuration/rust/rejected/axmc-macos-diagnostics.json), accepted before and rejected now                                                                                                                                                                                                                                                                        | Remove `diagnostics`, or set it to `false`                                                       |
| `cpus` default of a macOS pool       | 2, the axmc default                                                                          | 4, counted in the CPU budget                                                                                                                                                                                                                         | [`axmc-macos-defaults.json`](../../tests/host-configuration/rust/accepted/axmc-macos-defaults.json)                                                                                                                                                                                                                                                                                                                | Set `cpus`, or raise `budget.cpus`                                                               |
| `slots` and `diskGb` of a macOS pool | Required, as on a Linux pool                                                                 | Optional, with defaults of 1 and 50                                                                                                                                                                                                                  | [`axmc-macos-defaults.json`](../../tests/host-configuration/rust/accepted/axmc-macos-defaults.json), rejected before and accepted now                                                                                                                                                                                                                                                                              | None; a configuration that validated still validates, unless another row above rejects it        |

### Labels

`validate` rejects two entries for the same repository that share a non-platform
label and have the same `os`, and accepts them when `os` differs. In a TOML file it
also rejects an organisation pool and another pool of the same organisation, or of
one of its repositories, under the same condition; [the TOML schema](./toml-configuration.md#schema-version-1) states that rule. On axpc every entry is Linux, so the existing rule and message
stand for JSON.
[Security](../security/index.md) owns the rule, including what a consumer
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
is a configuration problem, `pool[n].diagnostic_directory: must be outside the
checkout` in `config.toml` (or `repositories[n].diagnosticDirectory` in a `config.json`
it migrates), in the snapshot. With no configuration held the agent stops for
`invalid_configuration`; at a Start all it keeps the last valid one, as [The Agent's
Configuration](./resident-agent.md#the-agents-configuration) describes. The agent
looks from a sink only when the path is one of the host it runs on, so a Windows path
read on another host is not compared. Because the sink's own place was examined, the
agent writes no note that the rule was skipped. `start`, `validate` and `build` are
unchanged: their checkout stays the one above the configuration file.
`validate_toml_with` and `validate_with` take this rule as
`CheckoutRule::AboveAndAtSink`, and `validate_toml` and `validate` keep `start`'s.
`tests/checkout_rule_agent.rs` covers a sink inside a checkout, in JSON and in TOML,
outside one, a sibling prefix, a case variant (accepted on Linux, refused on macOS), and
the unchanged rule of `start`.

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
Tests](./configuration-fixtures.md#fixtures-and-tests) says how they are read. `validate` is a pure function
from the file's bytes and a profile to a plan or an ordered list of errors, and it
records every error it finds rather than stopping at the first.

These rules read a JSON file. [The TOML Configuration](./toml-configuration.md#the-toml-configuration)
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
   order: a colliding `name`, then for the same repository a different token file,
   or for another repository a reused token file, then an ambiguous routing label
   for the same repository. A JSON configuration has no organisation pools, so it has
   no organisation routing check.
5. On axmc, the budget checks, over the same entries as step 4.

The exit code is 1 and standard output is empty whenever there is an error. A valid
configuration prints its summary on standard output, with lines ending in `\n`:

- **axpc.** The header `Host configuration is valid: <n> entries, image <name>`, a
  CPU policy line that only the axpc summary has, and for each entry its target
  (`Repository owner/name`, or `Organization X, runner group Y` for an organisation
  pool), slots, labels, token file, name, container name pattern, CPU and
  memory limits, planned affinity count and position, diagnostic settings, and
  volumes.
- **axmc.** The header `Host configuration is valid: <n> entries, profile axmc`,
  followed by `, image <name>` when the configuration names an image, and no CPU
  policy line. Each entry has its target as on axpc, `os`, slots, labels, token file,
  name, CPU and memory limits, `disk`, and diagnostic settings, but no container
  name pattern, affinity, or volume lines. A `Budget use` block ends the summary,
  with the macOS VMs, CPUs, memory, and disk used against the limit.

### Behaviour That Is Easy to Miss

- **Required and direct fields.** A field read as a required value reports `null`
  as `required field is missing`: `repositories`, `imageName` on axpc, and per entry
  `owner`, `repository`, `tokenPath`, `slots` on axpc, `name`, `volumes`,
  `diagnosticDirectory`, a volume's `suffix` and `mountPath`, and on axmc `budget`,
  its members, `os`, and a Linux pool's `diskGb`. A field read directly reports `null` as its own
  type error: `cpus`, `memoryGb`, `labels`, `diagnostics`, and
  `diagnosticRawRecords`.
  On axmc every pool reads `slots` the second way, and a macOS pool reads `diskGb`
  the second way too; see [macOS VM Pools](./macos-pools.md#macos-vm-pools).
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
test named under [Running It](./entry-points.md#running-it).

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
