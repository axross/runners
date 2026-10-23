# CLI

The Rust command line tool under [`cli/`](../../cli): what `validate` accepts and
prints, how it stays equal to the PowerShell validator, the shared fixtures both
read, the toolchain and the gates that cover it, and what to touch when its
provisional name changes. The binary is `runners`. It has one command so far,
`validate`; nothing that runs jobs uses it yet, and the PowerShell supervisor under
[`hosts/windows-docker-desktop/`](../../hosts/windows-docker-desktop) still runs the
axpc host.

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
square of the number of entries. On a release build, 8,000 entries (about 0.8 MB)
take under a second when nothing collides. Colliding entries grow the error output
with the square too, because every error is reported: 8,000 entries in one
repository with the default labels print about 32 million error lines and use
about 9 GB of memory. The 1 MiB limit bounds both. Real configurations have about
ten entries.

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
clippy, and tests on Linux, Windows, and macOS, and the axpc checkout-rule test on
Windows, which needs drive-letter paths. Tests find fixtures relative to the crate's
manifest directory, and the checkout-rule tests build a temporary directory with the
marker files at test time. The advisory check, `cargo deny --locked check advisories`,
fetches the RustSec database, so it runs in
[`rust-advisories.yaml`](../../.github/workflows/rust-advisories.yaml) and not in
`mise run check`; see [Development Workflow](./development-workflow.md).

## Dependencies

A new crate MUST NOT be added without an entry here, and every crate in
[`cli/Cargo.lock`](../../cli/Cargo.lock) has one. [`cli/deny.toml`](../../cli/deny.toml)
allows crates.io only, one version of each crate, and the licences below. The CLI
is unpublished, so it carries no licence field of its own; `deny.toml` ignores
private crates for that reason.

| Crate                                                  | Why                                                                                                    | Licence                             |
| ------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ----------------------------------- |
| `clap`, with `clap_builder`, `clap_lex`, and `anstyle` | Argument parsing, with default features off                                                            | MIT OR Apache-2.0                   |
| `regex`, with `regex-automata` and `regex-syntax`      | The whole-value patterns for names, labels, and image names                                            | MIT OR Apache-2.0                   |
| `aho-corasick` and `memchr`                            | Optional dependencies of `regex`                                                                       | Unlicense OR MIT                    |
| `serde`, with `serde_core`                             | Reading the parsed JSON tree and the shared expectations into derived types, with default features off | MIT OR Apache-2.0                   |
| `serde_derive`, `syn`, `quote`, and `proc-macro2`      | The `Deserialize` derive; these run at build time                                                      | MIT OR Apache-2.0                   |
| `unicode-ident`                                        | A dependency of `syn` and `proc-macro2`                                                                | (MIT OR Apache-2.0) AND Unicode-3.0 |

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
type is reported with its path. Nothing implements or derives `Serialize`, because
nothing writes JSON.

Allowed licences are `MIT`, `Apache-2.0`, and `Unicode-3.0`; a crate whose licence is
an `OR` expression is accepted when one alternative is on that list. `Unicode-3.0`
is on the list because `unicode-ident`, which `syn` uses, is
`(MIT OR Apache-2.0) AND Unicode-3.0`, and an `AND` needs both sides allowed. The
maintainer approved it at plan revision 10. Adding another licence requires the
maintainer's review of it.

`serde_derive`, `syn`, `quote`, and `proc-macro2` run at build time. A new
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
