# Security

This project's supply-chain and runner-trust conventions: how a `uses:` reference
under [`.github/`](../../.github) is pinned, what a workflow may be granted, where
a self-hosted runner may and may not run, what a runner's shared storage exposes,
and what is never committed. It does not cover application-level security, such as
input validation or the OWASP-style concerns the installed `application-security`
capability owns.

The runner image under [`images/actions-runner/`](../../images/actions-runner) and
the Windows host scripts under
[`hosts/windows-docker-desktop/`](../../hosts/windows-docker-desktop) exist and are
reviewed against the sections on runner trust and shared storage below. The
agent-host configuration this repository is also for does not exist yet; those
sections are the constraints it will be reviewed against.

## Pinning Convention

A third-party action, anything outside the `actions/` GitHub organization, is
pinned to the full 40-character commit SHA behind its latest release, with that
release's tag kept as a trailing comment for a human to read:

```yaml
uses: owner/action@d34db33fd34db33fd34db33fd34db33fd34db33f # v1.2.3
```

A mutable tag (`@v1`, `@v1.2.3`, `@main`) can be repointed by whoever controls it.
A compromised maintainer account or release pipeline moves the tag, and every
workflow that trusts it runs the new code on its next dispatch without anyone here
changing a line. A commit SHA cannot be repointed: it names one immutable tree.

The SHA MUST be resolved from a release tag with
`git ls-remote URL 'refs/tags/vX.Y.Z^{}'`, never guessed or copied from somewhere
other than that resolution. The `^{}` peel matters because a release tag may be an
annotated tag, for which `git ls-remote` returns the tag _object's_ SHA unless the
peel is applied, a value `uses:` will not resolve as a commit. Of the two
third-party actions pinned today, `anthropics/claude-code-action` uses annotated
tags (its `v1.0.237` names a tag object that peels to the pinned commit) and
`jdx/mise-action` uses lightweight tags, where the peeled and unpeeled refs give
the same SHA. That is a fact about those repositories today, not a property to
rely on, so the peel is always the right thing to write. A SHA that was not
resolved this way MUST NOT be written: a wrong one fails at the run that first
uses it, not at review time.

The same standard applies to anything else a workflow or script fetches and
executes: a `docker://` reference, a reusable workflow, and a downloaded binary
are pinned to an immutable identifier, and a download is verified against a
checksum before it runs. The runner image's base is pinned the same way, to a
version and the digest of that version's image index, resolved from the registry
and never guessed; Dependabot's `docker` entry proposes its refreshes. [`.agents/setup`](../../.agents/setup) does this for
mise, and [`mise.toml`](../../mise.toml) pins every tool to an exact version.
[`mise.lock`](../../mise.lock), with the npm dependency locks it references under
`.mise/locks/`, records each tool's download URL and checksum for `linux-x64`;
`locked = true` is deliberately not set, so a platform the lockfile does not cover
still installs. Dependabot's `github-actions` entry refreshes action pins monthly;
the versions in `mise.toml` are refreshed by hand, looked up with `mise latest`,
never recalled. PSScriptAnalyzer is not a mise tool: `lint:powershell` downloads
its package from the PowerShell Gallery and refuses it unless its SHA-256 matches
`PSSCRIPTANALYZER_SHA256` in `mise.toml`, so the version and that hash are
refreshed together, with the hash computed from the real download. A manual
refresh of a mise tool version is: edit `mise.toml`, run
`mise lock --platform linux-x64`, and commit `mise.toml`, `mise.lock`, and
`.mise/locks/` together.

## `actions/*` Stays on Major Tags

GitHub's own `actions/` organization is excluded from the SHA-pinning rule above
and kept on a plain major-version tag (`actions/checkout@v7`), across every
workflow in this repository. This is a recorded decision, not an oversight.

The reasoning is a joint-trust argument: GitHub itself owns both the `actions/`
organization and the GitHub-hosted runner that executes it, so trusting
`actions/*`'s own tag is trusting the same party this project already trusts to
run the workflow at all, a materially different position from trusting an
independent third-party maintainer's tag. Every job here runs on a GitHub-hosted
runner, so the argument holds for all of them.

