---
type: Specification
title: "TOML configuration and edits"
description: "Which TOML keys, validation errors, conversions and comment-preserving edits are supported?"
---

# TOML configuration and edits

## The TOML Configuration

A host configuration can also be written as TOML, schema version 1. The library
reads that format with the same rules as JSON, converts a JSON configuration to it,
and changes an existing file while keeping its comments. The agent reads its
configuration as `config.toml` and migrates a `config.json` to it, as
[The Agent's Configuration](./resident-agent.md#the-agents-configuration) describes. The commands do
not: `validate`, `build`, `start`, and the rest take JSON through `--config`, as [the library entry points](./entry-points.md#running-it) describe. The roadmap issue [#72](https://github.com/axross/runners/issues/72) names the later
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
| `[lifecycle]`    | table                                   | Optional. The five lifecycle keys are described below.                                                                                                                                                                                                |
| `[[pool]]`       | array of tables                         | At least one. Each table is one entry of today's `repositories`.                                                                                                                                                                                      |

| `[[pool]]` key           | Type                                | Renames                          | Rule                                                                                                                                    |
| ------------------------ | ----------------------------------- | -------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------- |
| `name`                   | string                              | `name`                           | Required.                                                                                                                               |
| `repository`             | string                              | `owner` and `repository`         | Exactly one of this and `organization` is required. Its value is `"owner/name"`; each part follows the owner or repository rule.        |
| `organization`           | string                              | none                             | The organisation's login, under the owner rule. TOML only.                                                                              |
| `runner_group`           | string                              | none                             | Required with `organization`, rejected without it. The exact group name in GitHub, not blank and without control characters.            |
| `runners`                | integer                             | `slots`                          | Required on axpc. Optional on axmc, with the JSON key's default: 2 on a Linux pool and 1 on a macOS pool.                               |
| `labels`                 | array of strings                    | `labels`                         | Optional. Absent or empty gives the platform labels plus `routing_label`.                                                               |
| `os`                     | `"linux"`, `"macos"`                | `os`                             | Optional on both hosts, and a pool without it is a Linux pool. Windows accepts `"linux"`, which changes nothing, and rejects `"macos"`. |
| `cpus`                   | number                              | `cpus`                           | Optional, with the JSON key's default and range.                                                                                        |
| `memory_gb`              | integer                             | `memoryGb`                       | Optional, with the JSON key's default and range.                                                                                        |
| `disk_gb`                | integer                             | `diskGb`                         | macOS: required on a Linux pool and optional on a macOS pool, as `diskGb` is. Windows: an undefined key.                                |
| `diagnostics`            | boolean                             | `diagnostics`                    | Optional, false by default.                                                                                                             |
| `diagnostic_directory`   | string                              | `diagnosticDirectory`            | Required with `diagnostics`, invalid without it, under the checkout rule.                                                               |
| `diagnostic_raw_records` | boolean                             | `diagnosticRawRecords`           | Optional, false by default, invalid when true without `diagnostics`.                                                                    |
| `[[pool.volumes]]`       | tables of `suffix` and `mount_path` | `volumes`, `suffix`, `mountPath` | Windows: optional, and a pool without them has none. macOS: rejected.                                                                   |

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
- **Organisation pools.** `organization` and `runner_group` select an organisation
  runner group instead of one repository. Such a pool rejects `volumes`, including an
  empty list, with `not allowed with organization`. `scale_set` is not a known key of
  this configuration, so it is reported as an undefined key, except on an organisation
  pool, where it is reported as `not allowed with organization`. JSON gains no
  organisation keys. With the same OS, an organisation pool must not share a
  non-platform label with another pool of that organisation or any of its repositories,
  comparing logins and labels without case. This includes the default `routing_label`,
  so overlapping pools need distinct explicit labels. Repository-only files retain their
  validation rules. A pool naming neither target reports
  `pool[i]: names neither repository nor organization`, and one naming both reports
  `pool[i]: names both repository and organization`. An organisation pool without
  `runner_group` reports `pool[i].runner_group: required with organization`, and a pool
  with `runner_group` and no `organization` reports
  `pool[i].runner_group: only with organization`.
- **No token key.** The file holds no token and no token path, so `token_path` is an
  undefined key. A pool's token is not a configuration setting, and the token-file
  rules of the JSON profiles do not apply to TOML.
- **`[lifecycle]`** holds five optional booleans, `start_at_login`,
  `start_runners_on_launch`, `keep_awake`, `stop_on_network_change`, and
  `resume_after_stop`. Any other key in
  it is a problem, and an absent key means "not set". The rule set checks and keeps
  the values. The agent applies them on axpc and on axmc, with the same defaults on
  both; [The Agent's Signals](./agent-lifecycle.md#the-agents-signals) gives what each
  setting does and its default.
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

**Order.** The order is the [error order shared with JSON](./json-validation.md#error-order), with the TOML key names:

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
   `repository`, `organization`, `runner_group`, `os`, `labels`, `volumes`,
   `runners`, `name`, `cpus`, `memory_gb`, on macOS `disk_gb`, then `diagnostics` and `diagnostic_raw_records` when they are
   not booleans, `diagnostic_directory`, and last `diagnostic_raw_records` when it
   requires `diagnostics`.
7. The checks across pools, for each pool after the first against each pool before
   it: a colliding `name`, then an ambiguous routing label, which for an organisation
   pool is the organisation routing overlap in the Organisation pools bullet. Then on
   macOS the budget totals.

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
new and are pinned by their own fixtures, under [Fixtures and Tests](./configuration-fixtures.md#fixtures-and-tests).

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

`convert_to_toml(json, host, source, rule, clock)` in `cli/src/convert.rs` turns
the bytes of a JSON configuration into the text of an equivalent TOML document, and
checks that text before it returns it, both under the checkout `rule`. It reads and
writes no file: the caller writes the text, as the agent's migration does.

1. It validates the JSON under the host's JSON rules, which still require `tokenPath`.
   A rejected file is `SourceRejected`.
2. It writes the document, by structure. It sets `schema_version = 1`, and
   `routing_label` to the host's former label, `axpc` or `axmc`. It renames the keys,
   joins `owner` and `repository`, and leaves out `tokenPath`. It writes each key the
   JSON file set and no other, so a pool without `labels` still gets the routing label
   by default. It writes no `[lifecycle]`.
3. It validates the document with the full TOML rules under the same checkout rule. A
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
alone. The settings and pool editor use this write path. The tests exercise it.

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
  the edit and the reason.
- `FileForm`: the file is written in a form the write path does not edit, which only
  the user can change: a file that already holds the text the write path uses to mark
  a removal, or an edit inside an inline table written over several lines, which TOML
  1.1 allows, since its members' comments sit where the edit would move them. Like
  `Inapplicable` it carries only the edit, or the marker, and the reason. A caller
  shows it as a file to rewrite, for example that table on one line or as a
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
when it starts and before anything writes. The agent calls it at its start.
