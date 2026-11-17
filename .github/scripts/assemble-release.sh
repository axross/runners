#!/usr/bin/env bash
# writes SHA256SUMS for a release's files and is the only thing that does:
# release-build.yaml's assemble job writes the unsigned set's, in `unsigned` mode,
# and release.yaml's assemble-signed job writes the signed set's, in `signed` mode.
# <directory> holds exactly the set's files named for <version>, and no
# SHA256SUMS: the app executable and the MSI, and in `signed` mode also the Mac's
# notarized disk image, which only the signed set holds. each file's SHA-256 must
# equal the hash handed over for it, so a file that changed since its hash was
# taken is refused. the lines come out in a fixed order, and the result must pass
# `sha256sum --check --strict`.
# runs on Linux, where sha256sum is GNU's, as verify-release.sh does.
set -euo pipefail

usage() {
  echo "Usage: assemble-release.sh unsigned <version> <directory> <windows exe sha256> <windows msi sha256>" >&2
  echo "       assemble-release.sh signed <version> <directory> <windows exe sha256> <windows msi sha256> <macos dmg sha256>" >&2
  exit 2
}

mode="${1:-}"
case "$mode" in
  unsigned) [[ $# -eq 5 ]] || usage ;;
  signed) [[ $# -eq 6 ]] || usage ;;
  *) usage ;;
esac
version="$2"
directory="$3"
windows_sha256="$4"
windows_msi_sha256="$5"
macos_dmg_sha256="${6:-}"

refuse() {
  echo "error: $*" >&2
  exit 1
}

[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]] \
  || refuse "the version '${version}' is not X.Y.Z or X.Y.Z-<prerelease>"
[[ -d "$directory" ]] || refuse "${directory} is not a directory"

windows="marshal-${version}-windows-x64.exe"
windows_msi="marshal-${version}-windows-x64.msi"
macos_dmg="marshal-${version}-macos-arm64.dmg"

files=("$windows" "$windows_msi")
hashes=("$windows_sha256" "$windows_msi_sha256")
if [[ "$mode" == signed ]]; then
  files+=("$macos_dmg")
  hashes+=("$macos_dmg_sha256")
fi

expected="$(printf '%s\n' "${files[@]}" | LC_ALL=C sort)"
present="$(find "$directory" -mindepth 1 -maxdepth 1 -printf '%P\n' | LC_ALL=C sort)"
if [[ "$present" != "$expected" ]]; then
  refuse "the ${mode} set's files are not exactly ${files[*]}:
${present}"
fi
for file in "${files[@]}"; do
  if [[ -L "${directory}/${file}" || ! -f "${directory}/${file}" ]]; then
    refuse "${file} is not a regular file"
  fi
done

for hash in "${hashes[@]}"; do
  [[ "$hash" =~ ^[0-9a-f]{64}$ ]] || refuse "'${hash}' is not a SHA-256"
done

# check_hash <file> <hash it must have>
check_hash() {
  local line actual
  line="$(sha256sum "${directory}/$1")"
  actual="${line%% *}"
  [[ "$actual" == "$2" ]] || refuse "$1 has SHA-256 ${actual}, not the ${2} it was handed"
}
for index in "${!files[@]}"; do
  check_hash "${files[$index]}" "${hashes[$index]}"
done
for index in "${!files[@]}"; do
  printf '%s  %s\n' "${hashes[$index]}" "${files[$index]}"
done >"${directory}/SHA256SUMS"
if ! (cd "$directory" && sha256sum --check --strict SHA256SUMS); then
  rm -f "${directory}/SHA256SUMS"
  refuse "SHA256SUMS does not match the files it was just written from"
fi
cat "${directory}/SHA256SUMS"
