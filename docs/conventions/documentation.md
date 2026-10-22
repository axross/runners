# Documentation

How this project's own documentation is kept true: where a fact goes, what a
change owes the documents it invalidated, and how a new document becomes
reachable at all.

The general practice is not restated here. The installed
[`living-project-documentation`](../../.claude/skills/living-project-documentation/SKILL.md)
capability owns the shape of `docs/`, the format each document takes, and the
validators; load it before writing or restructuring anything under `docs/`.
This project keeps no decision log: a settled constraint's rationale lives
inline in the convention or operations document that governs it, in the present
tense, never in a record of its own; see that capability's own Settled Decisions
section. What follows is only this project's own answer within what the
capability still owns, which is the part nothing outside this repository can
know.

## A Document Is Corrected by the Change That Made It Wrong

A change that alters a behavior, a convention, or a procedure MUST correct the
document describing it in the same change, never as follow-up work. Documentation
that is only ever written accumulates claims nobody re-checks, and a reader
cannot tell which of them still hold; the correction is what makes the rest of
the tree worth trusting.

The plan for such a change SHOULD name which document it will invalidate, so the
correction is scoped work rather than something remembered at review time. A
change to a runner image, a host script, a workflow, or a hook invalidates the
operations or convention document that describes it, and a claim about runner
behavior is either verified or described as planned, as
[REVIEW.md](../../REVIEW.md)'s documentation lens requires.

## A New Document Earns an Index Entry

Adding a document under `docs/` MUST add a one-line entry in
[`index.md`](../index.md) saying what the document covers, so a reader can
decide whether to open it. Renaming or removing a document MUST update its
index entry in the same change.

The documentation index and the harness's skill catalog provide discovery. The
one routing table this project keeps in [`AGENTS.md`](../../AGENTS.md), Routing
a Change, names the owner of each surface a change touches; a document that adds
a runner image, host, or agent-host surface adds its row there as well, and a
new topic with no such surface does not grow a second index.

## Every Document Stands on Its Own

Every document under `docs/` MUST stand on its own: the behavior, the rule, the
procedure, or the cost it describes MUST be stated in full in that document,
never left for a reader to recover by opening a plan, a commit, or another
document's history. A document that only made sense after following a link would
fail on its own terms. This is also why an operational document states the costs
it asks a reader to accept inline, rather than pointing the reader elsewhere to
find out what they are agreeing to. A rejected alternative that would otherwise
leave no trace in the tree is written into the governing document as a
constraint of its own, covering what this project does not do and why, so a
later reader meets it by reading rather than by searching history for a word
they do not have; see the installed
[`living-project-documentation`](../../.claude/skills/living-project-documentation/SKILL.md)
capability's Settled Decisions section for the full rule.

## Checking a Documentation Change

A documentation change is not finished until its checks pass. Run, from the
repository root:

```bash
mise run check:links
mise run check:docs
```

`check:links` resolves every relative link in `docs/`, `images/`, `hosts/`,
`.claude/agents/`, and the root Markdown files, which a `docs/`-only sweep would
miss. `check:docs` runs the three `docs/` validators, which check what a reader
cannot see: a document `index.md` does not link, a spec with no matching
glossary heading, and a relative link within `docs/` that does not resolve.
The [README](../../README.md#commands) states that `mise run check` runs both
along with the rest of the gate.

Neither check resolves a `#anchor` within a link, so a heading rename that
strands an anchor passes both; the author of a change that renames or removes a
heading MUST search for links to it and repoint them. What the checks cannot see
stays a reviewer's job: whether a fact sits in the one document that owns it,
whether a document says anything the tree does not already say, and whether a
claim about runner behavior is verified.
