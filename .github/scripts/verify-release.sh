#!/usr/bin/env bash
# The release gate: @semantic-release/exec's verifyReleaseCmd in
# release/.releaserc.json. semantic-release runs it from release/ with the version
# it is about to release, in a dry run as well, so RELEASE_PHASE says which job of
# release.yaml is calling:
#
#   plan     append version=<version> to GITHUB_OUTPUT, for the later jobs.
#   release  refuse unless <directory> holds exactly the two binaries named for that
#            version and SHA256SUMS, SHA256SUMS lists exactly those two, and
#            `sha256sum --check --strict` passes. semantic-release has tagged and
#            published nothing yet, so a refusal stops the release.
#
# Any other RELEASE_PHASE, or none, is refused, so a job that forgets to set it
# cannot release. The attest job applies the same file check without this script,
# since it checks nothing out. It runs on Linux, where sha256sum is GNU's.
#
# Usage: verify-release.sh <version> <directory>
set -euo pipefail

if [[ $# -ne 2 ]]; then
  echo "Usage: verify-release.sh <version> <directory>" >&2
  exit 2
fi
version="$1"
directory="$2"

refuse() {
  echo "error: $*" >&2
  exit 1
}

case "${RELEASE_PHASE:-}" in
  plan)
    [[ -n "${GITHUB_OUTPUT:-}" ]] || refuse "GITHUB_OUTPUT is not set, so the version cannot be handed to the later jobs"
    echo "version=${version}" >>"$GITHUB_OUTPUT"
    echo "The next release is ${version}."
    ;;
  release)
    windows="marshal-${version}-windows-x64.exe"
    macos="marshal-${version}-macos-arm64"
    [[ -d "$directory" ]] || refuse "${directory} is not a directory"
    expected="$(printf '%s\n' SHA256SUMS "$macos" "$windows" | LC_ALL=C sort)"
    present="$(cd "$directory" && find . -mindepth 1 -maxdepth 1 | sed 's|^\./||' | LC_ALL=C sort)"
    if [[ "$present" != "$expected" ]]; then
      refuse "the release files are not exactly ${macos}, ${windows} and SHA256SUMS:
${present}"
    fi
    for file in SHA256SUMS "$macos" "$windows"; do
      [[ -f "${directory}/${file}" ]] || refuse "${file} is not a regular file"
    done
    listed="$(awk '{ print $2 }' "${directory}/SHA256SUMS" | LC_ALL=C sort)"
    if [[ "$listed" != "$(printf '%s\n' "$macos" "$windows" | LC_ALL=C sort)" ]]; then
      refuse "SHA256SUMS does not list exactly ${macos} and ${windows}"
    fi
    (cd "$directory" && sha256sum --check --strict SHA256SUMS) \
      || refuse "a release file does not match SHA256SUMS"
    echo "The release files for ${version} are complete and match SHA256SUMS."
    ;;
  *)
    refuse "RELEASE_PHASE must be plan or release, not '${RELEASE_PHASE:-}'"
    ;;
esac
