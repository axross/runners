# Agent work

Deliver a change, operate an agent session and maintain the skills it runs.

## Change delivery

- [Deliver a project change](./change-delivery.md) — Take an approved change through verification, review, handoff and issue closure.
- [Standing delivery grant](./delivery-grant.md) — Identify the exact authorized targets, effects, costs, exclusions and adoption evidence for agent delivery.
- [Branch naming and merging](./branches.md) — Choose a working branch, preserve published history and handle a nonconforming pull request head.
- [Apply host delivery permissions](./host-permissions.md) — Apply the standing grant on Amp and Codex and collect the required Claude Code and Amp session confirmations.
- [Out-of-tree acceptance evidence](./acceptance-evidence.md) — Decide when external evidence is required and who can approve changes to its method, destination or timing.
- [Checkpoint and resume a run](./run-state.md) — Publish recoverable state without altering the approved plan and identify the correct checkpoint when resuming.

## Review

- [Select and verify an independent review](./independent-review.md) — Select the provider for the authoring host and decide whether its posted result meets the review gate.
- [Request the Codex review](./codex-review.md) — Request the review for a Codex or manual change and qualify the outcome of the external service.
- [Run an Amp review round](./amp-review.md) — Start a fresh child thread, verify its review and end the thread under the host conditions.
- [Run a Claude Code review round](./claude-subagent-review.md) — Qualify an independent reviewer, isolate its worktree, verify its posted result and account for its ending.
- [Request the alternative Claude CI review](./claude-ci-review.md) — Request the workflow review only when selected by the maintainer and identify its eligible output.
- [Wait for review and CI](./wait-for-checks.md) — Qualify a host wait mechanism and determine when the run must report an unmet wait instead.

## Sessions

- [Start and provision an agent session](./session-setup.md) — Start Codex in the intended checkout and provision the toolchain used by Amp and Claude Code.
- [Agent quality hooks](./quality-hooks.md) — Determine what Amp and Claude Code check after edits and at turn end, including failure and skip conditions.
- [Ask for a maintainer decision](./maintainer-decisions.md) — Choose the available question mechanism, including decisions from launched sessions and subagents.
- [Assign investigations, implementation and review](./delegate.md) — Qualify the actor and assignment for the host, route evidence to it and record an unavailable role.
- [Verify a GitHub write from Claude Code cloud](./github-write-fidelity.md) — Compose and verify the stored text, recognize observed rewrites and recover within the permitted correction limits.
- [Claude Code telemetry and session settings](./session-settings.md) — Choose operator-specific session settings and distinguish them from project telemetry attribution.

## Installed skills

- [Install and refresh project skills](./skill-installation.md) — Reinstall the locked skill set, handle a rename and verify source agreement and host selection.
- [Skill gaps and adopted deviations](./skill-exceptions.md) — Identify the authority, scope, evidence and reassessment conditions for project-specific exceptions.
