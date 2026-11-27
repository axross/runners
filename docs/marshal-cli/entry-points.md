---
type: Architecture
title: "marshal-cli entry points"
description: "Which executable runs the library and which entry points are test-only?"
---

# marshal-cli entry points

## Running It

The library has no `main`. The app's executable runs it, in the modes that
[The Executable and Its Modes](../marshal/session.md#the-executable-and-its-modes) lists: the app, the resident agent, and
the hidden modes the library relaunches it in, the diagnostics exporter and on Unix
the container and VM shims, which `marshal_cli::command::run` dispatches. No
user-facing command is dispatched there, so the executable never serves protocol
version 1.

The tests run the library's `validate`, `build`, `start` and the four clients as real
processes through [`commands-helper`](../../cli/helpers/commands_helper.rs), a
test-only program of the `marshal-cli` package like `fake-docker` and
`instance-helper`, which a release never ships. It takes the arguments the commands
took, `--config <path>` and `--profile axpc|axmc`, with the profile following the
operating system when it is left out, and relaunches as the hidden modes, so the
tests hand it to the exporter and the shims. The helper returns the library entry point's exit code.

`validate`, the library's `command::load_plan` that the agent, `start` and `build`
share, reads the configuration file and the marker files above it, and writes
nothing. It does not check that a token file exists or who can read it; `start`
does. See [building the host image](./build-images.md#build) for `build`, and [foreground host entry points](./foreground-commands.md#running-a-host) for `start` and the four clients. The following exits apply to `validate`.

| Exit | Meaning                                                                                                                                                                                                                                      |
| ---- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 0    | The configuration is valid. The plan summary is on standard output, with `\n` line endings.                                                                                                                                                  |
| 1    | The file is missing or unreadable, is not valid JSON, or breaks a rule. Every error is on standard error, in the format below and the [validation error order](./json-validation.md#error-order), and nothing is printed to standard output. |
| 2    | A usage error, including no `--profile` on an operating system that has no default.                                                                                                                                                          |

Without `--profile`, the helper takes `axpc` on Windows and `axmc` on macOS. On any
other operating system it stops with exit 2 and asks for `--profile`. The agent
takes `axmc` on macOS and `axpc` everywhere else.

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
