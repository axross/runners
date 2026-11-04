#!/usr/bin/env bash
# writes SHA256SUMS for the release's four files and is the only thing that
# does: release-build.yaml's assemble job writes the unsigned set's, and
# release.yaml's assemble-signed job writes the signed set's. <directory> holds
# exactly the two binaries and the two installers named for <version>, and no
# SHA256SUMS. each file's SHA-256 must equal the hash handed over for it, so a
# file that changed since its hash was taken is refused. the four lines come out
# in a fixed order, and the result must pass `sha256sum --check --strict`.
# runs on Linux, where sha256sum is GNU's, as verify-release.sh does.
set -euo pipefail

if [[ $# -ne 6 ]]; then
  echo "Usage: assemble-release.sh <version> <directory> <windows exe sha256> <windows msi sha256> <macos sha256> <macos pkg sha256>" >&2
  exit 2
fi
version="$1"
directory="$2"
windows_sha256="$3"
windows_msi_sha256="$4"
macos_sha256="$5"
macos_pkg_sha256="$6"

refuse() {
  echo "error: $*" >&2
  exit 1
}

[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]] \
  || refuse "the version '${version}' is not X.Y.Z or X.Y.Z-<prerelease>"
[[ -d "$directory" ]] || refuse "${directory} is not a directory"

windows="marshal-${version}-windows-x64.exe"
windows_msi="marshal-${version}-windows-x64.msi"
macos="marshal-${version}-macos-arm64"
macos_pkg="marshal-${version}-macos-arm64.pkg"

expected="$(printf '%s\n' "$macos" "$macos_pkg" "$windows" "$windows_msi" | LC_ALL=C sort)"
present="$(find "$directory" -mindepth 1 -maxdepth 1 -printf '%P\n' | LC_ALL=C sort)"
if [[ "$present" != "$expected" ]]; then
  refuse "the files are not exactly ${macos}, ${macos_pkg}, ${windows} and ${windows_msi}:
${present}"
fi
for file in "$macos" "$macos_pkg" "$windows" "$windows_msi"; do
  if [[ -L "${directory}/${file}" || ! -f "${directory}/${file}" ]]; then
    refuse "${file} is not a regular file"
  fi
done

for hash in "$windows_sha256" "$windows_msi_sha256" "$macos_sha256" "$macos_pkg_sha256"; do
  [[ "$hash" =~ ^[0-9a-f]{64}$ ]] || refuse "'${hash}' is not a SHA-256"
done

# check_hash <file> <hash it must have>
check_hash() {
  local line actual
  line="$(sha256sum "${directory}/$1")"
  actual="${line%% *}"
  [[ "$actual" == "$2" ]] || refuse "$1 has SHA-256 ${actual}, not the ${2} it was handed"
}
check_hash "$macos" "$macos_sha256"
check_hash "$macos_pkg" "$macos_pkg_sha256"
check_hash "$windows" "$windows_sha256"
check_hash "$windows_msi" "$windows_msi_sha256"

printf '%s  %s\n' \
  "$macos_sha256" "$macos" \
  "$macos_pkg_sha256" "$macos_pkg" \
  "$windows_sha256" "$windows" \
  "$windows_msi_sha256" "$windows_msi" >"${directory}/SHA256SUMS"
(cd "$directory" && sha256sum --check --strict SHA256SUMS) \
  || refuse "SHA256SUMS does not match the files it was just written from"
cat "${directory}/SHA256SUMS"
