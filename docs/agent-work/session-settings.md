---
type: Convention
title: Claude Code telemetry and session settings
description: Choose operator-specific session settings and distinguish them from project telemetry attribution.
---

# Claude Code telemetry and session settings

## Telemetry Tagging

[`.claude/settings.json`](../../.claude/settings.json) carries an `env` block
setting two OpenTelemetry variables, so this project's usage separates from every
other repository sharing an account or a cloud environment.
`OTEL_RESOURCE_ATTRIBUTES` stamps `repository=runners` onto the resource Claude
Code exports; `OTEL_METRICS_INCLUDE_ENTRYPOINT` adds the session's launch surface
to metric datapoints. They are two mechanisms rather than one: the resource
describes what is emitting, and the datapoint attribute describes one emission.
The block configures nothing else, no endpoint, no credential, and no
`CLAUDE_CODE_ENABLE_TELEMETRY`, so a contributor who has never set telemetry up
sees no behavior change from it.

Verifying a change to that block is the catch: Claude Code does not pass `OTEL_*`
variables to the subprocesses it spawns, so `echo $OTEL_RESOURCE_ATTRIBUTES`
inside a session prints nothing even when the exporter holds the value. Confirm it
in the metrics backend instead, against a session started **after** the change.

## Recommended Environment Variables

Two environment variables are worth setting for any Claude Code session run
here, cloud or local, per the upstream skills library's own cost analysis
([axross/skills#506](https://github.com/axross/skills/issues/506)) rather than a
measurement of this repository's own usage:

- `CLAUDE_CODE_AUTO_COMPACT_WINDOW=500000` moves auto-compaction's trigger
  earlier, which lowers the average main context a long session carries.
- `CLAUDE_CODE_ENABLE_PROMPT_SUGGESTION=false` stops prompt-suggestion
  generation, a recurring cost that sessions here do not use.

Set them in the environment dialog at claude.ai/code for a cloud session, or in
`~/.claude/settings.json` for a local one. `~/.claude/settings.json` does
**not** reach a cloud session; its scope stops at your own machine.

`CLAUDE_AUTOCOMPACT_PCT_OVERRIDE` is not a substitute for the first variable:
Claude Code on the web sets it itself, and its value overrides whatever is added
to the environment.

Neither variable belongs in a committed settings file: a cost-saving behavior one
contributor wants is not something to impose on another, which is also why
neither is added to [`.claude/settings.json`](../../.claude/settings.json) or
[`.claude/settings.local-example.json`](../../.claude/settings.local-example.json)
here.
