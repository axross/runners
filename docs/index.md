---
okf_version: "0.3"
title: Runners engineering
description: Operating a self-hosted runner, understanding Marshal, and delivering changes to runners.
---

<a id="documentation"></a>

# Runners engineering

<a id="glossary"></a>

Marshal runs GitHub Actions jobs on Windows hosts with Docker Desktop and on
Apple silicon Macs with Tart. Start with the job you are doing. The
[glossary](./glossary.md) explains names used across the host, app, and library.

<a id="operations"></a>

## Run or recover a host

- [Run a Windows host](./runner-hosts/index.md#windows-with-docker-desktop) to configure pools, start runners, and recover a failed installation.
- [Run a Mac host](./runner-hosts/index.md#macos-with-tart) to prepare Tart and Softnet, build a golden VM, and diagnose a job VM.
- [Recover the historical CLI](./runner-recovery/index.md) for the bounded release procedure, its evidence requirements, and the conditions for restoring app publication.

## Understand Marshal

- [Marshal](./marshal/index.md) covers the desktop app, its session with the resident agent, and platform-specific installation and behavior.
- [marshal-cli](./marshal-cli/index.md) explains the library's configuration and protocol contracts, how it runs jobs, and how it handles failure.
- [Security](./security/index.md) covers trust boundaries, credentials, workflow permissions, release authority, and accepted risks.

<a id="conventions"></a>

## Change or release the project

- [Contributing](./contributing/index.md) covers development setup, project layout, testing, and where to document a change.
- [Agent work and change delivery](./agent-work/index.md) covers planning, authorization, sessions, review, waiting for checks, and resuming interrupted work.
- [Releases](./releases/index.md) covers runner updates, versioning, signing, publication, and recovery from a partial release.

Requirements in these guides use MUST, MUST NOT, SHOULD, SHOULD NOT, and MAY
as defined by [RFC 2119](https://www.rfc-editor.org/rfc/rfc2119.html).

The [repository README](../README.md) describes which parts of the project
have shipped. For links from prior revisions, see the
[earlier operations guides](./operations/index.md).
