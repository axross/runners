# Agent Skills

Installing and refreshing the agent skills this project uses, and what to do when
one of them turns out to be wrong, silent, or missing for this repository.

Every skill under `.claude/skills/` is **installed**, not written here. All of
them come from the shared [axross/skills](https://github.com/axross/skills)
library and are copied in with the
[vercel-labs/skills](https://github.com/vercel-labs/skills) CLI, pinned by
[`skills-lock.json`](../../skills-lock.json). This project owns no skill of its
own; its conventions and operating procedures are the documents under
[`docs/`](../index.md). Two costs come with that choice. Refreshing needs Node and
network access, because `npx skills` fetches from the library over the network;
the installed skills themselves are plain Markdown, so this cost falls on
refreshing, not on every session. And the library is not this project's own: a
rule that turns out wrong, outdated, or silent on a case here cannot be fixed by
editing the installed copy, because the next install discards the edit while it
poses as a rule the library agrees with. See
[Deviations and Gaps](#deviations-and-gaps) for how that is handled instead.

The installed set is exactly the keys of `skills-lock.json`; that file, not this
document, is the inventory. The runtime is Claude Code (`--agent claude-code`); Amp reads the same
`.claude/skills/` directory.

## Install and Refresh

```bash
# refresh exactly the skills this project already manages
npx skills add axross/skills --agent claude-code --yes --copy \
  $(node -p "Object.keys(require('./skills-lock.json').skills).map(s => '--skill ' + s).join(' ')")
```

**Do not use `--skill '*'` here.** Against an external source it installs the
library's _entire_ catalogue, not the subset in `skills-lock.json`. The command
above derives the list from the lockfile instead, so it stays correct as the set
changes.

Adopting a new skill means naming it explicitly, and `--skill` takes exactly one
skill per flag: repeat the flag (`--skill a --skill b`) rather than passing a
comma-separated list. A comma-separated value matches nothing, installs nothing,
writes no lockfile, and reports an available-skill list that reads like ordinary
help rather than a failure, so a refresh can appear to succeed while doing
nothing at all.

`npx skills` can also fail to resolve the CLI in a fresh container or against a
stale npx cache, aborting with `npm error could not determine executable to run`.
Retry that one case with an explicit specifier, `npx --yes skills@latest add ...`,
rather than pinning `@latest` on every run.

Every directory under `.claude/skills/` MUST be treated as a generated artifact.
Editing one is pointless, since the next install discards it, so a change to a
skill goes upstream to the library as an issue or pull request there. The
regenerated skill directories and `skills-lock.json` MUST be committed together,
and a skill MUST NOT be added to `.claude/skills/` while it is absent from
`skills-lock.json`: the lockfile describes the directory's entire contents, and
that correspondence is what makes drift detectable at all.

### Checking for Drift

Two checks cover two different failures:

- `mise run check:skills` runs in CI and locally. It fails when
  `skills-lock.json` and `.claude/skills/` list different skills. It cannot see a
  hand-edit inside an installed copy.
- The installed `agent-skill-management` skill ships the check that can. From a
  clone of the library, run it with the library's `skills/` directory as the
  source root:

  ```bash
  node .claude/skills/agent-skill-management/scripts/check-installed-copies.mjs \
    <path-to-axross-skills-clone>/skills .claude/skills
  ```

  Every skill named in `skills-lock.json` MUST report `OK`. The command also
  reports `DRIFT ... no installed copy` for each library skill this project has
  not adopted; that is expected, because the project installs a subset, and it is
  not a fault. It needs a checkout of the library, so it is run by hand when
  refreshing or diagnosing, not in CI.

Installing a skill does not prove a host loaded it. In Claude Code, confirm the
selected source and new content in a **fresh** session with `/context` and a
skill load; the session that changed the tree read its skills at startup, and a
subagent spawned from it does not substitute for a fresh host session. In Amp,
record the intended source revision and a distinguishing passage from each
changed skill before running `reload_skills`. Then load those skills and compare
their returned source and body with the expected passages from this refresh.
A matching installed directory or discovery entry alone proves neither active
loading nor compliance. Report unavailable source, reload, or body evidence as
unavailable rather than substituting a shell inventory for active loading.

### When Upstream Renames a Skill

The refresh command above breaks on a rename rather than absorbing it: the
lockfile still holds the old name, so the `--skill` list it derives asks the
library for a name that no longer resolves, and the run fails on that one name
instead of refreshing anything. Run the install once by hand in that case, naming
every surviving skill plus the new name explicitly:

```bash
npx skills add axross/skills --agent claude-code --yes --copy \
  --skill <surviving-skill> --skill <surviving-skill> --skill <new-name>
```

Remove the stale skill with `npx skills remove <old-name>` rather than deleting
its directory by hand; the CLI is what rewrites `skills-lock.json`. The rename is
not finished at the lockfile: every repository-side reference to the old name,
such as a task in `mise.toml` that runs a skill's own scripts by path, and any
prose that names it, is carried in the same change.

## Discovery Metadata

Every installed skill front-loads its trigger in `description`, which is the one
field every host reads. `user-invocable` is a Claude Code frontmatter extension,
and its companion `when_to_use` is deliberately absent from the installed skills:
`when_to_use` is not part of the Agent Skills specification, so a trigger placed
only there would be invisible on another host. A skill whose trigger has to be
findable MUST carry it in `description` rather than in a host-specific field.

## Deviations and Gaps

Two different things route here, and they resolve the same way. A **deviation**
is a collision: an installed capability requires one thing, this project
proposes or has adopted a bounded alternative. A **gap** is an installed capability being wrong,
outdated, or simply silent on a case that comes up here, or a capability the
library does not have at all. Either way the installed skill is left exactly as
it is, and the resolution is written down in this document.

That matters because an unrecorded deviation reads to the next agent, and to a
reviewer, as a plain violation of a MUST rule, and an unrecorded gap gets
rediscovered from scratch by whoever hits it next.

A suspected gap MUST be verified against the actual host-selected skill's text
and source before being routed anywhere; a rule that already covers the case is
not a defect to file. When that source cannot be established, report routing as
blocked rather than infer it from this project's lockfile. A real gap is then
resolved by one or both of two routes: an issue opened on
[`axross/skills`](https://github.com/axross/skills) when the gap generalizes
beyond this project, and a written note in the register below saying what the
capability states, what this project does instead, and how to handle the case
meanwhile. The human's go-ahead MUST be obtained before opening an upstream
issue or posting feedback on an existing one, since it is a public write beyond
this repository's delivery grant, and
the gap MUST be recorded locally in the meantime rather than leaving the finding
to depend on that issue landing.

An additive, nonconflicting local convention is not a deviation. A conflicting
choice is only valid after human adoption under
[Agent Skill Management](../../.claude/skills/agent-skill-management/SKILL.md)'s
installed-rule change contract. This project's register MUST retain that
contract's required exception and adoption evidence. The contract owns validity
and reassessment; this register owns their local record, not permission to
override a host restriction or publish upstream.

The exposing task continues under applicable installed rules and any separately
valid deviation. A pending upstream proposal never puts its proposed rule into
force; if applicable boundaries prevent continuing, report the blocker. Any
upstream request filed or left pending SHOULD be named in the completion report.

## The Register

### Gap - the library has no Docker, shell, or GitHub Actions workflow skill

This repository's main artifacts are Dockerfiles, shell and PowerShell scripts,
and GitHub Actions workflows. `axross/skills` has no skill for any of them, so no
installed capability states how to write or review one. What governs them
instead:

- the pinned linters in [`mise.toml`](../../mise.toml), which enforce
  well-formedness (hadolint, shellcheck, PSScriptAnalyzer, actionlint);
- [Security](../conventions/security.md), which states the supply-chain and
  runner trust-boundary rules;
- the language lenses in [REVIEW.md](../../REVIEW.md), which the independent
  reviewer applies;
- `application-security` and `code-maintainability` for the generic rules they
  already state.

A session that meets a question none of these answers takes it to the maintainer
rather than inventing a convention. **Upstream status:** not filed. The
maintainer has not yet given the go-ahead for an issue on `axross/skills`; the
completion report of any change that touches one of these surfaces names the gap
as pending.
