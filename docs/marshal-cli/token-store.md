---
type: Specification
title: "Host token store"
description: "Which credentials can the agent read, replace or remove on Windows and macOS?"
---

# Host token store

## The Token Store

`token_store` is the one place the agent keeps a token. The interface names no
operating system: `get`, `set` and `remove` by a key of one of two kinds, each in
lower case. A repository key is `owner/name`, so `Axross/Runners` and
`axross/runners` are one token. An organization key is the organization's login
alone, matching the configuration's owner pattern. A repository key always holds one
slash and an organization key none, so the two never collide. `token_set` and
`token_remove` take a repository key only and refuse an organization's login.
A token is trimmed, then refused when it is empty or longer than 1280 UTF-16 code
units, which keeps it inside the Credential Manager's 2560 bytes.

- **Windows.** The Credential Manager of the current user, through
  `windows-native-keyring-store`, as one generic credential per key named
  `app.axross.marshal:github.com/<owner>/<name>` for a repository, or
  `app.axross.marshal:github.com/<org>` for an organization, with the token as
  UTF-16 and with local persistence. The crate's default persistence is the enterprise one, which
  roams with the user's profile, so the store asks for the local one.
  `tests/token_store_windows.rs` sets, reads and removes a real credential of each
  kind, a repository's and an organization's, checks its name with `cmdkey`, reads
  its persistence back from the store's `persistence` attribute, round-trips a token of 1280 UTF-16 units and
  a non-ASCII, non-BMP one, and sends a `token_set` through an agent whose store is
  the Credential Manager to a registration; only the Windows CI job runs it. Its tests
  take turns at the Credential Manager, as a precaution, because the crate's upstream
  reports it losing updates under concurrent use. The Windows CI job has intermittently
  seen a miss, where a read right after a successful write returned no value or the
  earlier one; the cause is not established, and
  [#148](https://github.com/axross/runners/issues/148) holds the investigation and the
  probe's result. The tests neither retry a read nor warm the
  Credential Manager up first, since either would hide a miss rather than remove
  its cause.
- **macOS.** The user's login keychain, through `security-framework`, as one generic
  password per key with the service `app.axross.marshal`. A repository's item has the
  account `github.com/<owner>/<name>` and the label `Marshal: <owner>/<name>`, and an
  organization's the account `github.com/<org>` and the label
  `Marshal: <org> (organisation)`; Keychain Access shows the label. Only a
  repository's account has a slash after `github.com/`, so the two kinds never share
  an item. These names are a contract K3, M4, X1 and X2 build on, and change only
  through a revision of M2's plan. The store is the file-based login keychain, the
  default keychain that adds go to, not the data-protection keychain. Setting a token
  deletes any item for the key and adds a new one, so the item trusts the build that
  stored it last; if the add fails after the delete, the key has no token. Removing
  deletes the item, and removing a key without one is not an error. The store turns
  the Keychain's user interaction off for as long as it lives, which is the agent's
  life, so a call that would need the user fails at once instead of showing a dialog.
  `tests/keychain_store.rs` sets, reads, replaces and removes a real item of each
  kind under a test-only service, checks each label with `security`, and checks that
  removing an organization's token keeps a repository's token of the same owner.
- **Linux.** There is no store. The agent runs with none, every pool is skipped with
  the reason "no token store", a `token_set` is refused as `store_failed`, and the
  agent is stopped for `no_pool_started`.
- **Tests.** An in-memory store, which can be made to fail with each kind of error.

A store failure has a kind, which says what it asks of the caller, and the platform's
own code when it gave one. The kinds are `needs_user`, such as a locked keychain the
user can unlock, `access_refused`, when the item refuses this program, and `other`.
The Credential Manager's failures are all `other`, with the Windows error number in
the code. The Keychain's codes `errSecInteractionNotAllowed` (-25308),
`errSecInteractionRequired` (-25315) and `errSecAuthFailed` (-25293), which a locked
keychain returns, are `needs_user`; `errSecItemNotFound` (-25300) is no token, not a
failure; every other code is `other`, with Apple's code. Until the Mac run shows
which code an item another build of Marshal added returns, a `needs_user` failure's
words name both causes, a locked login keychain or another build's item, and the
fix: unlock the keychain in Keychain Access, or enter the token again. A refused
delete also gives the one command the window shows,
`security delete-generic-password -s app.axross.marshal -a github.com/<owner>/<name>`
(`-a github.com/<org>` for an organisation's item). The kind and the code appear in the `store_failed` error (see Errors), in
the reason of a pool skipped because the store failed (`the token store failed: ...
(kind needs_user, code 1312)`), and in the log line of a registration that failed
for the same reason. None of them ever holds a token.

The agent reads a token from the store when a pool is admitted, again at each
registration, and at each GitHub status check, so a `token_set` takes effect at the
next registration and check, and a
`token_remove` makes the next registration fail and its slot back off with "no
token". A running job container keeps the JIT configuration it was started with.

`has_token` in the snapshot is the store's answer as of the last configuration
read, not a live reading: it refreshes only on a configuration read, a `token_set`
or a `token_remove`.
