---
type: Convention
title: "Placing and checking project documentation"
description: "Choose the owning subject for a change and check that readers can still find its decision, evidence and exceptions."
---

# Documentation

## Choose the owning subject

Start with the reader who needs to make a decision. Use the [documentation
index](../index.md) to find the subject. Add an answer to an existing page when
the reader needs it to make the same decision. Add a subject when readers need
to choose among several related decisions and the existing subjects cannot serve
them by linking to one owner.

| Reader's question                                                          | Owning subject and boundary                                                                                                                                                               |
| -------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| What can I do or see in the desktop app?                                   | [Marshal](../marshal/index.md) owns windows, controls, events and the installed app. Its client follows the resident agent protocol.                                                      |
| What will a library call or agent message accept, return or refuse?        | [Marshal CLI and agent](../marshal-cli/index.md) owns configuration, protocol, runtime state and command behavior. Put the operator’s platform-specific steps under runner hosts.         |
| How do I bring a machine up, verify it or recover it?                      | [Runner hosts](../runner-hosts/index.md) owns host-specific prerequisites, commands and recovery. A release publisher has different permissions and evidence.                             |
| How does an agent session plan, verify and deliver a change?               | [Agent work](../agent-work/index.md) owns session and delivery rules. An operator's provisioned runner host is a separate subject.                                                        |
| How does a maintainer produce or restore a release?                        | [Releases](../releases/index.md) owns release steps, failure recovery and publication. Follow the [signing policy](../security/release-authority.md) before performing a privileged step. |
| Which trust boundary applies?                                              | [Security](../security/index.md) owns the restriction, its accepted exceptions and their rationale. Link the operator or reviewer directly to the relevant section.                       |
| Which tool, repository path or engineering practice does this project use? | [Contributing](../contributing/index.md) owns checkout setup, command selection and shared conventions. Put observable behavior in the subject whose users depend on it.                  |

Choose a page type by the answer it gives. A Specification defines an observable
result, an Architecture assigns responsibilities and a Definition settles the
meaning of a term. Use a Procedure for ordered work, a Convention for a team
choice and a Policy for the authority shared by several operations. Keep the
constraint, reason and exception together at their owner. The
[Living Project Documentation skill](../../.claude/skills/living-project-documentation/SKILL.md)
defines the OKF v0.3 format.

## Project names

Use a compound name when a general word alone could hide a project-specific
meaning. For example, use **Runner Host** for the machine serving a job and
**Runner Image** for the image used to run it. A local variable or private helper
can use a shorter name when its scope makes the meaning clear. The
[glossary](../glossary.md) preserves active aliases and routes them to their
technical owners.

## Which Surface Invalidates Which Document

A change to one of these surfaces MUST correct the document that describes it in
the same change:

