---
type: Policy
title: "Credentials and identifying information"
description: "Decide what belongs in public source or logs and where the maintainer has accepted a bounded signing exception."
---

# Credentials and identifying information

Review configuration, logs, sources and release commands for values that identify a private host or grant access. The signing exceptions below apply to particular commands and credentials.

## Nothing Identifying or Secret Is Committed

The repository is public and is meant to be reusable by anyone, so it holds no
credential and no value that identifies a person or a machine: no token, key,
registration or just-in-time configuration, `.env` value, hostname, IP address,
home-directory path, or account name, other than the public maintainer handle and
the owner and repository names of this repository and of `axross/skills`, and no
consumer-specific owner, repository, image, volume, or task name. Examples use
placeholders (`<owner>`, `<repo>`). A value a runner needs is read from the
environment or supplied at run time. The exceptions are the two default runner labels, `axpc`
and `axmc`, which a host adds when `labels` is absent or empty and which are
committed by the maintainer's decision: `axpc` names the Windows Docker Desktop
host and `axmc` the macOS host. Every other host, consumer, or machine value stays
a parameter. A further exception is the macOS signing team ID and the exact
designated requirement, which a documentation change records in the CLI document
after the first signed release as the contract text for later work. The team ID is
public in every signed file, so recording it discloses nothing; it is not an account
credential. Another is the maintainer's update-feed public key, committed in
`app/crates/app/tauri.conf.json`, and the separate release-verification public key in
`app/crates/feed/release-public-key.txt`, with the tests that pin each. The app and
the publication check need these public values in their builds, and a public key lets
no one sign or sign in. The two may differ during a rotation. This exception permits
only those public sources and their pinning tests, never a private key or a password;
the key's custody is in [Updates](../marshal/updates.md#the-update-feed-key).

The checked-in licence notices under `app/notices/` reproduce third-party licence
texts and copyright notices verbatim, as those licences require, including the names
and e-mail addresses of the people they name. That text is the one exception to the
rule against committing personal names, account names and e-mail addresses. It covers
only verbatim licence and copyright text that `app/notices/generate.sh` produces or
that the repository's licence copies hold (`app/notices/licences/` and the licence
files beside the window's bundled fonts and colours). It also covers the published
repository URL that `generate.sh` writes into the notices for each crate, and a
reference in `docs/marshal/notices.md` to a third-party crate's public repository,
today `wravery/webview2-rs` and `madsmtm/objc2`; an account name, personal name or
e-mail address of any other kind, or anywhere else, is still a finding.

[`.gitignore`](../../.gitignore) excludes `settings.local.json`, `.env.local`,
token files, and private keys so that a local working file cannot be committed by
accident; it is a backstop, not the control. The workflows read one repository
secret, `CLAUDE_CODE_OAUTH_TOKEN`, used by `claude-review.yaml` and added by the
maintainer. The `release` Environment keeps six Environment secrets, read only
by the temporary CLI signing job and retained for M2's later signing job:
`APPLE_APPLICATION_P12_BASE64`, `APPLE_INSTALLER_P12_BASE64`, `APPLE_P12_PASSWORD`,
`APPLE_NOTARY_KEY_P8`, `APPLE_NOTARY_TEAM_KEY_ID` and `APPLE_NOTARY_ISSUER_ID`, and one
Environment variable that is not secret, `APPLE_TEAM_ID`; the maintainer creates
them, and no value is committed. W4's two update-feed secrets,
`UPDATE_FEED_SIGNING_KEY` and `UPDATE_FEED_SIGNING_KEY_PASSWORD`, are retained for its
`sign-update` job, which reads them only in its signing step through `env:`, and no
job reads them until that job is in the release run. A secret value is never echoed, passed on a command
line, or written into an image layer or build cache. Two exceptions for a
credential on a command line are recorded in `sign-macos` for the temporary CLI
and M2's later app signing. The password
arguments of `security` were allowed at the plan gate. The `--key-id` and `--issuer`
arguments of `notarytool`, which carry the API key's ID and issuer ID, were allowed
by the maintainer on 2026-11-04 PST. Neither tool has a non-interactive alternative:
`security` has none for its password, and `notarytool` and its `store-credentials`
command take the IDs only as arguments or through a keychain profile that is itself
created from those arguments, with no documented environment variable or
configuration file. The runner is discarded after the job, and a request is
authenticated by a token signed with the `.p8` private key, which reaches `notarytool`
only as a file path. GitHub masks secret values in the
log, not in a process listing.
