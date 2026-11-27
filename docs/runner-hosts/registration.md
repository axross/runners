---
type: Procedure
title: "Register a pool with GitHub"
description: "Choose a repository or organisation token, restrict the jobs that may use it, and grant optional access to job names."
---

# Register a pool with GitHub

## Repository Settings (Set by Hand)

For each target repository, under **Settings, Actions, General, Fork pull
request workflows**, choose the option that requires approval for workflow runs
from outside collaborators. Do not list a public repository whose fork pull
requests run on these labels until that is set. The requirement and its reason
are in [Per-Repository Isolation on a Runner Host](../security/pool-isolation.md#per-repository-isolation-on-a-runner-host). A job runs on this host when every label
in its `runs-on` is one the registration carries, so `runs-on: axpc` selects it,
and so does `runs-on: self-hosted`.

## The Fine-Grained Token

Marshal asks GitHub for a single-use runner registration through
[`POST /repos/{owner}/{repo}/actions/runners/generate-jitconfig`](https://docs.github.com/en/rest/actions/self-hosted-runners),
which needs the **Administration** repository permission set to **Read and
write**. Create one **fine-grained personal access token per target repository**,
under **Settings, Developer settings, Fine-grained personal access tokens**:

- **Resource owner:** the repository's owner. For a repository owned by an
  organisation, select that organisation, not your personal account.
- **Repository access:** only that one repository.
- **Permissions:** Administration, read and write. Actions, read, is optional: with
  it the agent can name the job a busy runner runs, at the cost the token permission
  section of [Security](../security/pool-isolation.md#a-token-carries-only-its-pool-kinds-permissions)
  names. Nothing else.

An organisation that blocks fine-grained tokens does not appear as a resource
owner. When the organisation requires approval, a member who is not an owner must
have an owner approve the token before starting the pool. A pending token can read
public resources only; an organisation owner's token needs no separate approval.

Choose an expiration within the organisation's maximum lifetime policy. GitHub
defaults that policy to 366 days, and the organisation can shorten it. GitHub
refuses registration when a token is:

- Expired.
- Awaiting required approval.
- Refused by the organisation's policy.

A runner whose registration fails shows "Backing off" with GitHub's answer,
scrubbed of the token, and the agent's log keeps the same answer. Check the
token's settings and organisation policy on GitHub rather than assuming every
failure means the token's value is wrong. GitHub's
[token creation steps](https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/managing-your-personal-access-tokens)
and [organisation token policies](https://docs.github.com/en/organizations/managing-programmatic-access-to-your-organization/setting-a-personal-access-token-policy-for-your-organization)
describe those controls.

Enter each token in Marshal's window: **Open Marshal**, the **Tokens** tab, then
**Set Token** on the repository's row, paste the token and **Save**. Marshal keeps it
in the Windows Credential Manager, one entry per repository, and never shows it
again; the pools of that repository share it. See how [the agent stores and reads tokens](../security/local-agent.md#the-agent-keeps-tokens-in-the-operating-systems-store)
and [which repository pools may share them](../security/pool-isolation.md#per-repository-isolation-on-a-runner-host).

The agent reads the token from the store at every registration, so a rotated token
applies to the next job in every slot with no restart. Rotate a token with
**Replace Token** on its row, and remove one with **Remove Token**.

### Organisation Pools

A pool configured with `organization` uses an organisation token, not any of that
organisation's repository tokens. Before starting one, satisfy the Organisation
Pools Share a Runner Group's Trust Boundary section of
[Security](../security/pool-isolation.md#organisation-pools-share-a-runner-groups-trust-boundary). [TOML Configuration](../marshal-cli/toml-configuration.md#the-toml-configuration) describes `organization` and `runner_group`.

Create the token while signed in as an owner of the organisation:

1. Open **Settings, Developer settings, Fine-grained personal access tokens** and
   choose **Generate new token**.
2. Select the organisation as **Resource owner**.
3. Select **Public repositories (read-only)** under **Repository access**. Do not
   select private repositories; this token is not used to read repository data.
4. Set the organisation permission **Self-hosted runners** to **Read and write**.
   Add no repository permissions.
5. Choose an expiration within the organisation's lifetime policy and generate
   the token.
6. Open Marshal's **Tokens** tab and find the organisation under
   **Organisations**. Choose **Set Token**, paste the token and choose **Save**.

Every organisation pool with that login uses this token. Saving it starts pools
skipped for **no token** while the agent is running. Use **Replace Token** to
rotate it; the next registration reads the replacement. **Remove Token** stops
those pools from getting new runners, but runners already up finish their job.
Repository rows keep their separate tokens.

Removing an organisation's last pool does not remove its token, and the window then
shows no row to remove it from. Before removing the last pool for a login, choose
**Remove Token** on the organisation's row. If the pool is already gone, delete the
Credential Manager entry `app.axross.marshal:github.com/<org>`.

If GitHub refuses registration, the pool's runners show "Backing off" with
GitHub's scrubbed answer. Check the token's resource owner and permission, then
the organisation's token policy; Marshal cannot distinguish every policy refusal
from a permission failure.

### Adding Actions: read for Job Names

Marshal's window names the job each busy runner runs, and links to it, only when the
repository's token also has **Actions**, **Read-only**. Without it the window still
shows the runner as running a job, and the pool says "Job names hidden". To add it, on
GitHub open **Settings, Developer settings, Fine-grained personal access tokens**,
choose the repository's token, **Edit**, and under **Repository permissions** set
**Actions** to **Read-only**, then **Update**. Editing a token's permissions keeps its
value, so nothing changes on this machine: the token stays where it is stored, and the
next job's name shows within a minute of a runner becoming busy, without a restart.

Actions: read also lets anyone holding the token read the repository's workflow runs,
their logs and their artifacts; the token permission section of
[Security](../security/pool-isolation.md#a-token-carries-only-its-pool-kinds-permissions)
names that cost. Leave it off to keep the token to Administration alone.

### Token Files of the Command Line

The command line read each token from a file at the pool's `tokenPath`, which its
own access control step restricted. Marshal reads no token file and imports none:
enter each repository's token in the window once, as [The Fine-Grained Token](#the-fine-grained-token) describes. Keep the old token files
until the app's runners work, because rolling back to the command line reads them,
then delete them. The [historical Windows host guide](https://github.com/axross/runners/blob/dc8e1ebcc0722997206487f3dcca2ae7e519cf02/docs/operations/windows-runner-host.md#restricting-a-token-file) gives the access-control steps for those files.
