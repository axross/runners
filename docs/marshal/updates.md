---
type: Specification
title: "Signed updates and drain outcomes"
description: "When does the app install an update, wait for a job, or prompt again?"
---

# Signed updates and drain outcomes

## Updates

W4 ([#78](https://github.com/axross/runners/issues/78)) owns this section. An
installed app updates itself from a signed feed: it asks its agent to drain, waits for
the jobs already running to finish, and installs the new release.
[Security](../security/app-trust.md#updates-come-only-from-the-signed-feed) holds
the rules the update path is reviewed against, and
[CLI](../marshal-cli/agent-lifecycle.md#the-update-drain) holds the agent's drain.

The maintainer's update-feed public key, minisign key ID `6F39E3131E21E1E4`, is
committed in `tauri.conf.json`. Until the release run publishes the feed, which W4's
last pull request adds, a check finds no feed and changes nothing; an app with no
committed key, or a development build at `0.0.0-dev`, never reads the feed.

**Checking.** The app reads
`https://github.com/axross/runners/releases/latest/download/latest.json` when it
starts and every hour after, through Tauri's updater plugin, which only the app mode
registers. A check gives up after 30 s; a failed one is logged and changes nothing
else. For a release newer than its own, the app checks the download address (`https`,
`github.com`, under `/axross/runners/releases/download/`), downloads the MSI within 10
minutes, and the plugin verifies its signature against the committed key and that the
signature names the feed's version. Only then does it hold the update, and it does not
check again while it holds one.

**Draining.** Holding a verified update, the app asks its agent for
`drain_for_update` once the agent is running or stopped, no drain is under way and no
action of the window waits for an answer. While the agent resumes, builds or stops,
or another client's drain is under way, it waits; a refused drain is asked again a
minute later. While the agent drains, the host summary, the menu's first line and the
icon's tooltip read `Updating to <version>: waiting for <N> runners to finish`, or
`Updating to <version>` when it waits for none, and the live region says so; Start
All and Build stay unavailable. Stop all stops every runner and the agent ends for the
update with the runners stopped by you; Quit installs nothing.

**Installing.** After the agent's `ending` for this app's drain, the app waits up to
60 s for the agent's process to exit, from the process ID the drain's answer or the
snapshot gave. It then writes the record below, saying whether `ending` said the
runners were running, and starts `msiexec.exe /i <msi> /passive /promptrestart
AUTOLAUNCHAPP=True LAUNCHAPPARGS="<its own arguments>"`; the plugin then ends the app.
The installer shows Windows Installer's progress window. Starting the new app, which
starts its agent, depends on the MSI honouring `AUTOLAUNCHAPP` and `LAUNCHAPPARGS`;
W1's installer pull request tests it in `test-msi-installer.ps1`. If the process has not exited in 60 s, or the installer does not
start, the app drops the update and the record, handles the agent's end as any other,
and checks again an hour later. An end for an update another client asked for is
handled as any other end.

**A lost connection during the drain.** If the agent's process still runs, the app
reaches it again without starting another. If the agent's first snapshot still shows
this app's drain, for the same version and process, the drain was past its last steps
and goes on, and its `ending` installs as above; otherwise the app asks again. If no
agent answers while the process still runs, the app waits for it to exit as after
`ending`. If it has
exited, the agent ended unexpectedly: the app installs as above, and the record says
the runners stay stopped if the last state it heard keeps them stopped, as the
restart after a drop reads it. When nothing installs, the agent is started again
stopped or running as `ending`, or that last state, said.

**The record.** `update-stopped-by-you.json` in `%LOCALAPPDATA%\marshal\` names the
update's version, when it was written, and `running`, whether the runners were
running; a record without `running` is one written before it was added, and keeps
them stopped. A launch of that version within 30 minutes of it starts the agent
running when `running` is true and stopped otherwise, without consulting
`start_runners_on_launch`, so an update never undoes a stop by you or by a signal;
every launch removes it, so it applies once. Without a record that applies, a launch
is fresh.

**An older agent.** An app that finds a running agent older than the agent it starts,
for example after an installer was run by hand, asks it to drain for that version and
then starts its own agent, stopped unless `ending` said the runners were running.
The comparison is with the version of the `marshal-cli` library the app links, which
the agent gives in `welcome`, not with the app's own package version, so a release
that stamps the two differently does not replace the app's own agent again and again.
An older agent that does not list `drain_for_update` is used as it is, logged once,
and nothing installs while it runs.

**After a failed install.** Windows Installer rolls back to the installed version,
but Marshal stays off until it is launched from the Start menu, which starts the
runners and checks again. Tauri's updater leaves each downloaded MSI in a
`Marshal-<version>-updater-*` folder under the temporary folder; Windows' storage
cleanup removes it. The unsigned executable may lose its place in the notification
area after an update.

**The feed check.** The `marshal-feed` crate holds the address rule and the same
signature rules the plugin applies, and builds `marshal-feed-check`, which the release
run is to use on the feed it writes before anything is published:

```text
marshal-feed-check [--public-key <file>] <latest.json> <version> <SHA256SUMS> <platform>=<file>...
```

It checks the feed's version, that it has exactly the given platforms, each entry's
address under the release's tag, each file's SHA-256 against `SHA256SUMS`, and each
signature, and lists every problem. Without `--public-key` it checks against the
release-verification public key embedded when it is built, which is separate from the
key in `tauri.conf.json`; [Security](../security/app-trust.md#updates-come-only-from-the-signed-feed)
owns that rule. `--public-key` selects a throwaway key for the merge checks' round
trip. Its tests use throwaway keys in `app/crates/feed/tests/fixtures/`.

**Where the feed stands.** The scripts that sign and write the feed and the merge checks
that run them are in place; the release run does not publish `latest.json` yet. The
jobs' text is staged in `.github/staged/release-feed.yaml`, outside `.github/workflows/`,
where GitHub never runs it, and the signing scripts, the committed `tauri-cli` archive
checksum and the merge checks' round trip are described in
[CLI](../marshal-cli/release-artifacts.md#the-update-feeds-tools-and-checks). The last pull
request of W4 moves the staged text into the release run once the app's publication is
restored, and the first release after it is the first to carry a feed.

### The Update-Feed Key

The key is a minisign key made by `tauri signer generate`, protected by a password.
Agents never handle the private key or its password.

The maintainer manages the key as follows:

- **Generation.** Generate and publish the public key as follows:
  1. Generate the key on your own machine with a password.
  2. Post the public key on [#78](https://github.com/axross/runners/issues/78).
  3. Commit the public key as the updater's `pubkey` in
     `app/crates/app/tauri.conf.json`, with the same approved public value in the
     release-verification source, `app/crates/feed/release-public-key.txt`.

  Both keys now committed have the minisign key ID `6F39E3131E21E1E4`.

- **The two secrets.** The maintainer stores the private key, the whole text of the
  file `tauri signer generate` wrote, as the `release` Environment secret
  `UPDATE_FEED_SIGNING_KEY`, and its password as `UPDATE_FEED_SIGNING_KEY_PASSWORD`.
  No job reads them until the feed's last pull request adds `sign-update`.
- **The offline backup.** The maintainer keeps a copy of the key file and of the
  password offline, the two stored apart, and reports on the tracking issue that the
  secrets and the backup exist before the first release after the feed lands.
- **Rotation.** Installed apps trust one key, so a new key is introduced through a
  release:
  1. Commit the new app public key, retaining the old public key in the separate
     release-verification source. Sign the transition release with the old key,
     which the installed apps and the publication check both accept.
  2. Replace the two secrets with the new key and its password, and switch the
     committed release-verification source to the new public key, only after the
     maintainer confirms that the hosts have taken that release and before
     publishing the next release. A mismatch blocks publication; no override
     bypasses the check.
  3. Reinstall any host that missed the release in step 1 from a newer release's
     MSI.
- **Loss.** Without the old key no installed app accepts a new one through the feed.
  Recover as follows:
  1. Generate a new key.
  2. Commit its public key in both the app and release-verification sources.
  3. Release with the new signing key.
  4. Reinstall each host from that release's MSI.
- **A leak.** Rotate at once, as above.
