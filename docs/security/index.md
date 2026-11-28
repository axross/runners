# Security boundaries

Use the boundary a proposed change would cross. Host setup and recovery procedures live in [Runner hosts](../runner-hosts/index.md).

- [Which tools and dependencies require an immutable pin or explicit review?](./dependency-pins.md)
- [Where may CI run, and what permission can each workflow receive?](./workflow-permissions.md)
- [Who can dispatch, approve, sign and publish a release?](./release-authority.md)
- [Which pools may share credentials, networks and persistent storage?](./pool-isolation.md)
- [Which process holds credentials and diagnostic evidence?](./local-agent.md)
- [What may the app display, execute or download?](./app-trust.md)
- [What can a job VM reach on axmc?](./vm-isolation.md)
- [Which values can appear in source code and logs?](./disclosure.md)

The [security topic finder](../conventions/security.md) also provides direct routes by established section name.
