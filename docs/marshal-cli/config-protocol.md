---
type: Specification
title: "Configuration editing protocol"
description: "What does the agent do when a client reads, edits or reloads configuration?"
---

# Configuration editing protocol

## Editing the Configuration

`config_read`, `config_edit` and `config_reload` let a client edit `config.toml` and
apply a change without `stop_all` and `start_all`. `cli/src/agent/machine/configure.rs`
answers them, `cli/src/agent/config/` reads the edits and works out each pool's
effect, and `cli/src/agent/core.rs` applies it. The agent reads the file at its start,
at `start_all` and for each of these methods, and never watches it. Only the start,
`start_all` and an applied `config_edit` or `config_reload` make what it read the
configuration in use.

- **`revision`** is a digest of the file's bytes and their length, from std's
  default hasher, compared for equality only. It names no path. A `config_edit` or `config_reload` names the revision it was made
  from and is refused with `config_changed` when the file no longer has it.
- **`config_read`** reads the file now and changes nothing the agent holds. `status`
  is `valid`, `invalid`, `missing` or `unreadable`, the last for a file that is not
  TOML. `settings` holds `routing_label` and `image_name`, and `budget` on axmc;
  `pools` holds each pool's keys in the file's order. Both hold only keys the
  schema defines that the file sets with a value of the right type, so an undefined
  key or a wrong-typed value appears only in `problems`, each a `path`, a `line` and a
  `message`, at most 100. `schema_version` and `lifecycle` are not sent. A result
  that would pass the message limit has `too_large` true and no values.
- **`scale_set`** is an optional text setting in the same reader and editor.
  Setting, changing or removing it uses the comment-preserving write path and
  restarts only the affected pool under the effect rules below. A rejected value
  leaves the last valid configuration running.
- **`edits`** are the write path's operations as JSON, addressed by TOML key paths
  counted from 0, such as `pool[1].runners`: `{"op":"set","path","value"}`,
  `{"op":"remove","path"}`, `{"op":"add_entry","array","values"}` and
  `{"op":"remove_entry","path"}`. A value is a boolean, a number, a string or an array
  of strings. An edit of `schema_version` or `lifecycle` is `config_refused`, and a
  malformed one `invalid_request`, whose message repeats no value.
- **A check** (`check: true`) runs the edits, or reads the file for a reload, and
  checks the result with the rules the agent loads with, the checkout rule at the
  diagnostic directory's own place included, and the exporter's sink checks on each
  `diagnostic_directory` it sets or changes, and writes nothing. Without `check` the
  agent checks again and writes through [Changing the File](./toml-configuration.md#changing-the-file). On
  a running agent a change of `image_name` first checks the new image, as `start_all`
  does, and is `image_unusable`, with nothing written, when the image is missing or
  unusable. While it waits for that check, `lifecycle_set` is refused as `busy`, so a
  reload cannot put out of effect a setting saved meanwhile. A check changes no
  setting in effect and no waiting resume.
- **The answer** lists each pool's `effects`: `pool`, `change`, `runners_before`,
  `runners_after` and `stops_runners`, true when the change stops runners the pool has, which fails
  the jobs on them, on a running agent only. `repositories_released` lists the
  repositories no pool names after the change, whose tokens the client may offer to
  remove; it never lists organisation keys, and the agent removes no token.
  `applies` is true when the change applies now.
- **A reload of a file with problems** answers `config_invalid` or
  `config_unusable` and records the problems in the snapshot; the configuration in use
  stays, as `last_valid` when it came from the file.

The written file is the configuration in use from the moment the write succeeds, so
an abandoned apply, a later `start_all` and a restart of the runtime core all use
it. An applied `config_edit` or `config_reload`, on a stopped or a running agent, also
puts the `[lifecycle]` of that file in effect as `start_all` does, with the effects
[The Agent's Signals](./agent-lifecycle.md#the-agents-signals) lists, and ends a waiting
resume when `resume_after_stop` is now off; a refused or failed one leaves the
settings in effect as they were, and on axmc none changes. A write on an agent stopped
for `no_configuration` or `invalid_configuration` leaves it stopped by you, as a
successful `build` does; the runners stay stopped until `start_all`. A `config.json`
rejected at its migration also stops the agent for `invalid_configuration`, but both
methods are refused then, so that stop stays. On a stopped agent nothing else happens.
On a running agent the answer follows the write and the start of the apply, and the
snapshot shows the rest. Pools are matched by name with the configuration in use:

| Change                                                                                                     | `change`          | Effect                                                |
| ---------------------------------------------------------------------------------------------------------- | ----------------- | ----------------------------------------------------- |
| None                                                                                                       | `unchanged`       | Its runners and their jobs carry on                   |
| A new pool                                                                                                 | `added`           | Admitted and prepared as at `start_all`, then started |
| A removed or renamed pool                                                                                  | `removed`         | Its slots stop with the stop sequence                 |
| Only `runners` raised                                                                                      | `runners_added`   | The new slots start; the others carry on              |
| Only `runners` lowered                                                                                     | `runners_removed` | The highest-numbered slots stop; the others carry on  |
| Any other key of the pool, a `routing_label` it takes because it lists no labels, `image_name` or `budget` | `restarted`       | Its slots stop, then it starts as a new pool          |

A pool that waits for its token takes a new `runners` and goes on waiting, and one
that runs no slot because it was skipped is admitted and prepared again. A slot an
apply stops has its diagnostic containers finalised with its other containers, as
a stop does, and its job host is released only once nothing in it awaits that
finalisation. A slot whose stop does not end its runner within the exit deadline stays
listed as leaving until the runner ends, then starts again if the configuration in
use still has it; one whose host still holds evidence stays taken until the runners
next stop, so no new runner replaces that host first. A pool an apply restarts or
adds while slots of its own are still held in either way is not prepared over them:
it is listed `skipped` with the reason that it restarts once its earlier runners have
stopped, and it is admitted and prepared once the last of them is finished, or at the
next `start_all` when one holds evidence; while a drain for an update lasts, it is
prepared only once the drain is cancelled. A slot an apply stops shows `stopped` with the reason that the edit stops it until
its containers are stopped and its job host released, then leaves the list; a
removed pool is listed, `stopped`, until its last slot has. A pool that restarts
shows `preparing` with the reason that the edit restarts it. One apply runs at a
time: an edit or a reload meanwhile is `config_applying`, and one while the runtime
core waits to restart is `runtime_restarting`. During a drain for an update both
are `refused`, as every method is but a stop, a quit, `status`, `config_read`,
`token_set` and `token_remove`. `stop_all` and `quit` abandon the
apply, and a failure of the core fails it; the file stays written either way.
