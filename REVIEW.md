# Code review policy

Apply this file when reviewing changes, not when implementing them. It is this
repository's highest-priority review-only policy and complements the
[Code Review](./.claude/skills/code-review/SKILL.md) methodology. Where
posted-report instructions differ, this file governs. The Claude adapter in
[claude-review.yaml](./.github/workflows/claude-review.yaml) supplies the policy
through its system prompt, the Amp review child thread's assignment names it,
and [AGENTS.md](./AGENTS.md) instructs reviewers to read and apply it. Availability does not prove consumption or compliance: check the
reviewer-origin result against this policy, and treat unobserved provider input
as unknown, not as proof of a failed review.
[Development Workflow](./docs/operations/development-workflow.md) owns provider
invocation and completion evidence. Code Review's
[Posted and CI Reviews](./.claude/skills/code-review/SKILL.md#posted-and-ci-reviews)
section owns the default posted-review rules.

This is a **strict** review: run every mandatory check and report every finding
within the scope below. Review the acceptance criteria carried in the pull
request body's **Acceptance criteria** section. Do not open the tracking issue
to find them; an absent criteria section is itself a finding (Important in
Claude and Amp review).

## Adopted review arrangements

Substantive checks apply to every route. These arrangements replace conflicting
presentation and reporting-scope defaults in this file and Code Review:

- **Codex** — accept native priorities, inline findings, summaries, no-findings
  responses, and provider boilerplate. No Important/Nit conversion, formal
  review object, zero tally, affirmative per-check assertions, or proof of
  lower-priority completeness is required. The native high-priority reporting
  filter is accepted, not an explicit unchecked-material limitation. Preserve
  every actual finding, including P2/P3, with its identity, priority, citations,
  and disposition; native acceptance never dismisses it. A final thumbs-up
  reaction can confirm a clean round only under Development Workflow's
  request, material, and terminal-result correlation.
- **Claude** — use diff-anchored inline findings plus exactly one top-level
  summary, not a single formal review submission. Important/Nit labels,
  all-findings reporting, summary scope, and the tally remain required,
  including `0 important, 0 nits` for a clean round. This review is advisory,
  with no APPROVE or REQUEST_CHANGES verdict.
- **Amp** — the Amp review child thread follows the Claude arrangement, with two
  differences. It posts its diff-anchored inline findings and its one summary
  together as a single submitted COMMENT-type review, never APPROVE or
  REQUEST_CHANGES. Its summary also carries the `<!-- ai-agent -->` marker, the
  child thread's URL, and the reviewed commit, because the review is posted
  under the same GitHub account as the change it reviews.

Accepting native scope accepts the risk of unreported omissions, not proof of
exhaustive checking. Explicitly unchecked required scope, outstanding findings,
fresh-review requirements, and contributor evidence still constrain readiness
under [Loop Engineering](.claude/skills/loop-engineering/SKILL.md).

## Severity labels for posted reviews

Claude and Amp review use Code Review's two posted labels, **Important** and
**Nit**, instead of its internal Critical/Major/Minor/Nit triage or an
Approve/Request-Changes verdict. Codex managed review uses native priority
labels; its presentation and reporting scope follow the adopted arrangements
above. In this repository, a hard project rule includes every MUST rule of a
skill whose `description` matches the changed files and every applicable MUST
rule in project conventions, such as [Security](./docs/conventions/security.md),
subject to registered deviations in the
[Agent Skills register](./docs/operations/agent-skills.md#the-register), the
reporting exclusions below, and Code Review's acceptance-criteria finding
boundary.

**Guidelines:**

- MUST flag an applicable hard-rule violation and cite its owning skill or
  project convention and rule. Claude and Amp review MUST label it Important;
  Codex review uses its native priority label without implying an exact
  equivalence.

## Mandatory checks

Run the following checks on every review and raise findings within their scope:

- **Skills and project conventions conformance** - verify the change against
  every skill whose `description` matches the changed files and every applicable
  project convention. Honor the
  [Agent Skills register](./docs/operations/agent-skills.md#the-register). Cite
  the owning skill or convention and rule for each deviation, without
  reintroducing external-evidence absence-only findings.
- **Acceptance criteria** - assess every criterion in the pull request body
  under [Code Review's review lenses](./.claude/skills/code-review/references/review-lenses.md),
  Acceptance Criteria boundary. Report violations demonstrable by inspecting
  the changed material, inline where a changed line supports the finding.
  Claude and Amp review label these Important; Codex uses its native
  priorities. A real-host result, machine-specific measurement, or other
  external verification does not become a finding solely because it cannot be
  confirmed from the diff or lacks an evidence link, whether designated or not.
  Do not re-run or judge external evidence in this inspection check.
  Contributor verification and required evidence remain separate readiness
  obligations in
  [Development Workflow](./docs/operations/development-workflow.md).
- **Public-repository safety** - this repository is public and is meant to hold
  nothing that identifies a person or a machine. Verify the
  [safety lenses](#public-repository-safety-lenses) below for every changed
  file, apply the [VM runtime lenses](#vm-runtime-lenses) to a change of the
  axmc Linux VM runtime, and apply the [language lenses](#language-lenses) for
  each kind of file in the diff.
- **Refactoring opportunities** - examine changed code, scripts, workflow
  definitions, and documentation for concrete ways to simplify
  responsibilities, remove duplication, or reduce unnecessary complexity. Report
  opportunities supported by the changed material using Code Review's existing
  evidence and severity rules, not a new severity floor. MUST NOT expand this
  check into unrelated pre-existing material or speculative abstractions and
  cleanup.

**Guidelines:**

- MUST run every check above on every review.
- MUST give each finding a severity label supported by the selected provider,
  `file:line` evidence, and a concrete fix according to
  [Code Review](./.claude/skills/code-review/SKILL.md).

The code-inspection limit on the acceptance-criteria check does not waive
implementation verification or self-review's reporting of required checks.
Known unmet requirements remain unmet; silence in a review cannot replace the
contributor's verification.

## Public-repository safety lenses

Each lens is a hard project rule. Claude and Amp review label a violation
Important; Codex review uses its native priority label.

- **No credentials or host-identifying values.** No token, key, password,
  registration or JIT configuration, `.env` value, internal hostname, IP
  address, LAN address, machine or user name, home-directory path, email
  address, or account name is committed, in a file, a default, an example, a
  comment, or a test fixture. The public maintainer handle and the owner and
  repository names of this repository and of `axross/skills` are allowed, and so
  are the default runner labels `axpc` and `axmc`, the two committed host labels,
  kept by the maintainer's decision, and the macOS signing team ID and designated
  requirement that a later CLI record may hold, as Security records (see
  [Security](docs/conventions/security.md#nothing-identifying-or-secret-is-committed)).
  Verbatim third-party licence and copyright text in the notices under
  `app/notices/` and the licence copies they draw on, the crates' published
  repository URLs in those notices, and `docs/operations/app.md`'s references to a
  third-party crate's public repository are the exception Security records for
  personal names, account names and email addresses, so they are not reported; such
  a name or address anywhere else still is.
  An example uses an obviously fake
  placeholder (`<owner>`, `<repo>`, `example.invalid`). A value a runner needs
  is read from the environment or a prompt at run time.
- **No values specific to the repository this code was extracted from.** The
  runner material is generic. An owner, repository, image, container, volume,
  or scheduled-task name belonging to a particular consumer is a parameter,
  never a literal. The default runner labels `axpc` and `axmc` are the two
  exceptions.
- **No self-hosted runners for this repository's own jobs.** Every workflow
  here uses a GitHub-hosted runner (`ubuntu-latest`, `ubuntu-24.04-arm`,
  `windows-latest`, `macos-latest`, or another hosted label).
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
  same standard. There are three recorded exceptions, all in
  [Security](docs/conventions/security.md#pinning-convention): a step that
  installs the Rust toolchain `cli/rust-toolchain.toml` or
  `app/rust-toolchain.toml` names through `rustup`, the Node that
  `actions/setup-node` installs in `release.yaml`, and `tauri-cli`, installed
  from source with `cargo install --locked` at the one version in
  `app/tauri-cli.version`. Any other unpinned download is still a finding.
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
  mount is justified where it is declared. A volume of installed toolchains
  (the tool cache, `~/.cargo`, `~/.rustup`) is a finding unless it meets all
  three conditions of the bounded exception in
  [Security](docs/conventions/security.md): one target repository, no
  deployment-secret job on its labels, and no pull request from a fork on its
  labels. Any volume on an entry whose labels run a deployment-secret job or a
  fork pull request is a finding.
- **Runner registration and lifetime.** A runner is ephemeral and registered
  per job through a just-in-time configuration. A long-lived registration
  token, a runner reused across jobs without a reset, and a container that
  runs privileged, mounts the Docker socket, or mounts the host's home
  directory without a stated need are findings.
- **Secrets stay out of logs and images.** A secret is never echoed, passed on
  a command line that a process listing exposes, written into an image layer
  (`ENV`, `ARG`, `COPY`), or left in a build cache. Two exceptions for a
  credential are recorded, both in `sign-macos`: `security`'s password arguments, and
  `notarytool`'s `--key-id` and `--issuer` arguments for the notary key's ID and issuer
  ID. The `.p8` key reaches `notarytool` only as a file path. Any other change to how
  the key's ID, issuer ID or `.p8` reaches `notarytool` is a finding to raise (see
  [Security](docs/conventions/security.md#nothing-identifying-or-secret-is-committed)).

## VM runtime lenses

These lenses apply to a change of the axmc Linux VM runtime: `cli/src/runtime/tart/`,
`cli/src/macos/`, `cli/src/golden/`, the golden VM's provisioning assets, and the
macOS host document. Each is a hard project rule, owned by
[Security](docs/conventions/security.md#a-linux-job-on-axmc-runs-in-a-per-job-vm)
and the sections after it. Claude and Amp review label a violation Important;
Codex review uses its native priority label.

- **No JIT configuration in an argument list.** The JIT configuration reaches the
  guest only on the standard input of a `tart exec -i` call, never with `-t`, and
  inside the VM only the launcher puts it in the Docker client's environment. It is
  never an argument of `tart`, `tart exec`, the launcher or `docker`, never a log
  line, and never part of the VM's image.
- **Guest output is bounded.** Every guest-agent call's captured output is cut off
  at a fixed bound (64 KiB), and the CLI never parses it beyond its expected form.
  The one exception is the diagnostics export's `docker cp` archive, which
  `VmCopy` streams; the strict archive reader and the bundle limits bound it
  instead. An unbounded read of any other guest call, or a parse that trusts its
  shape, is a finding.
- **Deletion on every failure path.** Every exit from a slot's cycle after the
  clone, and from a build until its rename, deletes that VM within a budget: an
  error, a failed probe, a boot failure, a stop, an interrupt and a timeout. After
  the rename the build VM is the golden VM and is not deleted. A new early return,
  `?` or `break` between the clone and the deletion is checked for it. The shim
  deletes the VM when the liveness pipe closes, which covers a CLI that ended at the
  exit deadline or died, and a VM neither deleted is left to the next `start` or
  `build`. No shim or other child inherits the instance lock.
- **The Softnet check and rules on every boot.** Every `tart run` follows a Softnet
  file check and a version check, and carries the two Softnet rules from
  [Security](docs/conventions/security.md#softnet-confines-a-job-vms-network) and no
  other network option. A boot path that skips the check, a cached result, a
  `marshal` that sets Softnet's SUID bit, and a fallback to Tart's default network
  are findings.
- **No shared directory, clipboard, audio, USB accessories or Rosetta.** Every
  `tart run` turns off graphics, the clipboard, audio and USB accessories, and
  carries no `--dir` and no `--rosetta`. Every Tart call sets `TART_NO_AUTO_PRUNE`,
  gets `marshal`'s `PATH`, and has a standard output that is not a terminal.
- **Names and cleanup stay exact.** Cleanup at `start` matches only the job VMs of
  configured pools by their name form, so it never deletes a golden VM or a build
  VM. Only `build` deletes golden VMs, and only `build` and a build VM's own shim
  delete a build VM. A looser match could delete a VM `marshal` does not own.
- **Pins hold.** The base image is pinned by digest, the current pin is never on
  the retired list, Docker Engine comes from Docker's apt repository at pinned
  versions after the signing key's fingerprint is checked, and the tested Tart and
  Softnet versions agree between the CLI and the host document.

The Dockerfile lens's rule against `apt-get upgrade` is about the runner image. The
golden VM's provisioning upgrades the base's Ubuntu packages on purpose, so a
rebuild picks up security fixes; that is not a finding.

## Language lenses

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
- **Release workflows.** A change to
  [`release-build.yaml`](./.github/workflows/release-build.yaml),
  [`release.yaml`](./.github/workflows/release.yaml), the release-build job and the
  `Install Blocks` job in `merge-checks.yaml`, the scripts under `.github/scripts/`,
  the release tooling under [`release/`](./release), or `cli/.cargo/config.toml` is
  checked against each bullet of the release record in
  [Security](docs/conventions/security.md#a-release-is-published-only-from-a-dispatch-on-main).
  A publish or attest step outside `release.yaml`, a job that both runs the npm tree
  and holds `id-token: write`, an asset other than the two named binaries, the two
  installers named for them, and `SHA256SUMS`, such as a helper binary, an installer
  that carries a script or a custom action that runs code, a download that is none of
  the pinned WiX package, the recorded Rust toolchain exception, or the Node that
  `actions/setup-node` installs (recorded under Security's Pinning Convention), or a
  departure from the record that the change does not also make to the record, is a
  finding. Install-time verification is optional for the installers, so a document or
  comment that presents the check block as required of an operator is a finding, and
  so is a change that weakens a pin or drops one of the check block's four checks, or
  any check of the bare-binary blocks that roll back to `v0.1.0`. The signing job's
  rules are also findings when broken: an `environment:` in any job but `sign-macos`,
  or in a workflow a pull request triggers or a job such a workflow calls; a signing
  secret read by any job but `sign-macos`; `sign-macos` holding more than
  `contents: read`, running a checkout, an action besides `actions/download-artifact` and
  `actions/upload-artifact`, a package manager, a fetch, a file from the repository,
  or a `${{ }}` inside `run:`, or calling a signing tool by anything but its system path; a
  signing step that a later job can pass without, such as `continue-on-error`, a
  job-level `always()`, `!cancelled()` or `failure()`, a step condition in
  `sign-macos` beyond the `if: always()` of the clean-up and the notary-log upload, a
  step condition in `verify-macos` or `assemble-signed`, or one in `attest` or
  `release` that skips a step, which the shape check does not see; an attest or release job that reads
  the unsigned set or does not follow every signing step; a secret printed, or passed
  on a command line apart from `security`'s password arguments and `notarytool`'s
  `--key-id` and `--issuer` arguments in `sign-macos`, the two exceptions for a
  credential that Security records; a change to how the notary
  key's ID, issuer ID or `.p8` key reaches `notarytool`; a change to the identifier
  `app.axross.marshal`, the team pin, or the G2 intermediate's pin without its source and the record's update;
  and a signing job or step added without being added to `check-release-shape.mjs` and
  its test. A change that relaxes the record, or touches the signing job, needs the
  maintainer's review.
- **Rust.** The CLI under `cli/` and the app's crates under `app/` have no
  `unsafe` and no panic on an input path (`unwrap`, `expect`, `panic!`,
  indexing). The CLI collects every error in the order
  [CLI](docs/operations/cli.md) states under Validation Rules rather than returning
  the first. A new CLI crate has an
  entry in [CLI](docs/operations/cli.md) and passes `cli/deny.toml`; a new direct
  dependency of an app crate is recorded in [Security](docs/conventions/security.md#the-apps-dependencies)
  and passes `app/deny.toml`. A new
  procedural-macro crate, such as one a derive feature brings, is flagged for the
  maintainer's review, and so is a new duplicate-version skip. No token or JIT
  configuration reaches a `Debug` form, a log line, an error message, a command
  line, a control-channel response, or, in the app, the page, a view state or an
  event; a new log line or response that could carry
  one unmasked is a finding (see
  [Security](docs/conventions/security.md#tokens-stay-out-of-the-log-and-job-clients-end-with-start)).
  A change to a validation rule changes the shared fixtures
  and their expectations in the same change. Give `file:line` evidence as in the other
  lenses.
- **The app's web view.** A change under `app/` is also checked against the app's
  record in [Security](docs/conventions/security.md#the-app-surface), its web view,
  capability and token bullets. The content security policy in `tauri.conf.json`
  stays the one Security states in full, and `tauri.conf.json` keeps the web view
  on bundled files with no developer tools in the release build. The two
  capabilities, the main window's and the confirmation window's, and the permission
  files Tauri generates keep to the permissions Security lists; a permission, a
  capability or Tauri feature added, such as `dynamic-acl`, is a finding. The
  pages stay text-only: `dom.js` refuses `style`, `href`, `src`, `srcdoc`, `action`, `formaction` and
  `on*`,
  and text from the configuration, Docker or the agent is set as text, never as
  markup, with no inline script, remote asset or npm package. No token sits in the
  page, a view state or an event after `token_set` is called. A change to the
  policy, a capability, a permission file, `tauri.conf.json` or the element builder
  needs the maintainer's review. The test manifest `build.rs` links into the
  integration tests holds only the version 6 common controls dependency and only on
  a Windows MSVC target; another entry in it, such as an execution level, a second
  `embed-resource` call, or a test skipped or cfg-ed out to avoid it, is a finding
  and needs the maintainer's review.
- **Diagnostics.** A labelled diagnostic container is removed only by a
  finalisation, which tries its export while its clock allows and uses its recorded
  settings, or by the manual recovery. The one exception to the recorded settings is
  a container an earlier process left: no record exists, so a startup with the
  pool's diagnostics on adopts it with the pool's current settings. A path that
  removes one without trying its export while its clock allows, or finalises one
  the backend already retains with a later plan's settings, is a finding. A
  diagnostic VM on axmc is deleted without an export by design in exactly three
  cases: a second SIGINT, a `start` that ends while it still holds the VM (the VM
  shim), and a leftover at the next `start`, each reporting the diagnostic gap. A
  fourth export-less deletion of a VM, or any of the three that reports no gap, is a
  finding (see [CLI](docs/operations/cli.md#tart-backend), the paragraph "Evidence a
  VM loses"). The sink checks refuse and never create, repair, or relax, and a new
  log line, warning, or channel answer names only the pool, the container, and the
  gap, never bundle contents. The exporter child and each of its clients, the
  `docker` clients on axpc and the `tart exec` clients on axmc, end by their
  deadline and with `start`; a new wait in that path that has no bound,
  or that polls for liveness instead of a closed pipe or a process handle, is a
  finding (see [CLI](docs/operations/cli.md#diagnostics) and
  [Security](docs/conventions/security.md#diagnostic-evidence-stays-private)).
- **Image contract.** A change to the run contract, which is the argument lists,
  the launcher, and the labels the CLI reads, or to what the runner image must
  provide for it, such as a path, user, group, or tool the launcher or a job
  relies on, raises the contract version in `images/actions-runner/Dockerfile`
  and in `cli/tests/build-definition/Dockerfile`, and adds the new version's row
  to `APPROVED_CONTRACTS` in `cli/src/image/contract.rs`. A change that alters
  either without raising the version is a finding, even when `APPROVED_CONTRACTS`
  cannot see it. A runner bump sets the `FROM` line and the `runner-version` and
  `runner-released` labels of both Dockerfiles, and adds the release's date from
  the actions/runner release page to `RUNNER_RELEASES` in the same file. The
  tests that depend on the declared values read them from the Dockerfiles or the
  CLI's contract version, so neither change edits any other test; see
  [CLI](docs/operations/cli.md#the-image-contract).
- **Documentation.** A claim about runner behavior is either verified or
  described as planned. A document that describes a file, image, or script
  that does not exist yet says so.

## Reading beyond the diff

[Code Review's review scoping](./.claude/skills/code-review/SKILL.md#review-scoping)
owns boundary checks: open each owner a change names and every scope-overlapping
neighbor, even outside the diff. For installed-skill changes, follow
[Agent Skills](./docs/operations/agent-skills.md); installed directories are
generated and must match their selected lockfile inventory.

**Guidelines:**

- MUST verify every deferral against the named owner and open undeclared
  overlapping owners that could expose duplication or a conflicting rule.
- MUST NOT infer whole-change compliance from one compliant instance.

## Reporting exclusions

In posted reviews, MUST NOT repeat exact mechanical failures from the checks
that [`merge-checks.yaml`](./.github/workflows/merge-checks.yaml) runs through
`mise run check` on every pull request. The exclusion is only the exact
mechanical defect named below:

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
- Rust formatting (`mise run format:check`), clippy (`mise run lint:rust`), the
  CLI's tests (`mise run test:rust`), and its licence, source, and duplicate checks
  (`mise run deny:rust`). The advisory check runs in its own workflow and is not
  required.
- The app's clippy (`mise run lint:app`), tests (`mise run test:app`), licence,
  source, and duplicate checks (`mise run deny:app`), toolchain pin check
  (`mise run check:app-pins`), lockfile check (`mise run check:app-lock`), main
  window page tests (`mise run test:app-frontend`), the test of the `tauri-cli` pin's
  reader (`mise run test:tauri-cli-version`), and the licence notices check
  (`mise run check:notices`).
- The agreement of `skills-lock.json` with `.claude/skills/`
  (`mise run check:skills`). Whether an installed copy matches its upstream
  source is not checked in CI and remains reportable.

Internal self-review still flags these mechanical failures.

MUST report broader rule failures these checks do not establish:

- Design failures.
- Inaccurate documentation.
- Wrong owners.
- Missing coverage.

A linter passing does not show that a Dockerfile, script, or workflow is safe,
only that it is well-formed. MUST NOT generalize these exclusions to every check
CI happens to run.

## Reporting

Reviewers need not enumerate passed criteria, applied skills, changed files, or
successful refactoring checks. Report findings and explicit limitations in the
provider's presentation, as the adopted review arrangements above describe.
When required material remains unavailable or review scope remains unchecked,
MUST name that material or scope and why. MUST reference findings instead of
repeating them.

An identifiable completed independent review treats unflagged items as
satisfied for the review gate, without positive per-check assertions. This
review outcome is not proof that each rule was consumed or each check ran; the
[delivery procedure](./docs/operations/development-workflow.md#review-coverage-evidence)
owns the remaining readiness blockers. Review completion and contributor
readiness are separate; a quiet review cannot discharge author-side
verification or evidence obligations.

**Guidelines:**

- Model-authored review prose MUST NOT include any provider's review trigger
  phrase, documented in
  [the independent review](./docs/operations/development-workflow.md#the-independent-review),
  anywhere in a summary or an inline comment. Refer to it by name instead, for
  example "the review request". App-generated status and help text are exempt;
  the model does not control that fixed text. The Claude workflow's loop guard
  admits the reviewer's own `claude[bot]` comments unless they carry a Markdown
  heading, so a summary that quotes the phrase can start another review.
