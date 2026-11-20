#!/usr/bin/env bash
# the semantic-release gate fails closed unless its caller names the planning or
# publication phase. only publication requires the complete checked CLI set.
set -euo pipefail

if [[ $# -ne 2 ]]; then
  echo 'Usage: verify-release.sh <version> <directory>' >&2
  exit 2
fi
version="$1"
directory="$2"

refuse() {
  echo "error: $*" >&2
  exit 1
}

[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]] \
  || refuse "the version '${version}' is not X.Y.Z or X.Y.Z-<prerelease>"
case "${RELEASE_PHASE:-}" in
  plan)
    [[ -n "${GITHUB_OUTPUT:-}" ]] || refuse 'GITHUB_OUTPUT is not set, so the version cannot be handed to the later jobs'
    echo "version=${version}" >>"$GITHUB_OUTPUT"
    echo "The next release is ${version}."
    ;;
  release)
    windows="marshal-${version}-windows-x64.exe"
    windows_msi="marshal-${version}-windows-x64.msi"
    macos="marshal-${version}-macos-arm64"
    macos_pkg="marshal-${version}-macos-arm64.pkg"
    [[ -d "$directory" ]] || refuse "${directory} is not a directory"
    listed_names="$(printf '%s\n' "$macos" "$macos_pkg" "$windows" "$windows_msi" | LC_ALL=C sort)"
    expected="$(printf '%s\n' SHA256SUMS "$macos" "$macos_pkg" "$windows" "$windows_msi" | LC_ALL=C sort)"
    present="$(find "$directory" -mindepth 1 -maxdepth 1 -printf '%P\n' | LC_ALL=C sort)"
    [[ "$present" == "$expected" ]] \
      || refuse "the release files are not exactly ${macos}, ${macos_pkg}, ${windows}, ${windows_msi} and SHA256SUMS:
${present}"
    for file in SHA256SUMS "$macos" "$macos_pkg" "$windows" "$windows_msi"; do
      [[ ! -L "${directory}/${file}" && -f "${directory}/${file}" ]] \
        || refuse "${file} is not a regular file"
    done
    listed="$(awk '{ print $2 }' "${directory}/SHA256SUMS" | LC_ALL=C sort)"
    [[ "$listed" == "$listed_names" ]] \
      || refuse "SHA256SUMS does not list exactly ${macos}, ${macos_pkg}, ${windows} and ${windows_msi}"
    (cd "$directory" && sha256sum --check --strict SHA256SUMS) \
      || refuse 'a release file does not match SHA256SUMS'
    echo "The release files for ${version} are complete and match SHA256SUMS."
    ;;
  *) refuse "RELEASE_PHASE must be plan or release, not '${RELEASE_PHASE:-}'" ;;
esac
