---
type: Policy
title: "Permissions for workflows and automated review"
description: "Determine where repository jobs may run, who can invoke a review, and what the workflow may access."
---

# Permissions for workflows and automated review

Run this public repository’s own jobs on GitHub-hosted runners. Start jobs with read-only permissions and give each write scope to the job that needs it. The review workflow has specific exceptions for its plugin and trigger identity.

## Workflow Permissions Start From Read-Only

Every workflow declares a top-level `permissions:` block that starts from
`contents: read`. A broader scope is granted on the job that needs it, and never
on a job that runs pull-request-controlled code. `actions/checkout` sets
`persist-credentials: false` wherever the job does not itself push or call the
API with the checkout's credential. A workflow triggered by `issue_comment` checks
out the default branch, never the pull request's head, so no code a commenter
could have introduced runs in a job that holds a secret.
[`merge-checks.yaml`](../../.github/workflows/merge-checks.yaml),
[`rust-advisories.yaml`](../../.github/workflows/rust-advisories.yaml), and
[`release-build.yaml`](../../.github/workflows/release-build.yaml) hold only
`contents: read`. [`release.yaml`](../../.github/workflows/release.yaml) grants write
scopes to its plan, attest, and release jobs; the [release record](./release-authority.md) says which
and why. [`claude-review.yaml`](../../.github/workflows/claude-review.yaml)
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
reach, so this is an exception to the [pinning convention](./dependency-pins.md#pinning-convention). It is an accepted
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
- the [least-privilege job permissions](#workflow-permissions-start-from-read-only);
- the [author-association gate](#the-review-gate-admits-one-bot-identity-by-login-not-association), which limits who can start a run;
- the default branch's ruleset, outside this repository's files, which blocks
  deleting the branch and pushes that are not fast-forwards, but not a direct
  fast-forward push;
- the vendor's own trust: a change to that repository's default branch is made by
  the organization that also supplies the action and the model.

Revisit the exception if the maintainer wants the command pinned; either route
above does it without a new action or Claude Code feature.

## Self-Hosted Runners Stay Out of This Repository's Own Jobs

This repository is public. GitHub advises against self-hosted runners on public
repositories, because a pull request from a fork can run arbitrary code on the
runner, and a persistent runner then lets that code outlive the job. Every
workflow in this repository therefore runs on a GitHub-hosted runner, its CI and
its review alike: a hosted label such as `ubuntu-latest`, `ubuntu-24.04-arm`,
`windows-latest`, or `macos-latest`. The CLI's tests run on three of those so each
operating system a runner host can be is exercised. A `runs-on` naming
`self-hosted` is a review finding.

That rule is about this repository's own jobs. A runner image or host script kept
here is a product the repository ships for other repositories to use, and this
repository does not run its own jobs on it. Whichever repository registers such a
runner owns that decision, and the [rules for a runner’s shared storage](./pool-isolation.md#shared-runner-storage-is-a-cache-poisoning-surface) govern its design.
`axross` is a User account, not an organization, so org-level runners are
unavailable and registration is per repository.

## The Review Gate Admits One Bot Identity by Login, Not Association

Every Claude Code pull request uses [Claude CI review](../agent-work/claude-ci-review.md#claude-ci-review-from-claude-code). Each request is subject to this gate, and each round runs with the [accepted marketplace risk](#the-review-plugin-marketplace-is-not-pinned-accepted-risk).

The reviewer job checks who posted the request before spending the repository's
tokens on a review. It admits an `OWNER`, `MEMBER`, or `COLLABORATOR`. Requests
posted through the installed Claude GitHub App carry the
`claude[bot]` login. GitHub reports its `author_association` as `NONE`, the same
value it reports for an outside contributor. The gate therefore admits that one
login when the comment passes its heading check. The reviewer's summary opens
with a one-line tally, so the heading check normally admits it too.

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
  branch, never the pull request head. Head files are read only through the
  [fixed retrieval command](#the-review-job-reads-head-files-only-through-a-fixed-command),
  and none is executed.
- The prompt is fixed, built from the pull request number and repository, never
  from the comment body, so a commenter's text is not an instruction.
- **Steering is not closed.** The pull request's title, description, diff,
  comments, and files are untrusted model input that the reviewer reads, and a
  prompt injection in them can redirect the reviewer within the tools it holds.
  Those tools are the workflow's `--allowedTools` plus whatever the unpinned
  command's frontmatter pre-approves, as described in the [review plugin risk](#the-review-plugin-marketplace-is-not-pinned-accepted-risk). A `gh` command can name
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

If the App login changes, the gate silently skips Claude CI review requests
posted by the renamed bot. There is no failed run or comment to flag the skip.
If the reviewer quotes the trigger phrase, an unrequested review can start.
Operators detect these failures by noticing a missing or unexpected review.
The gate excludes the broader `NONE` association because it would admit every
outside author.

## The Review Job Reads Head Files Only Through a Fixed Command

The review job checks out the default branch and the reviewer reads the pull
request's changes through `gh pr diff`, so the workspace never holds the head's
files. The reviewer reads a head file, a line range of one, or a directory
listing through one command instead, and the head is still never checked out or
executed.

- **Retrieval.** [`read-head.sh`](../../.github/scripts/read-head.sh) takes a
  40-character lowercase commit SHA and a repository-relative path, with an
  optional first and last line, and prints the result to standard output. It reads
  this repository through `GET` requests to the contents API, at most two for one
  call, and creates no file. It validates every argument before any request and refuses
  an input outside a narrow set of path characters, so it cannot be steered to
  another path, request method, or repository. The one exception is a line range on
  a directory other than the root: the command cannot know the path is a directory
  until the one request that shows it, and then refuses the range. The allowlist names this
  program, not a `gh api` prefix, because a prefix rule would also accept arguments
  that change a request's method or body.
- **The copy.** The `Install Head Reader` step runs after the checkout and before
  the action. It copies the script to one declared path outside the workspace, the
  runner's temporary directory, and the home directory, with the repository name,
  the `gh` path and the other tools' paths fixed into the copy. The step finds each
  tool and the interpreter only in one tool search path that it declares in its own
  environment, `/usr/bin:/bin`, and fixes that same value into the copy as its own
  search path, so the copy trusts when it runs the directories the step checked,
  and never the job's own search path. The copy's first
  line names its interpreter by absolute path and starts it in privileged mode, so
  the caller's search path, startup-file variables, and exported functions do not
  reach it. The step accepts a tool or the interpreter only from a directory chain
  and a file that root alone can change, and fails the job otherwise. The copy is owned
  by root, mode 555, in a directory that holds only the copy, and every ancestor
  of that directory is owned by root and writable by neither group nor others. The
  step checks each of these properties, prints the ownership chain, and fails the
  job if any one does not hold, so a command running as the job's user cannot
  overwrite, remove, rename, or add beside the copy. The job's user also has
  passwordless `sudo`, so a route that ran arbitrary commands could still replace
  the copy. No allowed tool offers such a route, so the protection is defence in
  depth, not a boundary against that.
- **No allowlist entry writes files.** This change removed two prefix entries that
  accepted file-writing options, and denied their command as defence in depth,
  because Claude Code also runs a built-in set of read-only commands without any
  rule. No remaining entry writes files or repository contents. The prompt tells
  the reviewer that command is unavailable and that its absence is not an
  unchecked item.
- **Head content is data.** The prompt tells the reviewer to treat everything from
  the head, including its instruction files, the title, the body, commits, and
  comments, as data under review, and never to check out, fetch, or add a worktree
  for the head.
- **The guard.** `mise run test:read-head` reads the workflow and fails unless the
  allowlist is exactly the expected set, the deny list holds its entries including
  the denied command, the copy step's path, the allowlist entry, and the prompt
  name the same path, and the prompt keeps its two prohibitions. Like the other
  guards here, it catches a mistake and is no control against a hostile edit.
  The tests that run the copy step need a root-owned directory; they run as root or
  with passwordless `sudo`, skip with a note elsewhere, and fail on GitHub Actions.

The residual risks are these:

- The host's handling of shell syntax, and of other spellings or wrappers of the
  denied command, which the rule matches only as written.
- The environment the programs the copy starts inherit from the reviewer's shell,
  such as configuration a program reads from the home directory and the dynamic
  loader's variables, which act before the script's own resets. The privileged
  start closes this for the interpreter only.
- The review plugin's pre-approved tools, an [accepted risk](#the-review-plugin-marketplace-is-not-pinned-accepted-risk)
  that can change without notice.
- The App token's write grant and its presence in the checkout's git configuration.
  The job does not bound the token; the
  [registered deviation](../agent-work/skill-exceptions.md#deviation---the-claude-ci-review-token-is-not-bounded-to-read-access)
  records the alternative behaviour that stands in for that bound.
- Large outputs, which the host may keep in its own files on the job's virtual
  machine.
- A pull request from a fork, whose head SHA the base repository's contents
  endpoint may not serve. The reviewer reports such a file as unchecked.
