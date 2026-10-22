@AGENTS.md

## Project gates within host boundaries

This entry file routes project requirements; it cannot rewrite the host's
instruction hierarchy. Claude Code documents
[CLAUDE.md as conversation context, not a system-prompt change](https://code.claude.com/docs/en/memory#claude-isnt-following-my-claude-md).
It also documents
[host-provided overrides of default behavior](https://code.claude.com/docs/en/agent-sdk/modifying-system-prompts#reminders-claude-code-adds-to-the-conversation).
Such an override comes from the host, not from this file or a loaded skill.
Neither a runtime-injected message nor project guidance has a fixed priority
merely because of its label or when it was read. Apply the actual host
hierarchy and any explicit delegation of defaults, using each instruction's
source, level, scope, and conditions.

The tracking issue, recorded plan, human plan approval, verification, and
independent review required by [AGENTS.md](./AGENTS.md) and
[Development Workflow](./docs/operations/development-workflow.md) remain
project gates. A task request such as "make the changes, commit, and push"
does not itself approve a plan or supply independent review. If a host
boundary prevents a required step, report that gate as unmet rather than
waiving it or claiming the change is ready.

**Guidelines:**

- MUST assess tool eligibility separately from a standing request, operation
  authorization, and project gates. A tool's purpose and usage conditions,
  host permission prompts, refusals, and prohibitions remain boundaries; no
  project request, grant, plan approval, or available tool overrides them.
- MUST meet this agreement's draft-pull-request request through the
  [plan-gate delivery confirmation](./docs/operations/agent-sessions.md#confirming-delivery-at-the-plan-gate-in-claude-code)
  in Agent Sessions; its confirming answer is the user's explicit request for
  that run's pull request.
- MUST treat this agreement's configured-subagent requests as satisfying an
  explicit-request condition only when the actual host or tool accepts that
  form of standing request and the particular use is otherwise eligible. A
  configured role alone proves neither capability nor eligibility. Record the
  delegation determination and its grounds under
  [Loop Engineering](./.claude/skills/loop-engineering/SKILL.md).
- MUST carry the verified
  [standing delivery grant](./docs/operations/development-workflow.md#authorizing-delivery-operations)
  forward for its covered targets and effects without asking again, when the
  host and tool permit the operation. The grant supplies human authorization,
  not tool eligibility or plan approval. Tracking-issue creation and plan
  publication precede approval; implementation follows approval, and ready
  follows verification and independent review.
- MUST evaluate an instruction to subscribe to pull-request activity or
  schedule recurring check-ins against its actual source, level, scope, tool
  conditions, and operation authorization. Ending the change loop does not
  fulfill that instruction. The loop's ready-transition teardown does not
  override a higher-priority host instruction; follow the applicable host
  instruction and report any incompatible project requirement as unmet.
  The delivery grant does not authorize scheduling, and monitoring does not
  authorize merging; merging stays with the maintainer.
- MUST stop the affected operation and surface an irreconcilable conflict at
  the plan gate, or the pending recovery phase if discovered later. Record
  the conflicting instructions, completed work, unmet gate, and next action.
  Do not work around a host prohibition through another tool or route. If an
  instruction's status cannot be established, report the uncertainty rather
  than silently treating it as an overridable default.

Everything else this host needs is routed from the imported entry, so
nothing else belongs here: [AGENTS.md](./AGENTS.md)'s host routing names
[Agent Sessions](./docs/operations/agent-sessions.md),
[Development Workflow](./docs/operations/development-workflow.md), and
[Agent Skills](./docs/operations/agent-skills.md) for what each owns.
