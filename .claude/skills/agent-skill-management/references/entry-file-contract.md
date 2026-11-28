# Agent entry-file contract

Use this reference when writing or auditing a project's agent entry file, the
always-loaded instruction file that routes an agent to the project's
capabilities. The entry file keeps what is specific to the project: where its
guidance lives, its values and the people it names. A loaded skill owns each
generic rule, so the entry file points to that rule and does not restate it.
This reference carries placeholders only; each project supplies its own values.

## What an entry file carries

An entry file carries six items. Items 2 to 6 have a canonical line, shown
below with placeholders for the project's own values; item 1 is project-specific
routing, so it has none. Items 3 to 5 read as one passage.

**Example:**

> 2a. Load `[<conduct capability>](<link>)` first in every top-level run, including read-only questions; load `[<development capability>](<link>)` for project work.
>
> 2b. MUST read `[README](<link>)` and the setup guide it links before project commands; read `[<doc index>](<link>)` when a task depends on project terminology, concepts or past decisions.
>
> 3. This repository's agent-comment marker is `<marker>`.
>
> 4. Its default branch is `<default branch>`, which agents never push to (`<delivery-defaults link>`);
>
> 5. merging is left to `<merging human>`.
>
> 6. This file is the maintainer's standing request that every change run delegate as `<delegation link>` sets out; the matrix grants no authorization.

The agent-comment marker is the fixed string that labels a comment an agent
wrote, and the delegation matrix is the change-loop capability's allocation of
work between the driver and its delegates. The `<conduct capability>` is the
project's baseline for handling uncertainty, asking and reporting, and the
`<development capability>` is its baseline for project work; a project without
one omits that clause.

**Guidelines:**

- MUST carry every item, except as the absent-owner bullet below provides: 1, routing to the project's capabilities by task; 2, the bootstrap order; 3, the agent-comment marker; 4, the default branch with the push rule as a deferring clause; 5, the merging human; 6, for change runs, the delegation standing request.
- MAY add project detail to a canonical line, and MUST keep each of its elements.
- MAY omit the conduct or development clause where that capability is not installed, as the prose above allows; MUST, where no change-loop capability is installed, state the default-branch rule plainly and omit the delegation standing request.
- MUST NOT otherwise restate in the entry file a rule that a loaded skill owns; route to the capability instead.
