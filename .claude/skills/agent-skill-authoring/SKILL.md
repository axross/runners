---
name: agent-skill-authoring
description: Creating, refining, splitting, consolidating, renaming, or auditing an agent skill — portable content boundaries and agentskills.io authoring. Triggers on `SKILL.md`, frontmatter, `description`, "where does this rule belong", "split this skill", "recast this skill as a capability", and "audit skill portability". Skill management owns installation and active loading; project policy and host guidance own gates and execution. Covers capability framing, judgment versus mechanism, standard metadata versus host extensions, conditional references, a portability checklist, and structural validators.
user-invocable: false
---

# Agent Skill Authoring

Use this capability whenever you create, refine, split, consolidate, rename, or audit an agent skill under the host project's skill root. It is what turns a durable convention into a well-formed, discoverable skill and keeps the skill tree coherent as it grows.

Skills authored here follow the agentskills.io format. For the host project's active skill inventory and topic-to-skill routing, defer to each skill's own `description` discovery metadata and the directory listing under the skill root; where a host also maintains a written index (e.g. `AGENTS.md`), keep it in sync too.

**Guidelines:**

- MUST run the bundled validator that owns what a change touched — `scripts/check-skill-frontmatter.mjs` after editing frontmatter, `scripts/check-skill-body.mjs` after editing prose, `scripts/check-skill-references.mjs` after adding, moving, or renaming a reference file — and all three when unsure; together they are the enforcement path for the frontmatter, naming, discovery length-cap, and reference-linkage rules this skill states nowhere else (see [audit-checklist.md](./references/audit-checklist.md)).
- SHOULD propose or implement a skill update when any task exposes a reusable convention, outdated guidance, a recurring review issue, or a missing project rule — skill maintenance happens when work reveals durable learning, not after every narrow fix.
- SHOULD skip skill maintenance when the work produced no generalizable learning, and state that it was skipped.

