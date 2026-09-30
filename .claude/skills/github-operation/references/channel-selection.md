# Channel Selection

Apply this reference when qualifying a GitHub channel or handling a failure. A route is qualified for a particular operation in the current session, not for every operation on a named host.

## Qualify an Authenticated Route

The sanctioned channel is the harness's GitHub tool channel: for example, GitHub MCP in a configured Claude Code session. Neither the name Amp nor the absence of MCP establishes what other routes work. A permitted alternative is an authenticated, high-level route the session already provides, such as a GitHub CLI operation.

Two channel properties can justify an alternative:

- **Absent:** the session exposes no GitHub tool channel.
- **Functionally limited:** the channel exposes the operation but cannot normally complete or verify it faithfully, such as a body read that removes HTML comments.

An authentication failure, timeout, rate limit, or 5xx is neither property. It is a failed invocation, not permission to switch credentials. A channel that exposes no write operation is also different from an absent channel: its missing operation is not an escape hatch.

**Guidelines:**

- MAY select another authenticated, high-level route only when the sanctioned channel is absent or has an established normal-operation limitation for the requested operation.
- MUST establish the alternative's presence and authenticated identity without printing credentials; keep tokens out of commands, logs, and output.
- MUST qualify the route's operation, target, required fidelity, and verification capability under the current host restrictions and least permission needed.
- MUST NOT use an alternative to reach an operation a present sanctioned channel does not expose, or to bypass a prohibited purpose.
- MUST NOT treat a transient invocation failure as grounds to switch routes or credentials; report the failure and apply [publication-and-recovery.md](./publication-and-recovery.md) if an effect may have occurred.
- MUST report the current session's observed capability or limitation, not a universal claim about a host, cloud environment, or private repository.

## Keep Raw Operations Default-Deny

A raw REST or GraphQL call is not authorized merely because a high-level command is unavailable. Some harnesses normally restrict GraphQL operations while serving REST reads. Establish that functional limitation rather than relabeling any high-level error as one.

The following consequences keep a raw operation denied:

| Property                     | Examples                                                               |
| ---------------------------- | ---------------------------------------------------------------------- |
| Irreversible                 | Merging a pull request; deleting or force-updating a ref               |
| Silently wrong               | Replacing a whole label list; replacing a body from an unverified read |
| Privilege- or gate-affecting | APPROVE or REQUEST_CHANGES; collaborator permissions; Actions secrets  |
| Outward-facing or costly     | Dispatching a workflow; publishing a release                           |

**Guidelines:**

- MUST treat raw routes as default-deny: an operation is eligible only when it has none of these consequences, serves something the sanctioned channel cannot serve, and satisfies every other access and authorization rule.
- MUST classify unlisted operations by those consequences, not by whether the table names them.
- MUST NOT infer write permission from a raw route that supplies stored bytes for reading.
- MUST NOT substitute ordinary comments for findings required on diff lines. A top-level COMMENT review is an alternative only when inline findings are not required; otherwise report the unavailable review operation.
