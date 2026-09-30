# Review Instructions

This file is this repository's highest-priority review-only policy. The Claude
adapter in [`claude-review.yaml`](.github/workflows/claude-review.yaml) supplies
it through the workflow's system prompt. This policy complements the
[Code Review](.claude/skills/code-review/SKILL.md) methodology, whose
[Posted and CI Reviews](.claude/skills/code-review/SKILL.md#posted-and-ci-reviews)
section owns the generic posted-review rules.

This is a **strict** review: run every mandatory check and report every
finding. Verify the acceptance criteria carried in the pull request body's
**Acceptance criteria** section. Do not open the tracking issue to find them;
an absent criteria section is itself a finding (Important).

## Severity Vocabulary for Posted Reviews

Claude review uses Code Review's two posted labels, **Important** and **Nit**,
instead of its internal Critical/Major/Minor/Nit triage or an
Approve/Request-Changes verdict. In this repository, a hard project rule
includes every MUST rule of a skill whose `description` matches the changed
files, and every MUST rule of
[Security](docs/conventions/security.md).

**Guidelines:**

- MUST flag a matching skill's MUST-rule violation and cite the skill and
  rule, labelled Important.

## Mandatory Checks

Run all three checks on every review and raise a finding for each miss:

- **Skill conformance** - verify the change against every skill whose
  `description` matches the changed files. Cite the owning skill and rule for
  each deviation.
- **Acceptance criteria** - verify every criterion in the pull request body.
  A criterion carrying the `(verified out of tree: <where>)` marker is checked
  only for a link to its published evidence: with the link, it is not a
  finding and gets one summary line naming it as designated, every round;
  without the link, it is a finding, unless the marker names a destination
  that can only exist after the pull request merges. Such a criterion is not a
  finding; it gets one summary line naming it as designated and pending, every
  round, and the author links the evidence once it exists. Do not re-run or judge
  the evidence itself. Every other unmet or diff-unconfirmable criterion is a finding,
  anchored inline when it attaches to a changed line, labelled Important.
- **Public-repository safety** - this repository is public and is meant to hold
  nothing that identifies a person or a machine. Verify the lenses below for
  every changed file.

**Guidelines:**

- MUST run all three checks on every review.
- MUST give each finding a severity label, `file:line` evidence, and a concrete
  fix according to [Code Review](.claude/skills/code-review/SKILL.md).

## Public-Repository Safety Lenses

Each lens is a hard project rule. A violation is Important.

- **No credentials or host-identifying values.** No token, key, password,
  registration or JIT configuration, `.env` value, internal hostname, IP
  address, LAN address, machine or user name, home-directory path, email
  address, or account name is committed, in a file, a default, an example, a
  comment, or a test fixture. The public maintainer handle and the owner and
  repository names of this repository and of `axross/skills` are allowed. An example uses an obviously fake
  placeholder (`<owner>`, `<repo>`, `example.invalid`). A value a runner needs
  is read from the environment or a prompt at run time.
- **No values specific to the repository this code was extracted from.** The
  runner material is generic. An owner, repository, image, container, volume,
  or scheduled-task name belonging to a particular consumer is a parameter,
  never a literal.
- **No self-hosted runners for this repository's own jobs.** Every workflow
  here uses a GitHub-hosted runner (`ubuntu-latest` or another hosted label).
  A `runs-on` that names `self-hosted`, or a label only a self-hosted runner
  carries, is a finding. A runner image or host script in this repository is a
  product the repository ships, not a runner it uses.
- **Third-party actions are pinned to a full SHA.** Any `uses:` outside the
  `actions/` organization is pinned to the 40-character commit SHA behind its
  latest release tag, with that tag as a trailing comment
  (`uses: owner/action@<sha> # vX.Y.Z`). The SHA must be the tag's peeled
  commit. `actions/*` stays on a major tag by the recorded decision in
  [Security](docs/conventions/security.md#actions-stays-on-major-tags); do not
  report it. A reusable workflow, a `docker://` reference, and a `run:` step
  that downloads and executes remote code without a checksum are held to the
  same standard.
- **Least-privilege workflow permissions.** Every workflow declares a
  top-level `permissions:` block that starts from `contents: read`. A broader
  scope is granted on the job that needs it, is justified in a comment or in
  [Security](docs/conventions/security.md), and never on a job that runs
  pull-request-controlled code. `pull_request_target` and `workflow_run` need a
  stated reason; `issue_comment` workflows check out the default branch, never
  the pull request's head.
- **Cache and volume boundaries.** A runner image, host script, or workflow
  that shares a cache or volume between jobs is judged against the threat model
  in [Security](docs/conventions/security.md): what a job can write for a
  later, more privileged job to read. A volume that crosses a repository, a
  trust level, or a fork boundary, a cache keyed by attacker-controlled input,
  and a cache restored into a job that holds a secret are findings. A shared
  mount is justified where it is declared.
- **Runner registration and lifetime.** A runner is ephemeral and registered
  per job through a just-in-time configuration. A long-lived registration
  token, a runner reused across jobs without a reset, and a container that
  runs privileged, mounts the Docker socket, or mounts the host's home
  directory without a stated need are findings.
- **Secrets stay out of logs and images.** A secret is never echoed, passed on
  a command line that a process listing exposes, written into an image layer
  (`ENV`, `ARG`, `COPY`), or left in a build cache.

## Language Lenses

Apply the lens for each kind of file in the diff.

- **Dockerfile.** A base image is pinned to a version and, for a shipped image,
  a digest. Layers are ordered and combined so a rebuild reuses the cache, and a
  package-manager cache is removed in the layer that created it. The image does
  not run as root unless the workload requires it, and the reason is stated.
  Downloads are checksum-verified. `ADD` from a URL, `latest` tags, and
  `apt-get upgrade` are findings.
- **Shell.** Scripts start with `set -euo pipefail` (or a stated reason they do
  not), quote every expansion, avoid `eval` and `curl | sh`, and handle a
  missing tool or a failed download explicitly. Untrusted input, such as a
  pull request title or branch name, is never interpolated into a command.
- **PowerShell.** Scripts set `$ErrorActionPreference = 'Stop'`, avoid
  `Invoke-Expression`, pass arguments as arrays rather than built strings, and
  check native-command exit codes (`$LASTEXITCODE`) explicitly, because
  `Stop` does not cover them. A scheduled-task or service definition runs with
  the least-privileged account that works.
- **Workflow YAML.** Expression injection: an untrusted context value
  (`github.event.*.title`, `body`, `head_ref`, a comment body) is passed through
  an `env:` variable and quoted, never expanded inside `run:`. A job has a
  `timeout-minutes`. `concurrency` cancels superseded pull request runs only.
- **Documentation.** A claim about runner behavior is either verified or
  described as planned. A document that describes a file, image, or script
  that does not exist yet says so.

## Reading Beyond the Diff

[Code Review's review scoping](.claude/skills/code-review/SKILL.md#review-scoping)
owns boundary checks: open each owner a change names and every scope-overlapping
neighbor, even outside the diff. For installed-skill changes, follow
[Agent Skills](docs/operations/agent-skills.md); installed directories are
generated and must match their selected lockfile inventory.

**Guidelines:**

- MUST verify every deferral against the named owner and open undeclared
  overlapping owners that could expose duplication or a conflicting rule.
- MUST NOT infer whole-change compliance from one compliant instance.

## Do Not Report

Do not repeat mechanically enforced failures in a posted review when the
matching path filter means that check runs. The exclusion is only the exact
mechanical defect named below; broader design judgment and missing coverage
remain review findings. [`merge-checks.yaml`](.github/workflows/merge-checks.yaml)
runs `mise run check` on every pull request, which enforces:

- Prettier formatting (`mise run format:check`) of Markdown, JSON, YAML, and
  TypeScript files.
- markdownlint (`mise run lint:markdown`), shellcheck (`mise run lint:shell`),
  hadolint (`mise run lint:docker`), actionlint (`mise run lint:actions`), and
  PSScriptAnalyzer (`mise run lint:powershell`) findings on the files each one
  covers.
- The relative-link check (`mise run check:links`).
- The `docs/` structural validators (`mise run check:docs`). Their narrow
  structural findings are excluded; documentation accuracy and ownership are
  not.
- The agreement of `skills-lock.json` with `.claude/skills/`
  (`mise run check:skills`). Whether an installed copy matches its upstream
  source is not checked in CI and remains reportable.

**Guidelines:**

- MUST NOT report a posted finding covered exactly by this list.
- MUST report broader prose-rule failures when a listed check is only a narrow
  proxy. A linter passing does not show that a Dockerfile, script, or workflow
  is safe, only that it is well-formed.
- MUST NOT generalize this list to every check CI happens to run.

## Reporting

[Code Review's posted-review policy](.claude/skills/code-review/SKILL.md#posted-and-ci-reviews)
owns the Claude review container, finding shapes, tally, and summary scope.

**Guidelines:**

- Claude review MUST NOT write the review trigger phrase, the literal documented
  in [the independent review](docs/operations/development-workflow.md#the-independent-review),
  anywhere in a summary or an inline comment. Refer to it by name instead, for
  example "the review request". The workflow's loop guard admits the reviewer's
  own `claude[bot]` comments unless they carry a Markdown heading, so a summary
  that quotes the phrase can start another review.
- Claude review MUST use its two-output route: diff-anchored inline findings
  plus exactly one top-level summary with the required tally, including
  `0 important, 0 nits` when no findings exist. This is this repository's
  exception to Code Review's single-submission rule; the advisory review
  carries no APPROVE or REQUEST_CHANGES verdict.
