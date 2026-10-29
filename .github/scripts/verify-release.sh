#!/usr/bin/env bash
# the release gate: verifyReleaseCmd in release/.releaserc.json, run from
# release/ with the version about to be released, in a dry run as well.
# RELEASE_PHASE names the release.yaml job calling it: plan hands the version on
# through GITHUB_OUTPUT; release refuses unless the directory holds exactly the
# two binaries named for the version and a SHA256SUMS that lists only them and
# passes `sha256sum --check --strict`, before anything is tagged or published.
# any other value is refused, so a job that forgets it cannot release.
# runs on Linux, where sha256sum is GNU's.
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
