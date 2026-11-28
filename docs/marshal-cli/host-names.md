---
type: Definition
title: "Marshal host names"
description: "Which lock, channel and log names identify the same host across releases?"
---

# Marshal host names

## Renaming the CLI

The name `marshal` is fixed. It was provisionally `runners`, the repository's name,
until it was renamed before any release. The command-line binary that carried it is
no longer built, but the per-host names derive from it, and the agent keeps them:

- the instance lock, `\\.\pipe\marshal-instance` on Windows and
  `/Users/Shared/.marshal-instance.lock` on macOS;
- the control channel, `\\.\pipe\marshal-control-<user SID>` on Windows and a
  `control.sock` under `~/Library/Application Support/marshal/` on macOS;
- the log directory and file names, `%LOCALAPPDATA%\marshal\logs\` and
  `~/Library/Logs/marshal/`, with the `marshal-` prefix.

A rename would be a breaking change for every host that runs the last command-line
release or the app. An agent under the new names would not see a running instance's
lock, so a second one could start beside it; the app could not reach that instance's
channel; and the logs would land in a new directory, away from the earlier ones.

The app's executable is `marshal-app.exe`, a name of its own, so Task Manager tells it
from a leftover `marshal.exe`; the names above do not derive from it.

The `runners` names still in the repository are the repository's name, not the
binary's: the image labels `com.github.axross.runners.*`, the container labels
`runners.diagnostic-lifecycle` and `runners.cpu-probe`, the `.cache/runners`
directory, the CI image tag `runners-docker-test:ci`, the login-profile markers in
`.agents/setup`, the `repository=runners` telemetry attribute in
`.claude/settings.json`, and the Amp continuation marker.
