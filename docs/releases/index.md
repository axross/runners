# Releasing runners and Marshal

Change the version of a runner image, publish a release or maintain the credentials used to sign it.

## Runner image and release

- [Update the GitHub Actions runner](./runner-update.md) — Update the runner version, digest and image together and drain running jobs before rollout.
- [Publish or recover a release](./publish.md) — Prepare signing, check the planned release version, dispatch from main and recover a partial publication.
- [Release version selection](./versions.md) — Determine the version from tags and commit subjects, including the base tag that keeps releases on 0.x.
- [Renew release signing certificates](./signing-renewal.md) — Replace expiring signing certificates and verify the credentials used by the release workflow.
