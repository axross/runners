---
type: Architecture
title: Windows job container lifecycle
description: How a slot moves from CPU and image checks through registration, job execution and container cleanup.
---

# Windows job container lifecycle

## How a Container Picks Up a Job

For each execution pool, the agent runs that entry's `slots` as separate loops. A
sibling pool for the same repository has its own slots. Each slot follows this
cycle:

1. Check the image's contract, then remove the slot's leftover probe and
   discover/select current allowed CPUs before registration. A failure backs off
   without registering a runner.
2. Request a single-use registration from GitHub with that repository's token, for
   a uniquely named runner carrying the entry's labels. A real registration with
   GitHub is verified on axpc, each pool registering its platform and configured
   labels (maintainer's report,
   [#56](https://github.com/axross/runners/issues/56#issuecomment-6511344317)).
3. Start a throwaway container from the image with the entry's volumes and
   the affinity guard followed by `/home/runner/run.sh`. The registration reaches the runner through the
   `ACTIONS_RUNNER_INPUT_JITCONFIG` environment variable, which the runner reads
   at start in the pinned version (`CommandSettings.cs` in `actions/runner`).
   [The token-handling policy](../security/local-agent.md#tokens-stay-out-of-the-log-and-job-clients-end-with-start) governs how the agent hands it to Docker. Anyone with access to the local Docker daemon can still read it from
   `docker inspect` until the container is removed, and the registration is
   single-use.
4. The runner takes one matching job and exits. Normally `--rm` removes the
   container. A diagnostic entry's container exports its bounded evidence first,
   then is removed, also when the export fails.
   The slot then returns to step 1. Container exit is not the GitHub job result;
   a runner can exit successfully after reporting a failed consumer job.

A failed image check, CPU discovery, registration request or container that exits
non-zero delays the next attempt, doubling from 5 seconds to a 300-second ceiling;
a container that ran a job to completion is replaced at once. When the part of
the agent that runs the pools fails as a whole, it is started again after 10 s, 30 s,
and 60 s; a fourth failure within 10 minutes stops the runners, with the restart
limit as the reason the window shows. [CLI](../marshal-cli/pool-runtime.md#names-and-the-slot-loop)
gives the slot loop and its backoff, and [CLI](../marshal-cli/foreground-instance.md#instance-state) the
restarts.