The key words "MUST", "MUST NOT", "REQUIRED", "SHALL", "SHALL NOT", "SHOULD", "SHOULD NOT", "RECOMMENDED", "MAY", and "OPTIONAL" in this document are to be interpreted as described in [RFC 2119](https://www.rfc-editor.org/rfc/rfc2119.html).

## Scoping and MECE

See [scoping-and-mece.md](./references/scoping-and-mece.md) for:

- choosing a coherent skill boundary, skill name, split, consolidation, or source-of-truth location
- checking overlap with neighboring skills before adding new guidance
- separating portable judgment and workflow meaning from project policy, host execution, and skill distribution
- classifying mixed paragraphs and references before migrating their detailed rules
- classifying every section as mechanism or judgment when a vendor- or runner-specific skill sits beneath a tool-agnostic owner
- using section length and topic growth as signals for restructuring

**Guidelines:**

- MUST read [scoping-and-mece.md](./references/scoping-and-mece.md) before assigning a rule's owner or migrating content between capabilities, project policy, and host guidance.

## Capability Framing

See [capability-framing.md](./references/capability-framing.md) for:

- framing a skill as an ability the agent gains rather than a document it reads
- naming the activity a skill enables, and the document-style suffixes to avoid
- the voice of the `description` opening clause, the H1, and the opening paragraph
- recasting an existing guideline-style skill in a fixed order, without changing what it requires

## Frontmatter and Naming

See [frontmatter-and-naming.md](./references/frontmatter-and-naming.md) for:

- creating or editing discovery-critical `SKILL.md` frontmatter
- distinguishing standard required and optional metadata from host-specific invocation controls
- choosing the skill directory name and keeping it aligned with the `name` field
- assessing host-project harness fields against the selected host and distribution path

**Guidelines:**

- MUST read [frontmatter-and-naming.md](./references/frontmatter-and-naming.md) before editing metadata or judging whether a host extension is required or supported.

## Description Writing

See [description-writing.md](./references/description-writing.md) for:

- drafting, trimming, or auditing the `description` field against its byte cap
- the four-slot contract — trigger, identity, boundary, coverage — and why only the last may be sacrificed to a host's listing truncation
- fusing the trigger and the identity into one opening clause, and the two half-clauses that fail on their own: a bare token list, and "The ability to …"
- stating the surfaces a skill refuses to serve, alongside the hand-offs that route a competing skill away
- adding likely user phrasings and symptom-based triggers without over-broadening the skill
- the ~640-byte corpus mean the target names, and why it is a centre of gravity rather than a per-skill ceiling

## Body Content Style

See [body-content-style.md](./references/body-content-style.md) for:

- writing or revising substantive skill-body or reference-file sections
- balancing concise topic explanation, examples, and guideline bullets
- citing the upstream documentation URL in a section that pins a version or mirrors a vendor's option surface
- placing normative RFC-2119 requirement bullets in detailed reference content rather than parent routing sections

## Progressive Disclosure

A `SKILL.md` carries exactly three things: what capability this is, the rules that apply unconditionally within it, and the routing that says which reference to read when. Every other normative statement — the detail behind a MUST or SHOULD, procedures, tables, examples — goes in a reference. The test for keeping a rule in the body is whether the reader needs it **before the routing decision**, not whether the rule is important: a change-loop capability's distinction between read-only work and a change must be available before the reader chooses an implementation reference. This contract is itself needed before the routing decision it governs, which is why it is stated here rather than left behind the pointer below — the same reason the load-bearing test just after it is stated directly rather than deferred.

A rule is **load-bearing** when an agent that loads `SKILL.md` and opens no reference would produce wrong output for want of it — held before the work starts, not looked up once the reader already knows the question exists. That test still sorts a skill's material, but its consequence is a conditional read obligation rather than a relocation: a load-bearing rule's own statement stays in its reference, and `SKILL.md` carries an RFC-2119 obligation to read that reference before the work its rule governs. A reference nobody is told to read never gets read; a `SKILL.md` that states every rule directly cannot be tree-shaken by the sessions that will never touch most of them. This test is itself load-bearing for a skill's author, so it is stated here as a rule rather than left behind the pointer below.

**Guidelines:**

- MUST treat a rule as load-bearing when an agent that loads `SKILL.md` alone would produce wrong output for want of it — a fixed order, a closed set, or a constraint whose violation is not self-evident from the output — and as elaboration otherwise.
- MUST place a load-bearing rule's own statement, its RFC-2119 bullets, and everything that elaborates it in the reference file that governs it, not in `SKILL.md`.
- MUST give `SKILL.md` a `**Guidelines:**` block, placed after a reference's routing list, carrying one RFC-2119 bullet per reference that names the reference and states the condition — narrow enough to be skippable — under which it MUST be read.
- MUST NOT restate a load-bearing rule's statement or its RFC-2119 bullets in `SKILL.md` once its reference states them, except under the carve-out below.
- MUST keep a rule's own statement in `SKILL.md`, never moved to a reference, when the rule's triggering condition is unconditional within its own skill's scope — a pointer that would fire on every turn costs a read and shakes nothing.

See [progressive-disclosure.md](./references/progressive-disclosure.md) for:

- deciding when a skill should stay single-file or split into `references/`
- the three-part role contract and the "needed before the routing decision" test in full, and how the load-bearing test and the unconditional-scope carve-out relate to it
- the load-bearing test's full sorting table, and how it decides whether a reference earns a conditional read obligation
- the size thresholds that signal a skill or reference file has grown too large
- using the parent routing-section format: `## Topic`, `See [file.md](./references/file.md) for:`, descriptive situation bullets, then a `**Guidelines:**` block carrying the read obligation
- wording a read obligation's triggering condition narrowly enough to be skippable
- stating the fact a routing bullet points at — the flag, limit, or rule by name — instead of announcing that one exists
- keeping parent routing bullets free of RFC-2119-style requirement keywords so they remain routing cues, not duplicated rules
- the two placements a body-resident rule may take relative to a routing list, and when each applies

**Guidelines:**

- MUST read [progressive-disclosure.md](./references/progressive-disclosure.md) before splitting a skill into `references/`, restructuring its reference layout, or deciding where a body-resident rule belongs relative to a routing list.

## Cross-Referencing and Discovery

See [cross-referencing.md](./references/cross-referencing.md) for:

- adding, renaming, moving, deleting, or linking skills and reference files
- choosing one source of truth instead of copying detailed rules across skills
- using topic-based cross-skill references, verifying intra-skill relative links, and keeping skill discovery current (plus any written index a host maintains)
- routing only to applicable owners and hosts, including when an optional owner or project host document is absent

**Guidelines:**

- MUST read [cross-referencing.md](./references/cross-referencing.md) before adding or changing a cross-owner route.

## Project Skill Archetypes

See [project-skill-archetypes.md](./references/project-skill-archetypes.md) for:

- creating the project-specific skills a scaffolding pass calls for: structure, component, and UI/design
- the three-way ownership triangle and each archetype's skeleton, topics checklist, or table patterns
- growing archetype skeletons with worked examples and mechanical boundary checks

## Auditing and Validation

See [audit-checklist.md](./references/audit-checklist.md) for:

- auditing multiple skills or reporting skill-tree quality
- running the three bundled structure validators — `check-skill-frontmatter.mjs`, `check-skill-body.mjs`, and `check-skill-references.mjs` — and which kind of edit each one answers for
- running the bundled link-freshness audit (`scripts/link-freshness/check.mjs`) on a schedule to catch a cited vendor URL that has gone 404, why only a confirmed-dead link fails it, and why a `pull_request` trigger on it is a request-forgery primitive
- checking inventory, skill discovery, section anatomy, RFC-2119 bullets, topic-based cross-skill references, and relative links
- identifying overlap, stale assumptions, orphan references, and missing source-of-truth links
- applying the ownership and portability checklist without expanding a bounded change into a library-wide migration

**Guidelines:**

- MUST read [audit-checklist.md](./references/audit-checklist.md) when reviewing a skill's ownership boundaries or metadata portability.