The argument does not hold for a job that runs on a machine the maintainer owns.
If a workflow ever dispatches onto a self-hosted runner, this decision has to be
revisited for that workflow on its own terms; see
[Self-Hosted Runners Stay Out of This Repository's Own Jobs](#self-hosted-runners-stay-out-of-this-repositorys-own-jobs).
A review finding this file's `actions/*` references unpinned is seeing this
decision working as intended, not a gap.

## Workflow Permissions Start From Read-Only

Every workflow declares a top-level `permissions:` block that starts from
`contents: read`. A broader scope is granted on the job that needs it, and never
on a job that runs pull-request-controlled code. `actions/checkout` sets
`persist-credentials: false` wherever the job does not itself push or call the
API with the checkout's credential. A workflow triggered by `issue_comment` checks
out the default branch, never the pull request's head, so no code a commenter
could have introduced runs in a job that holds a secret.
[`merge-checks.yaml`](../../.github/workflows/merge-checks.yaml) holds only
`contents: read`. [`claude-review.yaml`](../../.github/workflows/claude-review.yaml)
holds `contents: read` at the top and, on its one job, `contents: read` plus
`id-token: write`. It needs no pull request, issue, or check write scope: at the
pinned version the action exchanges the job's OpenID Connect token for a Claude
GitHub App token and uses that token for every write to the pull request, and
the job token's only other use is an optional CI-status server that stays off
unless `actions: read` is granted and requested. Do not add a write scope to that
job without re-reading the action at the pinned SHA and recording why.

An untrusted context value (a comment body, a pull request title, a branch name)
is passed into a step through an `env:` variable and quoted, never expanded inside
`run:`. The workflow `if:` expression is not a substitute for that.

## The Review Plugin Marketplace Is Not Pinned (Accepted Risk)

`claude-review.yaml` installs the `code-review` plugin from the
`https://github.com/anthropics/claude-code.git` marketplace, at that repository's
default branch at run time. The command it runs is model instructions that the job
executes with shell tools, a repository secret, and an OpenID Connect token in
reach, so this is an exception to the pinning convention above. It is an accepted
risk. The action and Claude Code block the two direct routes to pinning it:

- A ref suffix on the URL. At its pinned SHA the action validates each URL entry
  against a pattern that must end in `.git`, so a `#ref` or `?ref=` suffix is
  rejected, and it has no ref input.
- A local copy under the marketplace's own name. The action accepts a local path,
  but the marketplace's manifest names it `claude-code-plugins`, and Claude Code
  reserves that name for GitHub sources in the `anthropics` organization.
  `claude plugin marketplace add <local clone at 525d3b35>` exits 1 with "The name
  'claude-code-plugins' is reserved for official Anthropic marketplaces and can
  only be used with GitHub sources from the 'anthropics' organization." This was
  reproduced with Claude Code 2.1.286, and reported with 2.1.285 during review.

Two indirect routes can pin the command:

- Rename the marketplace in a pinned checkout at run time, then add that local
  path. The reviewer reproduced this with Claude Code 2.1.285; it was not
  reproduced here. The maintainer was shown this route, as the option "rename and
  keep the plugin pinned", and chose to stay unpinned.
- Check the plugin out at a pinned commit and load its directory with
  `--plugin-dir` in `claude_args`. At the action's pinned SHA,
  `base-action/src/parse-sdk-options.ts` passes flags it does not recognize
  through to the CLI as extra arguments, and Claude Code 2.1.286 lists a
  `--plugin-dir` option. That was checked by reading the source and the help text
  only; no run was made. This route was identified after that decision; when it
  was put to the maintainer, they again chose to stay unpinned.

The maintainer accepted the risk. Its reach is wider than the workflow's
`--allowedTools` list suggests, because a command's own `allowed-tools`
frontmatter pre-approves tools in addition to that list, and it can change
without notice. As an example, not a guarantee, the command at commit
`525d3b35312636cf8c001ecb3df2be0324a48b43` pre-approved `Bash(gh issue view:*)`,
`Bash(gh search:*)`, `Bash(gh issue list:*)`, `Bash(gh pr list:*)`,
`Bash(gh pr comment:*)`, `Bash(gh pr diff:*)`, `Bash(gh pr view:*)`, and the
inline-comment tool. The first four are beyond the workflow's own list.

The controls that remain are these, and none of them pins the command:

- the subprocess environment scrub and the isolation step in the workflow, which
  installs bubblewrap and socat and lifts Ubuntu's AppArmor restriction on
  unprivileged user namespaces for the job's ephemeral VM;
- the least-privilege job permissions described above;
- the author-association gate below, which limits who can start a run;
- a ruleset blocking direct pushes to the default branch, which the maintainer is
  adding and which is outside this repository's files;
- the vendor's own trust: a change to that repository's default branch is made by
  the organization that also supplies the action and the model.

Revisit the exception if the maintainer wants the command pinned; either route
above does it without a new action or Claude Code feature.

## Self-Hosted Runners Stay Out of This Repository's Own Jobs

This repository is public. GitHub advises against self-hosted runners on public
repositories, because a pull request from a fork can run arbitrary code on the
runner, and a persistent runner then lets that code outlive the job. Every
workflow in this repository therefore runs on a GitHub-hosted runner
(`ubuntu-latest`), its CI and its review alike. A `runs-on` naming `self-hosted`
is a review finding.

That rule is about this repository's own jobs. A runner image or host script kept
here is a product the repository ships for other repositories to use, and this
repository does not run its own jobs on it. Whichever repository registers such a
runner owns that decision, and the rules for a runner's design are below.
`axross` is a User account, not an organization, so org-level runners are
unavailable and registration is per repository.

## Shared Runner Storage Is a Cache-Poisoning Surface

GitHub Actions isolates a job into its own runner: a job sees only the secrets
and permissions its own workflow grants it. A self-hosted runner that mounts
storage shared between jobs, such as a named Docker volume holding a package
cache, breaks that isolation in one direction. Whatever a job writes into the
volume, a later job reads, and a later job may hold a secret or a write
permission the earlier one did not.

The threat model for any runner image or host script added here is therefore:

- a job that runs untrusted code, such as a pull request's scripts or a
  dependency's install hook, can plant a file in a shared cache;
- a later job that restores that cache executes or links the planted file with its
  own, possibly broader, privileges;
- the attacker never needs the later job's secret; they need only one write to
  storage the later job trusts.

Design rules that follow, which REVIEW.md applies:

- A shared volume is scoped to one repository and one trust level. A volume MUST
  NOT be shared across repositories, between a public and a private repository,
  or between runs of a pull request from a fork and runs on the default branch.
- A runner is ephemeral: it takes one job through a just-in-time registration,
  then its container is removed. A long-lived registration token is not stored.
- Volumes hold only content the tool re-verifies, such as a content-addressed
  package cache. A volume does not hold credentials, configuration, or tool
  binaries the job later executes.
- A job that holds a deployment secret does not mount a volume that a less
  trusted job could have written.
- A container does not run privileged, does not mount the host's container socket
  or home directory, and has its egress limited where the host's network reaches
  anything sensitive. A justified exception is stated where it is declared;
  [LAN egress](#lan-egress-from-runner-containers-accepted-risk) is one.
- Each shared mount is declared with the reason it exists and the trust level of
  every job that can write to it.

## Per-Repository Isolation on a Runner Host

A runner host serves several repositories from one machine, so each repository's
registration, storage, and containers are kept apart from every other's. The rules
for [`hosts/windows-docker-desktop/`](../../hosts/windows-docker-desktop) and any
later host:

- One token per target repository, a fine-grained personal access token limited to
  that repository's Administration permission. A token MUST NOT cover several
  repositories or be stored as a GitHub Actions secret. The token is read from a
  file on the host on every registration, and the file is restricted to the host
  user.
- A registration is made with the entry's own token and carries `self-hosted`,
  `linux`, `x64`, and at least one custom label, so a job's runner is identifiable
  and a repository opts in by naming the label. A configuration entry without a
  custom label MUST be rejected.
- A registration reaches the runner through the `ACTIONS_RUNNER_INPUT_JITCONFIG`
  environment variable, set only in the process that starts the container and
  passed to `docker run` by name. It MUST NOT be placed on a command line, in a log
  line, or in an image layer.
- Volume and container names derive from the host prefix, owner, and repository.
  An entry's volumes are mounted only into its own containers, two entries that
  would share a name or a prefix are rejected, and stale-container cleanup removes
  only containers matching the entry's own name pattern.
- A repository that can run pull requests from forks on a host's labels requires
  approval for outside collaborators' workflow runs, per the operator procedure in
  [Windows Runner Host](../operations/windows-runner-host.md#repository-settings-set-by-hand).
- `docker run` uses `--pull never`, so a missing local image fails instead of
  pulling a same-named public image.

## LAN Egress From Runner Containers (Accepted Risk)

The Windows host starts containers on Docker Desktop's default network, which
routes to the machine's local network as any process on the machine does. A job
can reach devices there, such as a router or a NAS, that a GitHub-hosted runner
cannot. This is an exception to the rule above that egress is limited where the
host's network reaches anything sensitive. The maintainer accepted the risk for
Phase 1, because the Docker Desktop mechanism to block it (a firewall rule for the
containers' subnet, or a custom network) has not been researched. The operator
procedure tells operators to run the host only on a network they would trust the
listed repositories' workflows with. Revisit the exception when a blocking
mechanism is verified.

## Nothing Identifying or Secret Is Committed

The repository is public and is meant to be reusable by anyone, so it holds no
credential and no value that identifies a person or a machine: no token, key,
registration or just-in-time configuration, `.env` value, hostname, IP address,
home-directory path, or account name, other than the public maintainer handle and
the owner and repository names of this repository and of `axross/skills`, and no
consumer-specific owner, repository, image, volume, or task name. Examples use
placeholders (`<owner>`, `<repo>`). A value a runner needs is read from the
environment or supplied at run time.

[`.gitignore`](../../.gitignore) excludes `settings.local.json`, `.env.local`,
token files, and private keys so that a local working file cannot be committed by
accident; it is a backstop, not the control. The only repository secret the
workflows read is `CLAUDE_CODE_OAUTH_TOKEN`, used by `claude-review.yaml` and
added by the maintainer. A secret value is never echoed, passed on a command line,
or written into an image layer or build cache.

## The Review Gate Admits One Bot Identity by Login, Not Association

`claude-review.yaml`'s reviewer job gates on the comment's `author_association`,
admitting only `OWNER`, `MEMBER`, and `COLLABORATOR`. The gate exists to keep an
untrusted author from spending this repository's tokens or steering the reviewer.
Review requests from the Claude Code route, when posted through the installed
Claude GitHub App, carry the `claude[bot]` identity, whose association GitHub
reports as `NONE`, the same value an outside contributor's comment carries, so
those requests were gated out identically to an outsider's. The gate's fourth
clause admits that one login directly, and further requires the comment to carry
no Markdown heading anywhere in it. That test does less than it looks like it does:
the reviewer's summary opens with a one-line tally, not a heading, so the test
normally does not exclude it.

Admitting a login rather than widening the association list does not widen who can
trigger a paid run: a comment under the `claude[bot]` identity can only be
produced by a session holding this repository's own operator credentials. The
reviewer also posts its own summary under that same identity, so a login-only
clause would admit the reviewer's own output back through the gate it fired from
and arm an unbounded review chain.

The real guard is a rule, not the heading test. [REVIEW.md](../../REVIEW.md)
forbids the reviewer from writing the review trigger phrase anywhere in a summary
or an inline comment. A summary that quoted the phrase, say while describing an
acceptance criterion, would satisfy every clause of the gate, since it comes from
`claude[bot]` and normally carries no Markdown heading, and would start another
review. The workflow's `!contains(body, '## ')` test only excludes a comment that
happens to carry a heading. Only a top-level summary can trigger the workflow,
since inline review comments arrive as a different event, but the rule covers both
so that it is simple to follow and to check.

Two properties keep the admission bounded, and a third does not hold:

- Execution of untrusted code stays closed: the job checks out the default
  branch, never the pull request head.
- The prompt is fixed, built from the pull request number and repository, never
  from the comment body, so a commenter's text is not an instruction.
- **Steering is not closed.** The pull request's title, description, diff,
  comments, and files are untrusted model input that the reviewer reads, and a
  prompt injection in them can redirect the reviewer within the tools it holds.
  Those tools are the workflow's `--allowedTools` plus whatever the unpinned
  command's frontmatter pre-approves, as described above. A `gh` command can name
  another repository with `-R`, so the command allowlist does not confine reads to
  this repository; only the repository scope of the App token does, which the
  action documents but this job cannot confirm. The token stays reachable to the
  job: the action exports it as `GH_TOKEN` and `GITHUB_TOKEN`, which `gh` needs, and
  writes it into the checkout's `.git/config` remote URL. It carries the installed
  App's permissions, which can include writing contents.
  The action documents its subprocess scrub as removing Anthropic, cloud, and
  GitHub Actions secrets from subprocess environments; this job relies on it for
  `CLAUDE_CODE_OAUTH_TOKEN` and the `ACTIONS_*` runtime tokens, not for the two App
  token variables.
  Blocking direct pushes to the default branch with a repository ruleset caps the
  damage, and is a maintainer decision made outside this repository.

The cost is a silent-skip failure mode. The gate trusts a login string; if that
login changes, the gate silently reverts to skipping the Claude Code route's
requests, with no failed run and no comment. The trigger-phrase rule carries the
opposite risk: if the reviewer quotes the phrase, a review starts that nobody
asked for. Neither is mechanically detected; both are found only by noticing that
reviews stopped arriving, or arrived when they should not have. Admitting the
`NONE` association itself was rejected outright, since that would admit every
outside author.
