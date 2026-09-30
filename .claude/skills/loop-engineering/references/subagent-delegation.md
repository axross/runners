# Direct and Delegated Execution

Apply this reference when choosing who or what executes an assignment. The loop specifies contracts and phase meaning; the host supplies execution mechanisms and their permission boundaries.

## Executor Selection

Delegation to a subagent is the default choice for change-work execution. Direct parent execution is the fallback, valid only where delegation is unavailable through currently permitted tools or disallowed by host policy. That much — use only what the session permits, and execute in the parent only in that fallback case — is stated in [SKILL.md](../SKILL.md) under its carve-out and is not restated here; what follows is what applies once delegation is actually on the table.

**Guidelines:**

- MUST NOT require a universal actor ranking, role name, model, tool, separate checkout, live conversation, or resumable child.
- MUST report missing capability as `unavailable` and missing authorization as `authorization-waiting`.

## Read Routing

Not every read belongs in the parent's own context, and size alone does not decide which. Some reads are the object of the judgment about to be made: the exact wording is what the judgment is about, and no summary of it does the same work. Others are inputs to a judgment about something else — a large payload one conclusion is wanted from, where carrying the whole payload into the parent's context spends exactly what routing the read away would have saved. A third case is neither, because fidelity is the requirement while only part of the payload is wanted, and a summarizing intermediary cannot serve that at all.

| The read                                                   | Where it goes             | Examples                                                                                                              |
| ---------------------------------------------------------- | ------------------------- | --------------------------------------------------------------------------------------------------------------------- |
| the object of the judgment being made                      | the parent's own context  | the diff under review, two plan revisions being compared, a body about to be written back, the rules the parent obeys |
| large material wanted for one conclusion, not its own text | an investigator           | logs, a long thread, a wide search across files or history, a file tree being located in                              |
| fidelity required, but only part of the payload wanted     | narrowed at the tool call | one failing assertion line, field selection on a structured response, a bounded line range of a file                  |

**Guidelines:**

- MUST keep in the parent's own context any read whose exact text is the object of the judgment being made, along with the rules the parent is itself obeying.
- MUST route any other large payload wanted only for a conclusion to an investigator wherever a permitted one is available, and read it directly otherwise.
- MUST narrow the read at the tool call, rather than delegating it or reading it whole, wherever fidelity is the requirement but only part of the payload is wanted.

## The Investigator Return Contract

An investigator earns its place only by returning what the parent needs next: a conclusion, and a locator the parent can follow when the conclusion turns out not to be enough. Handing back the source text instead puts into the parent's context exactly what routing the read away existed to keep out, so it saves nothing. The output is the only channel back, which is why the output alone is what these rules constrain — nothing here bounds what the investigator reads to get there.

**Guidelines:**

- MUST give an investigation assignment a target it may widen if the answer requires it, one question answerable as an enumeration or a yes/no, and the shape its answer must take — a verdict, a list, or a bounded extract.
- MUST require a conclusion and a locator rather than the source text, admitting a quotation only where the conclusion cannot stand without one and bounded to what the conclusion needs.
- MUST NOT impose a read budget, a token cap, or a tool-call cap on an investigation; instruct it to read as much as settling the answer takes, stopping short only by reporting the material unresolved.
- MUST NOT let an investigation paste its source back, return a full summary, propose further investigation, or explore outside its target beyond what the question requires.

## Compatibility Preflight

An executor should fail before effects, not after them. Preflight checks the assignment's required reads, writes, commands, return path, and material fidelity against actual permitted capabilities.

**Guidelines:**

- MUST confirm access to every required material at its declared fidelity before execution; visual access means the image is actually rendered to the actor.
- MUST return a non-complete result before editing when source, revision, uncommitted diff, required material, permission, or return path does not match the assignment.

## Integration Boundary

A child's `complete` result closes only its assignment. The parent still owns phase progression and integrated evidence.

**Guidelines:**

- MUST compare the result, including its self-review material, policy, outcome, findings, and limitations, with actual changed files, uncommitted state, commits, checks, processes, and the approved revision before accepting it.
- MUST NOT accept missing, outdated, or mismatched required self-review evidence as clean; integration changes require current parent self-review evidence against the integrated diff.
- MUST inspect the integrated result rather than treating a receipt as proof that the whole change is ready.
- MUST issue a fresh assignment after a plan revision; a clarification that does not change the plan may continue under the current assignment.