| Changed surface                                                                                             | Owner to check in the same change                                                                                                                                                                                                                                                                                                                                                                             |
| ----------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Runner image in `images/actions-runner/` or Windows configuration in `hosts/windows-docker-desktop/`        | [Image build and updates](../runner-hosts/windows-image.md), [pool configuration](../runner-hosts/windows-pools.md) and the [image contract](../marshal-cli/runner-images.md) when a label or embedded input changes. Check [pool isolation](../security/pool-isolation.md) for cache or token changes.                                                                                                       |
| Library, test helpers, build definition, diagnostic exporter, fixtures or example configuration             | Use the [marshal-cli subject](../marshal-cli/index.md) for the changed contract and the [fixture procedure](../marshal-cli/configuration-fixtures.md) for examples. Review [security policy](../security/index.md) for labels, paths and dependencies. For an axpc field, update the [pool configuration](../runner-hosts/windows-pools.md) and [host example](../../hosts/windows-docker-desktop/README.md). |
| Tart, macOS prerequisites, signals or keep-awake, the golden assets, build VM, base pin or Mac host example | [VM runtime](../marshal-cli/tart-backend.md), [macOS pools](../marshal-cli/macos-pools.md) and the [affected Mac host procedure](../runner-hosts/index.md#macos-with-tart). For isolation, Softnet, hand-off, cleanup or pin changes, check [VM security](../security/vm-isolation.md) and [review lenses](../../REVIEW.md). Update the [glossary](../glossary.md) for a renamed concept.                     |
| App crates, pages, fonts, colours, icons, capabilities, licence notices, dependency policy or Tauri CLI pin | Update the [affected Marshal page](../marshal/index.md), the [web-view policy](../security/app-trust.md) or [dependency policy](../security/dependency-pins.md#the-apps-dependencies). Follow the [agent contract](../marshal-cli/agent-protocol.md) or [workspace checks](../marshal-cli/toolchain.md) when the library boundary changes.                                                                    |
| GitHub Actions workflow                                                                                     | [Workflow permissions and review triggers](../security/workflow-permissions.md), [dependency pins](../security/dependency-pins.md) and [release authority](../security/release-authority.md). Update [test commands](./testing.md) for merge-check changes and the [Claude CI review procedure](../agent-work/claude-ci-review.md) for review behavior.                                                       |
| Release workflows, `release/`, release scripts and tests, or `cli/.cargo/config.toml`                       | [Release artifacts](../marshal-cli/release-artifacts.md), [release authority](../security/release-authority.md) and [publisher procedure](../releases/publish.md).                                                                                                                                                                                                                                            |
| An `<!-- install-block: <id> -->` block or its test or merge job                                            | Keep the marker with its executable example in the [app installation procedure](../marshal-cli/install-on-windows.md). Check the [release record](../security/release-authority.md), [Windows host setup](../runner-hosts/windows-setup.md) and [verification commands](./testing.md) when affected.                                                                                                          |
| Hook, subagent definition, agent configuration or provisioning script                                       | [Agent session setup](../agent-work/session-setup.md), [quality hooks](../agent-work/quality-hooks.md) or [delegated roles](../agent-work/delegate.md). Update [contributor setup](../operations/development-setup.md) when the tools a contributor prepares change.                                                                                                                                          |
| Installed skill, `skills-lock.json` or project exception                                                    | [Skill installation](../agent-work/skill-installation.md) and any affected [adopted exception](../agent-work/skill-exceptions.md).                                                                                                                                                                                                                                                                            |
| Change loop, grant, branch naming or post-push review                                                       | [Change delivery](../agent-work/change-delivery.md), [delivery grant](../agent-work/delivery-grant.md), [branch convention](../agent-work/branches.md) or [review policy](../agent-work/independent-review.md) owns the changed decision.                                                                                                                                                                     |
| Mise task, tool pin or checkout setup                                                                       | [Verification commands](./testing.md) owns the task. [Development setup](../operations/development-setup.md) and the [project README](../../README.md) tell a contributor which tools to prepare.                                                                                                                                                                                                             |
| Technology or image, host, library, app, release or agent-host path                                         | Update the [repository layout](./directory-structure.md) and the navigation entry for a new or removed document.                                                                                                                                                                                                                                                                                              |
| A host’s supported operating mode                                                                           | [Project status](../../README.md), the [Windows or Mac operator route](../runner-hosts/index.md), [Marshal availability](../marshal/availability.md#what-exists) and the [library entry points](../marshal-cli/entry-points.md) distinguish source, released CLI and tested deployments.                                                                                                                      |

## Checking a Documentation Change

Run `mise run check:docs` from the repository root. The check validates the entire
bundle against the pinned OKF v0.3 validator and the project's engineering
rules. It resolves checkout-relative links with `--source-root .`. Run
`mise run check:links` as well for links from the project and agent entry points.
Use the [toolchain setup](../operations/development-setup.md#documentation-checks)
for a fresh checkout.

For a standalone export, run `mise run check:docs -- /absolute/path/to/export`
on the isolated bundle. Internal links and assets must work offline. A link to
source outside the bundle must identify the exported revision. Check shipped
evidence and assets against the originals byte for byte.

Walk from the root index, the subject index and a representative caller to
the passage that settles the decision. Check the exception as well as the
usual outcome. For example, a rollback link should reach the conditions
for rollback, including the case that requires the operator to stop. Preserve
published paths and fragments until their retirement has been agreed. A passing
validator establishes reachability and structure. A reviewer still needs to
verify that the page answers the question its link promises.
