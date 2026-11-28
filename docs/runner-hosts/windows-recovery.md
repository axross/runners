---
type: Procedure
title: "Recover a Windows runner host"
description: "Diagnose startup, image and registration problems and decide whether a compatible rollback exists."
---

# Recover a Windows runner host

## Recovery

- **A container left by an unclean stop** (a closed window, a hard kill, a crash,
  a reboot mid-job) is handled when the agent next prepares its pool, using each
  entry's own name pattern and the immutable diagnostic label. Ordinary containers
  are removed before the slots start. Labelled containers are exported and removed
  while the entry's diagnostics are on; with them off they refuse the entry until
  [Recover before disabling diagnostics](./windows-diagnostics.md#recover-before-disabling-diagnostics) is
  done. By hand, `docker ps -a --filter "name=<name>-"` lists candidates and
  `docker rm -f <name>` removes an ordinary one.
- **Docker Desktop restarts mid-job:** the container is gone, the job fails on
  GitHub's side, and re-running the workflow is the recovery. The agent keeps
  running: each slot's next attempt fails into its backoff while Docker does not
  answer, remeasures affinity, and resumes by itself when Docker is back.
- **The pools are down after a reboot or a sign-out:** unless `start_at_login`
  is on in `[lifecycle]`, nothing starts Marshal at sign-in. Start it from the Start menu once Docker Desktop is up, or straight
  away, since the runners wait for Docker.
- **Marshal does not answer, or a stop does not end:** the window says the agent is
  not answering, or keeps saying it is stopping. End `marshal-app.exe` in Task
  Manager; the next start removes the containers it left.
- **One pool's entry will not start:** a missing token, a
  volume that cannot be created, or stale containers that cannot be listed or that
  carry the diagnostic label with diagnostics off skip that entry only. The agent
  logs a warning naming the entry and the reason (never a token), the window shows
  the pool's reason, and the other startable pools run, including a sibling for the
  same repository. A pool waiting for its token starts once the token is set in the
  Tokens tab. Shared dependencies, such as that repository's token or the Docker
  daemon, can affect several pools. When no entry can start, the runners stay
  stopped with that reason. Fix the cause, then choose **Stop all runners** and
  **Start all runners** in an authorized quiet window; an entry skipped at start is
  not retried until the pools are prepared again.
- **A rejected configuration:** the window names every offending key and keeps the
  last valid configuration in use, or stays stopped when there is none. Fix the
  file, then choose **Start all runners**.
- **An image that cannot run jobs** (missing, or with another contract): the
  runners stay stopped, or stop, with that reason. Choose **Build image**, then
  **Start all runners**.
- **The restart limit:** a fourth failure of the pools' runtime within 10 minutes
  stops the runners, with the limit as the reason. Read the agent's log, fix the
  cause, then choose **Start all runners**.
- **A slot logging CPU discovery/cleanup failures:** registration is withheld.
  Check the intended Docker context/local image and available CPUs in a separately
  authorized maintenance window. Resolve the exact marked slot probe if deletion
  cannot be confirmed; never remove unrelated containers or prune broadly.
  A topology or readback mismatch must not be bypassed with unrestricted startup.
- **A slot logging registration failures:** the message carries GitHub's status. A
  401 or 404 usually means the token expired or lacks Administration access to that
  repository; replace it with **Replace Token** in the Tokens tab, as
  [The Fine-Grained Token](./registration.md#the-fine-grained-token) describes. For an organisation-owned repository,
  the token may instead await an owner's approval or break the organisation's
  policy; check [The Fine-Grained Token](./registration.md#the-fine-grained-token) before replacing it. Other slots
  continue their own registration loops, but a shared-token problem can affect
  every pool for that repository.

- **The window says job names are hidden** while the token has Actions: read: the
  agent's log records GitHub's answer to the workflow-runs request, its status code,
  its message and its `X-Accepted-GitHub-Permissions` header. A 403 there means the
  token still lacks the permission for that repository; check the token's
  **Repository access** and **Actions** setting as
  [Adding Actions: read for Job Names](./registration.md#adding-actions-read-for-job-names) describes.
  Marshal asks again when a runner next becomes busy, at most once a minute.
- **The window says GitHub status is unavailable:** the running agent is an older one
  without GitHub status, or, for now, has no room for another subscription and the
  window asks again every 60 s. Runner states on this computer are unaffected.

## Rolling Back

For a recovery CLI release, use the rollback procedure in
[recovery CLI installation and rollback](../runner-recovery/install.md#installing-updating-and-rolling-back), not the app procedure below.

Roll back when an app release misbehaves on the host. Rolling back means uninstalling the
installed release and installing the previous one: it needs an earlier release to go
back to, so a host on the first release has none, and a fault in it is fixed forward
in a new release. Pick a quiet time, because quitting Marshal stops its runners and
a job in flight fails.

1. Select the target release and verify that it supports this host's profile and
   configuration before uninstalling anything. If no compatible target is
   verified, keep the installed app. For a recovery CLI target, open
   `https://github.com/axross/runners/blob/v<version>/docs/operations/cli-recovery.md`
   now, replacing `<version>` with that published release's actual version without
   the `v`. Use that tag’s install procedure.
   A retained diagnostic container survives the installation. If the target may handle
   it differently, [recover the marked containers](./windows-diagnostics.md#recover-before-disabling-diagnostics)
   with the installed release before uninstalling it.
2. Follow the Windows procedure in
   [CLI](../marshal-cli/install-on-windows.md#updating-repairing-uninstalling-and-rolling-back): remove the tokens
   in the window only if you will not install Marshal again, quit Marshal, uninstall,
   then install the selected release's MSI, because the MSI refuses a lower version
   while a newer one is installed. For a recovery CLI target, install with the
   target-tag guide you opened in step 1, then follow the runtime procedures in
   the fixed historical tree that guide identifies.
   Keep the token files until
   the command line's runners work; the command line reads them, and the app's
   tokens stay in Credential Manager.
3. Start the release you installed, rebuild the image and start the runners. The
   build uses the image definition embedded in the installed release, so it gives
   back the image that release expects; an image of another contract keeps the
   runners from running jobs, as [Recovery](#recovery) says.
4. Confirm that **Settings, Apps** lists the version you installed, and that the
   [Health Checks](./windows-running.md#health-checks) hold for GitHub and the machine.
