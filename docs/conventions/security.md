# Security

This project's supply-chain and runner-trust conventions: how a `uses:` reference
under [`.github/`](../../.github) is pinned, what a workflow may be granted, where
a self-hosted runner may and may not run, what a runner's shared storage exposes,
and what is never committed. It does not cover application-level security, such as
input validation or the OWASP-style concerns the installed `application-security`
capability owns.

The runner images, host scripts, and agent-host configuration this repository is
for do not exist yet. The sections on runner trust and shared storage are the
constraints those changes will be reviewed against, not a description of anything
running today.

## Pinning Convention

A third-party action, anything outside the `actions/` GitHub organization, is
pinned to the full 40-character commit SHA behind its latest release, with that
release's tag kept as a trailing comment for a human to read:

```yaml
uses: owner/action@d34db33fd34db33fd34db33fd34db33fd34db33 # v1.2.3
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
checksum before it runs. [`.agents/setup`](../../.agents/setup) does this for
mise, and [`mise.toml`](../../mise.toml) pins every tool to an exact version.
[`mise.lock`](../../mise.lock), with the npm dependency locks it references under
`.mise/locks/`, records each tool's download URL and checksum for `linux-x64`;
`locked = true` is deliberately not set, so a platform the lockfile does not cover
still installs. Dependabot's `github-actions` entry refreshes action pins monthly;
the versions in `mise.toml` are refreshed by hand, looked up with `mise latest`,
never recalled. A manual refresh of a tool version is: edit `mise.toml`, run
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

## The Review Plugin Marketplace Is Pinned by Checkout

`claude-review.yaml` runs the `code-review` plugin's command, which is model
instructions the job executes with shell tools, a repository secret, and an
OpenID Connect token in reach. Its source is therefore pinned like any other
code the job runs. The action cannot pin a Git URL marketplace: at its pinned SHA
it validates each URL entry against a pattern that must end in `.git`, so a
`#ref` or `?ref=` suffix is rejected, and it has no ref input. It does accept a
local path, so the workflow checks `anthropics/claude-code` out at a full commit
SHA into `.review-marketplace` (with `persist-credentials: false`) and passes
`./.review-marketplace` as `plugin_marketplaces`.

The pin is the control, not the `--allowedTools` list. A command's own
`allowed-tools` frontmatter pre-approves tools in addition to that list. At the
pinned commit, `plugins/code-review/commands/code-review.md` grants
`Bash(gh issue view:*)`, `Bash(gh search:*)`, `Bash(gh issue list:*)`,
`Bash(gh pr list:*)`, `Bash(gh pr comment:*)`, `Bash(gh pr diff:*)`,
`Bash(gh pr view:*)`, and the inline-comment tool. Four of those
(`gh issue view`, `gh search`, `gh issue list`, `gh pr list`) are beyond the
workflow's own list. An unpinned marketplace would let a later commit widen that
grant unseen.

Dependabot does not bump a `with.ref`, so the pin is refreshed by hand:

1. Resolve the new commit with `git ls-remote https://github.com/anthropics/claude-code HEAD`
   (or a tag, peeled with `^{}`), never from memory.
2. Read `plugins/code-review/commands/code-review.md` at that commit, and compare
   its `allowed-tools` frontmatter and its stop conditions with the list above
   and with the `--append-system-prompt` text in the workflow. A new tool in the
   frontmatter is a review finding until it is accepted here.
3. Update `ref:` in the `Checkout Review Plugin Marketplace` step, then open a pull request like any other
   change to CI. The plugin also loads under the marketplace name in its
   `.claude-plugin/marketplace.json`, which must still be `claude-code-plugins`
   for the `plugins:` entry to resolve.

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
  anything sensitive. A justified exception is stated where it is declared.
- Each shared mount is declared with the reason it exists and the trust level of
  every job that can write to it.

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
no Markdown heading anywhere in it.

Admitting a login rather than widening the association list does not widen who can
trigger a paid run: a comment under the `claude[bot]` identity can only be
produced by a session holding this repository's own operator credentials. The
reviewer also posts its own summary under that same identity, so a login-only
clause would admit the reviewer's own output back through the gate it fired from
and arm an unbounded review chain.

Two rules close that side, and only the first is mechanical. The workflow's
`!contains(body, '## ')` test excludes any comment that carries a Markdown
heading, and the reviewer's summary usually opens with one. Nothing guarantees
that, so the second rule is the real guard: [REVIEW.md](../../REVIEW.md) forbids
the reviewer from writing the review trigger phrase anywhere in a summary or an
inline comment. A summary that quoted the phrase, say while describing an
acceptance criterion, and carried no Markdown heading would satisfy every clause of
the gate and start another review. Only the top-level summary can do this, since
inline review comments arrive as a different event, but the rule covers both so
that it is simple to follow and to check.

Three properties keep the admission bounded, and one does not hold:

- Execution of untrusted code stays closed: the job checks out the default
  branch, never the pull request head.
- The prompt is fixed, built from the pull request number and repository, never
  from the comment body, so a commenter's text is not an instruction.
- **Steering is not closed.** The pull request's title, description, diff,
  comments, and files are untrusted model input that the reviewer reads, and a
  prompt injection in them can redirect the reviewer within the tools it holds.
  The tools are narrowed to `gh pr` and `git` read and comment subcommands, but
  the Claude GitHub App token stays reachable to the job: the action exports it
  as `GH_TOKEN` and writes it into the checkout's `.git/config` remote URL. The
  token carries the installed App's permissions, which can include writing
  contents. `CLAUDE_CODE_SUBPROCESS_ENV_SCRUB` makes a best-effort attempt to
  keep secrets out of the model's shell and is not relied on for this.
  Blocking direct pushes to the default branch with a repository ruleset would
  cap the damage, and whether to add one is a maintainer decision not taken here.

The cost is a silent-skip failure mode. The gate trusts a login string; if that
login changes, the gate silently reverts to skipping the Claude Code route's
requests, with no failed run and no comment. The heading test carries the same
risk in the other direction if the reviewer's summary ever stops carrying a
heading. Neither is mechanically detected; both are found only by noticing that
reviews stopped arriving, or arrived when they should not have. Admitting the
`NONE` association itself was rejected outright, since that would admit every
outside author.
