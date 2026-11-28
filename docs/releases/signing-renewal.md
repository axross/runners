---
type: Procedure
title: Renew release signing certificates
description: Replace expiring signing certificates and verify the credentials used by the release workflow.
---

# Renew release signing certificates

## Renewing the Certificates

This section is held for M2 and W4: no job signs from W1's installer pull request
until M2's `sign-macos` returns, and this yearly work applies to it from then.

The Developer ID certificates are expected to last one year, which the maintainer
confirms from the certificates' dates, and renewing them is yearly work for the
maintainer, with the Account Holder, before they expire. `sign-macos` prints both
expiry dates, warns within 90 days, and refuses an expired certificate, so a lapse
stops signing and no release can be made until the certificates are renewed. Software
that was signed with a secure timestamp keeps working after its certificate expires.

1. Start when `sign-macos` warns, or earlier from the recorded dates. The Account
   Holder creates a new Developer ID Application certificate and a new Developer ID
   Installer certificate from the Developer ID G2 intermediate. A lapsed Apple
   Developer Program membership allows no new certificate, so renew the membership
   first.
2. Export each new identity as a `.p12` with one password, base64-encode them, and
   replace the secrets `APPLE_APPLICATION_P12_BASE64`, `APPLE_INSTALLER_P12_BASE64`
   and, if the password changed, `APPLE_P12_PASSWORD`. Do not change the identifier
   or `APPLE_TEAM_ID`: the identifier and the team stay the same, so the designated
   requirement is expected to stay the same and the Keychain trust it carries is kept.
   Keep one identity of each kind in the exported files, since `sign-macos` refuses
   two of the same kind.
3. Record the new expiry dates on the tracking issue. If the notarization key was
   renewed or revoked, replace its three secrets as well.
4. The first release after a renewal compares the bare binary's designated
   requirement with the recorded one, and records whether a package signed before the
   renewal still installs, which Apple's pages do not settle.

The embedded G2 intermediate expires on 2031-09-17. Replacing it takes a pull request
that changes the embedded certificate and its pin together, under
[Pinning Convention](../security/dependency-pins.md#pinning-convention).
