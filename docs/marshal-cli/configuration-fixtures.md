---
type: Convention
title: "Configuration fixtures and expectations"
description: "How should a contributor change the fixtures when validation changes?"
---

# Configuration fixtures and expectations

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
- `rust/raw/` holds the inputs under [Inputs the Fixtures Pin](./json-validation.md#inputs-the-fixtures-pin). `.prettierignore` and
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
  `schema_version`, `routing_label`, `[lifecycle]`, the organisation keys and their
  rules, letter case, duplicates, wrong
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

**The fixtures that do not map** stay JSON-only. The test lists all 44 by name with
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
